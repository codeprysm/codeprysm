//! Multi-workspace backend for cross-workspace operations.
//!
//! Provides aggregated search and query operations across multiple registered workspaces.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::future::join_all;
use tracing::{debug, info, warn};

use crate::error::BackendError;
use crate::registry::WorkspaceRegistry;
use crate::traits::Backend;
use crate::types::{EdgeInfo, GraphStats, IndexStatus, NodeInfo, SearchOptions, SearchResult};
use crate::LocalBackend;

/// Backend that aggregates operations across multiple workspaces.
///
/// Implements the `Backend` trait by:
/// - Searching all workspaces in parallel and merging results
/// - Prefixing node IDs with workspace name for disambiguation
/// - Aggregating stats from all workspaces
pub struct MultiWorkspaceBackend {
    /// Workspace registry for accessing individual backends
    registry: Arc<WorkspaceRegistry>,
}

impl MultiWorkspaceBackend {
    /// Create a new multi-workspace backend.
    pub fn new(registry: Arc<WorkspaceRegistry>) -> Self {
        Self { registry }
    }

    /// Create from an existing registry.
    pub async fn from_registry(registry: WorkspaceRegistry) -> Self {
        Self {
            registry: Arc::new(registry),
        }
    }

    /// Get the underlying registry.
    pub fn registry(&self) -> &WorkspaceRegistry {
        &self.registry
    }

    /// Get all healthy backends for searching.
    async fn get_search_backends(&self) -> Vec<(String, Arc<LocalBackend>)> {
        let workspaces = self.registry.list().await;
        let mut backends = Vec::new();

        for ws in workspaces {
            if ws.has_graph {
                match self.registry.backend(&ws.name).await {
                    Ok(backend) => backends.push((ws.name.clone(), backend)),
                    Err(e) => {
                        warn!("Skipping workspace '{}': {}", ws.name, e);
                    }
                }
            }
        }

        backends
    }

    /// Prefix a node ID with workspace name.
    fn prefix_node_id(workspace: &str, node_id: &str) -> String {
        format!("{}::{}", workspace, node_id)
    }

    /// Parse a prefixed node ID into (workspace, node_id).
    fn parse_node_id(prefixed: &str) -> Option<(&str, &str)> {
        prefixed.split_once("::")
    }

    /// Merge and deduplicate search results from multiple workspaces.
    ///
    /// Results are sorted by score (descending) and limited to `limit`.
    fn merge_results(
        mut results: Vec<(String, Vec<SearchResult>)>,
        limit: usize,
    ) -> Vec<SearchResult> {
        // Flatten all results with workspace prefixes
        let mut all_results: Vec<SearchResult> = results
            .drain(..)
            .flat_map(|(workspace, ws_results)| {
                ws_results.into_iter().map(move |mut r| {
                    // Prefix entity_id with workspace for disambiguation
                    r.entity_id = Self::prefix_node_id(&workspace, &r.entity_id);
                    // Add workspace to sources
                    r.sources.push(format!("workspace:{}", workspace));
                    r
                })
            })
            .collect();

        // Sort by score descending
        all_results.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Deduplicate by entity_id (keep highest score)
        let mut seen = std::collections::HashSet::new();
        all_results.retain(|r| seen.insert(r.entity_id.clone()));

        // Limit results
        all_results.truncate(limit);

        all_results
    }
}

#[async_trait]
impl Backend for MultiWorkspaceBackend {
    async fn search(
        &self,
        query: &str,
        limit: usize,
        options: Option<SearchOptions>,
    ) -> Result<Vec<SearchResult>, BackendError> {
        let backends = self.get_search_backends().await;

        if backends.is_empty() {
            return Err(BackendError::with_context(
                "multi-workspace search",
                "no workspaces with graphs available",
            ));
        }

        info!("Searching {} workspaces for: '{}'", backends.len(), query);

        // Search all workspaces in parallel
        // Request more results per workspace to allow for good merging
        let per_workspace_limit = limit * 2;
        let opts = options.clone();

        let futures: Vec<_> = backends
            .iter()
            .map(|(name, backend)| {
                let name = name.clone();
                let backend = Arc::clone(backend);
                let query = query.to_string();
                let opts = opts.clone();

                async move {
                    match backend.search(&query, per_workspace_limit, opts).await {
                        Ok(results) => {
                            debug!("Workspace '{}': {} results", name, results.len());
                            (name, results)
                        }
                        Err(e) => {
                            warn!("Search failed for '{}': {}", name, e);
                            (name, Vec::new())
                        }
                    }
                }
            })
            .collect();

        let results = join_all(futures).await;

        // Merge and return
        Ok(Self::merge_results(results, limit))
    }

    async fn get_node(&self, node_id: &str) -> Result<NodeInfo, BackendError> {
        // Parse workspace::node_id format
        let (workspace, actual_node_id) = Self::parse_node_id(node_id).ok_or_else(|| {
            BackendError::with_context(
                "get_node",
                format!(
                    "invalid node ID '{}', expected format 'workspace::node_id'",
                    node_id
                ),
            )
        })?;

        let backend = self.registry.backend(workspace).await?;
        let mut node = backend.get_node(actual_node_id).await?;

        // Prefix the ID in the response
        node.id = Self::prefix_node_id(workspace, &node.id);

        Ok(node)
    }

    async fn get_connected_nodes(
        &self,
        node_id: &str,
        edge_type: Option<&str>,
        direction: &str,
    ) -> Result<Vec<NodeInfo>, BackendError> {
        let (workspace, actual_node_id) = Self::parse_node_id(node_id).ok_or_else(|| {
            BackendError::with_context("get_connected_nodes", "invalid node ID format")
        })?;

        let backend = self.registry.backend(workspace).await?;
        let nodes = backend
            .get_connected_nodes(actual_node_id, edge_type, direction)
            .await?;

        // Prefix all node IDs
        Ok(nodes
            .into_iter()
            .map(|mut n| {
                n.id = Self::prefix_node_id(workspace, &n.id);
                n
            })
            .collect())
    }

    async fn get_edges(
        &self,
        node_id: &str,
        edge_type: Option<&str>,
        direction: &str,
    ) -> Result<Vec<EdgeInfo>, BackendError> {
        let (workspace, actual_node_id) = Self::parse_node_id(node_id)
            .ok_or_else(|| BackendError::with_context("get_edges", "invalid node ID format"))?;

        let backend = self.registry.backend(workspace).await?;
        let edges = backend
            .get_edges(actual_node_id, edge_type, direction)
            .await?;

        // Prefix edge node IDs
        Ok(edges
            .into_iter()
            .map(|mut e| {
                e.from_id = Self::prefix_node_id(workspace, &e.from_id);
                e.to_id = Self::prefix_node_id(workspace, &e.to_id);
                e
            })
            .collect())
    }

    async fn index_status(&self) -> Result<IndexStatus, BackendError> {
        let backends = self.get_search_backends().await;

        if backends.is_empty() {
            return Ok(IndexStatus::empty());
        }

        // Aggregate status from all workspaces
        let futures: Vec<_> = backends
            .iter()
            .map(|(name, backend)| {
                let name = name.clone();
                let backend = Arc::clone(backend);
                async move {
                    match backend.index_status().await {
                        Ok(status) => Some((name, status)),
                        Err(e) => {
                            warn!("Failed to get index status for '{}': {}", name, e);
                            None
                        }
                    }
                }
            })
            .collect();

        let statuses: Vec<_> = join_all(futures).await.into_iter().flatten().collect();

        if statuses.is_empty() {
            return Ok(IndexStatus::empty());
        }

        // Aggregate counts
        let total_semantic: u64 = statuses.iter().map(|(_, s)| s.semantic_count).sum();
        let total_code: u64 = statuses.iter().map(|(_, s)| s.code_count).sum();

        Ok(IndexStatus::existing(total_semantic, total_code))
    }

    async fn graph_stats(&self) -> Result<GraphStats, BackendError> {
        let backends = self.get_search_backends().await;

        if backends.is_empty() {
            return Err(BackendError::with_context(
                "graph_stats",
                "no workspaces available",
            ));
        }

        // Aggregate stats from all workspaces
        let futures: Vec<_> = backends
            .iter()
            .map(|(name, backend)| {
                let name = name.clone();
                let backend = Arc::clone(backend);
                async move {
                    match backend.graph_stats().await {
                        Ok(stats) => Some((name, stats)),
                        Err(e) => {
                            warn!("Failed to get graph stats for '{}': {}", name, e);
                            None
                        }
                    }
                }
            })
            .collect();

        let all_stats: Vec<_> = join_all(futures).await.into_iter().flatten().collect();

        if all_stats.is_empty() {
            return Err(BackendError::with_context(
                "graph_stats",
                "no workspace stats available",
            ));
        }

        // Aggregate
        let mut total_node_count = 0;
        let mut total_edge_count = 0;
        let mut total_file_count = 0;
        let mut total_component_count = 0;
        let mut nodes_by_type: HashMap<String, usize> = HashMap::new();
        let mut edges_by_type: HashMap<String, usize> = HashMap::new();

        for (_, stats) in all_stats {
            total_node_count += stats.node_count;
            total_edge_count += stats.edge_count;
            total_file_count += stats.file_count;
            total_component_count += stats.component_count;

            for (k, v) in stats.nodes_by_type {
                *nodes_by_type.entry(k).or_insert(0) += v;
            }
            for (k, v) in stats.edges_by_type {
                *edges_by_type.entry(k).or_insert(0) += v;
            }
        }

        Ok(GraphStats {
            node_count: total_node_count,
            nodes_by_type,
            edge_count: total_edge_count,
            edges_by_type,
            file_count: total_file_count,
            component_count: total_component_count,
        })
    }

    async fn read_code(&self, node_id: &str, context_lines: usize) -> Result<String, BackendError> {
        let (workspace, actual_node_id) = Self::parse_node_id(node_id)
            .ok_or_else(|| BackendError::with_context("read_code", "invalid node ID format"))?;

        let backend = self.registry.backend(workspace).await?;
        backend.read_code(actual_node_id, context_lines).await
    }

    async fn find_nodes(
        &self,
        pattern: &str,
        node_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<NodeInfo>, BackendError> {
        let backends = self.get_search_backends().await;

        if backends.is_empty() {
            return Ok(Vec::new());
        }

        // Search all workspaces in parallel
        let per_workspace_limit = limit;
        let node_type_owned = node_type.map(|s| s.to_string());

        let futures: Vec<_> = backends
            .iter()
            .map(|(name, backend)| {
                let name = name.clone();
                let backend = Arc::clone(backend);
                let pattern = pattern.to_string();
                let node_type = node_type_owned.clone();

                async move {
                    match backend
                        .find_nodes(&pattern, node_type.as_deref(), per_workspace_limit)
                        .await
                    {
                        Ok(nodes) => (
                            name.clone(),
                            nodes
                                .into_iter()
                                .map(|mut n| {
                                    n.id = Self::prefix_node_id(&name, &n.id);
                                    n
                                })
                                .collect::<Vec<_>>(),
                        ),
                        Err(e) => {
                            warn!("find_nodes failed for '{}': {}", name, e);
                            (name, Vec::new())
                        }
                    }
                }
            })
            .collect();

        let results = join_all(futures).await;

        // Flatten and limit
        let mut all_nodes: Vec<NodeInfo> =
            results.into_iter().flat_map(|(_, nodes)| nodes).collect();
        all_nodes.truncate(limit);

        Ok(all_nodes)
    }

    async fn index(&self, force: bool) -> Result<usize, BackendError> {
        let backends = self.get_search_backends().await;

        if backends.is_empty() {
            return Err(BackendError::with_context(
                "index",
                "no workspaces to index",
            ));
        }

        info!("Indexing {} workspaces", backends.len());

        let mut total_indexed = 0;

        // Index sequentially to avoid overwhelming Qdrant
        for (name, backend) in backends {
            match backend.index(force).await {
                Ok(count) => {
                    info!("Indexed {} entities in '{}'", count, name);
                    total_indexed += count;
                }
                Err(e) => {
                    warn!("Failed to index '{}': {}", name, e);
                }
            }
        }

        Ok(total_indexed)
    }

    async fn sync(&self) -> Result<bool, BackendError> {
        let backends = self.get_search_backends().await;

        if backends.is_empty() {
            return Ok(false);
        }

        let mut any_changed = false;

        for (name, backend) in backends {
            match backend.sync().await {
                Ok(changed) => {
                    if changed {
                        info!("Workspace '{}' synced with changes", name);
                        any_changed = true;
                    }
                }
                Err(e) => {
                    warn!("Failed to sync '{}': {}", name, e);
                }
            }
        }

        Ok(any_changed)
    }

    fn repo_id(&self) -> &str {
        "multi-workspace"
    }

    async fn health_check(&self) -> Result<bool, BackendError> {
        let backends = self.get_search_backends().await;

        if backends.is_empty() {
            return Ok(false);
        }

        // Check at least one backend is healthy
        for (name, backend) in backends {
            match backend.health_check().await {
                Ok(true) => {
                    debug!("Workspace '{}' is healthy", name);
                    return Ok(true);
                }
                Ok(false) => {
                    debug!("Workspace '{}' health check failed", name);
                }
                Err(e) => {
                    warn!("Health check error for '{}': {}", name, e);
                }
            }
        }

        Ok(false)
    }

    async fn check_provider(&self) -> Result<codeprysm_search::ProviderStatus, BackendError> {
        // Provider status is the same across all workspaces since they share
        // the same embedding provider. Check from any available backend.
        let backends = self.get_search_backends().await;

        for (name, backend) in backends {
            match backend.check_provider().await {
                Ok(status) => {
                    debug!(
                        "Provider status from '{}': semantic_ready={}, code_ready={}",
                        name, status.semantic_ready, status.code_ready
                    );
                    return Ok(status);
                }
                Err(e) => {
                    warn!("Provider check error for '{}': {}", name, e);
                }
            }
        }

        Err(BackendError::with_context(
            "check_provider",
            "No backends available to check provider status",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prefix_node_id() {
        assert_eq!(
            MultiWorkspaceBackend::prefix_node_id("project-a", "src/lib.rs:MyStruct"),
            "project-a::src/lib.rs:MyStruct"
        );
    }

    #[test]
    fn test_parse_node_id() {
        let result = MultiWorkspaceBackend::parse_node_id("project-a::src/lib.rs:MyStruct");
        assert_eq!(result, Some(("project-a", "src/lib.rs:MyStruct")));

        let result = MultiWorkspaceBackend::parse_node_id("invalid");
        assert!(result.is_none());
    }

    #[test]
    fn test_merge_results() {
        let results = vec![
            (
                "ws-a".to_string(),
                vec![
                    SearchResult::new("id1", "name1", 0.9),
                    SearchResult::new("id2", "name2", 0.7),
                ],
            ),
            (
                "ws-b".to_string(),
                vec![
                    SearchResult::new("id3", "name3", 0.8),
                    SearchResult::new("id4", "name4", 0.6),
                ],
            ),
        ];

        let merged = MultiWorkspaceBackend::merge_results(results, 3);

        assert_eq!(merged.len(), 3);
        // Should be sorted by score descending
        assert!(merged[0].score >= merged[1].score);
        assert!(merged[1].score >= merged[2].score);
        // IDs should be prefixed
        assert!(merged[0].entity_id.contains("::"));
    }

    #[test]
    fn test_merge_results_deduplication() {
        // Same entity ID in different workspaces (shouldn't happen, but test anyway)
        let results = vec![
            (
                "ws-a".to_string(),
                vec![SearchResult::new("id1", "name1", 0.9)],
            ),
            (
                "ws-b".to_string(),
                vec![SearchResult::new("id1", "name1", 0.7)], // Lower score
            ),
        ];

        let merged = MultiWorkspaceBackend::merge_results(results, 10);

        // Should have 2 results (different prefixes)
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn test_merge_results_limit() {
        let results = vec![(
            "ws-a".to_string(),
            vec![
                SearchResult::new("id1", "name1", 0.9),
                SearchResult::new("id2", "name2", 0.8),
                SearchResult::new("id3", "name3", 0.7),
                SearchResult::new("id4", "name4", 0.6),
            ],
        )];

        let merged = MultiWorkspaceBackend::merge_results(results, 2);
        assert_eq!(merged.len(), 2);
    }
}
