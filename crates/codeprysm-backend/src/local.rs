//! Local backend implementation.
//!
//! Provides direct access to:
//! - File system for code reading
//! - SQLite partitions for graph storage (via LazyGraphManager)
//! - Qdrant for semantic search

use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use codeprysm_config::PrismConfig;
use codeprysm_core::lazy::LazyGraphManager;
use codeprysm_core::{EdgeType, NodeType, PetCodeGraph};
use codeprysm_search::{EmbeddingConfig, GraphIndexer, HybridSearcher, QdrantConfig};
use regex::Regex;
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::error::BackendError;
use crate::traits::Backend;
use crate::types::{EdgeInfo, GraphStats, IndexStatus, NodeInfo, SearchOptions, SearchResult};

/// Local backend with direct file system and Qdrant access.
pub struct LocalBackend {
    /// Repository identifier
    repo_id: String,

    /// Workspace root directory
    workspace_root: PathBuf,

    /// Prism configuration
    config: PrismConfig,

    /// Lazy graph manager (handles partition loading)
    graph_manager: Arc<RwLock<Option<LazyGraphManager>>>,

    /// Qdrant configuration
    qdrant_config: QdrantConfig,
}

impl LocalBackend {
    /// Create a new local backend.
    ///
    /// # Arguments
    /// * `config` - Prism configuration
    /// * `workspace_root` - Path to the workspace root
    ///
    /// # Returns
    /// A new LocalBackend instance.
    pub async fn new(
        config: &PrismConfig,
        workspace_root: impl AsRef<Path>,
    ) -> Result<Self, BackendError> {
        let workspace_root = workspace_root.as_ref().to_path_buf();

        // Derive repo_id from workspace path
        let repo_id = workspace_root
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown")
            .to_string();

        let qdrant_config = QdrantConfig {
            url: config.backend.qdrant.url.clone(),
            api_key: config.backend.qdrant.api_key.clone(),
            ..Default::default()
        };

        Ok(Self {
            repo_id,
            workspace_root,
            config: config.clone(),
            graph_manager: Arc::new(RwLock::new(None)),
            qdrant_config,
        })
    }

    /// Create a local backend with a specific repo ID.
    pub async fn with_repo_id(
        config: &PrismConfig,
        workspace_root: impl AsRef<Path>,
        repo_id: impl Into<String>,
    ) -> Result<Self, BackendError> {
        let mut backend = Self::new(config, workspace_root).await?;
        backend.repo_id = repo_id.into();
        Ok(backend)
    }

    /// Get the CodePrysm data directory for this workspace.
    pub fn prism_dir(&self) -> PathBuf {
        self.config.prism_dir(&self.workspace_root)
    }

    /// Get the graph manifest path.
    pub fn graph_path(&self) -> PathBuf {
        self.config.graph_path(&self.workspace_root)
    }

    /// Check if the graph exists.
    pub fn graph_exists(&self) -> bool {
        self.graph_path().exists()
    }

    /// Load the graph if not already loaded.
    async fn ensure_graph(&self) -> Result<(), BackendError> {
        let needs_load = {
            let guard = self.graph_manager.read().await;
            guard.is_none()
        };

        if needs_load {
            self.load_graph().await?;
        }

        Ok(())
    }

    /// Load the graph from storage using LazyGraphManager.
    async fn load_graph(&self) -> Result<(), BackendError> {
        let prism_dir = self.prism_dir();

        if !prism_dir.exists() {
            return Err(BackendError::graph_not_found(&prism_dir));
        }

        let manifest_path = prism_dir.join("manifest.json");
        if !manifest_path.exists() {
            return Err(BackendError::graph_not_found(&manifest_path));
        }

        info!("Loading graph from {:?}", prism_dir);

        let manager = LazyGraphManager::open(&prism_dir)?;

        let mut guard = self.graph_manager.write().await;
        *guard = Some(manager);

        debug!("Graph loaded successfully");
        Ok(())
    }

    /// Get a read-only reference to the graph.
    async fn with_graph<F, R>(&self, f: F) -> Result<R, BackendError>
    where
        F: FnOnce(&PetCodeGraph) -> Result<R, BackendError>,
    {
        self.ensure_graph().await?;

        let guard = self.graph_manager.read().await;
        let manager = guard
            .as_ref()
            .ok_or_else(|| BackendError::with_context("graph access", "graph not loaded"))?;

        let graph_guard = manager.graph_read();
        f(&graph_guard)
    }

    /// Create a HybridSearcher for this backend.
    async fn create_searcher(&self) -> Result<HybridSearcher, BackendError> {
        // Convert codeprysm_config embedding settings to codeprysm_search embedding config
        let embedding_config = self.to_embedding_config();
        let searcher = HybridSearcher::connect_from_config(
            self.qdrant_config.clone(),
            &embedding_config,
            &self.repo_id,
        )
        .await?;
        Ok(searcher)
    }

    /// Convert config's embedding settings to search crate's EmbeddingConfig
    fn to_embedding_config(&self) -> EmbeddingConfig {
        use codeprysm_config::EmbeddingProviderType;
        use codeprysm_search::{AzureMLAuth, AzureMLConfig, OpenAIConfig};

        match self.config.embedding.provider {
            EmbeddingProviderType::Local => EmbeddingConfig::local(),
            EmbeddingProviderType::AzureMl => {
                if let Some(ref azure) = self.config.embedding.azure_ml {
                    // Resolve semantic auth: use env var name if specified
                    let semantic_auth = if let Some(ref env_var) = azure.semantic_auth_key_env {
                        AzureMLAuth::ApiKeyEnv(env_var.clone())
                    } else if let Some(ref env_var) = azure.auth_key_env {
                        // Legacy single key support
                        AzureMLAuth::ApiKeyEnv(env_var.clone())
                    } else {
                        AzureMLAuth::ApiKeyEnv("CODEPRYSM_AZURE_ML_SEMANTIC_API_KEY".to_string())
                    };

                    // Resolve code auth: use env var name if specified, else None (falls back to semantic)
                    let code_auth = azure
                        .code_auth_key_env
                        .as_ref()
                        .map(|env_var| AzureMLAuth::ApiKeyEnv(env_var.clone()));

                    let config = AzureMLConfig {
                        semantic_endpoint: azure.semantic_endpoint.clone(),
                        code_endpoint: azure.code_endpoint.clone(),
                        semantic_auth,
                        code_auth,
                        timeout_secs: azure.timeout_secs,
                        max_retries: azure.max_retries,
                    };
                    EmbeddingConfig::azure_ml_with_config(config)
                } else {
                    // No settings provided, let factory read from environment
                    EmbeddingConfig::azure_ml()
                }
            }
            EmbeddingProviderType::Openai => {
                if let Some(ref openai) = self.config.embedding.openai {
                    // Resolve API key from env var if specified
                    let api_key = openai
                        .api_key_env
                        .as_ref()
                        .and_then(|env_var| std::env::var(env_var).ok());

                    let config = OpenAIConfig {
                        base_url: openai.url.clone(),
                        api_key,
                        semantic_model: openai.semantic_model.clone(),
                        code_model: openai.code_model.clone(),
                        timeout_secs: openai.timeout_secs,
                        max_retries: openai.max_retries,
                        azure_mode: openai.azure_mode,
                    };
                    EmbeddingConfig::openai_with_config(config)
                } else {
                    // No settings provided, let factory read from environment
                    EmbeddingConfig::openai()
                }
            }
        }
    }

    /// Read file content for a node.
    fn read_file_content(
        &self,
        file_path: &str,
        start_line: usize,
        end_line: usize,
        context: usize,
    ) -> Result<String, BackendError> {
        let full_path = self.workspace_root.join(file_path);

        if !full_path.exists() {
            return Err(BackendError::with_context(
                "reading file",
                format!("file not found: {}", file_path),
            ));
        }

        let content = std::fs::read_to_string(&full_path)?;
        let lines: Vec<&str> = content.lines().collect();

        let start = start_line.saturating_sub(1).saturating_sub(context);
        let end = std::cmp::min(end_line + context, lines.len());

        let selected: Vec<&str> = lines[start..end].to_vec();
        Ok(selected.join("\n"))
    }

    /// Parse edge type from string.
    fn parse_edge_type(s: &str) -> Option<EdgeType> {
        match s.to_lowercase().as_str() {
            "contains" => Some(EdgeType::Contains),
            "uses" => Some(EdgeType::Uses),
            "defines" => Some(EdgeType::Defines),
            "dependson" | "depends_on" => Some(EdgeType::DependsOn),
            _ => None,
        }
    }
}

#[async_trait]
impl Backend for LocalBackend {
    async fn search(
        &self,
        query: &str,
        limit: usize,
        options: Option<SearchOptions>,
    ) -> Result<Vec<SearchResult>, BackendError> {
        let searcher = self.create_searcher().await?;

        // Check if index exists
        if searcher.is_index_empty().await? {
            return Err(BackendError::index_not_available(
                "Index is empty. Run indexing first.",
            ));
        }

        let opts = options.unwrap_or_default();

        // Convert node_types filter to Vec<&str>
        let type_filter: Vec<&str> = opts.node_types.iter().map(|s| s.as_str()).collect();

        // Perform search based on mode
        let results = match opts.mode.as_deref() {
            Some("code") | Some("info") => {
                searcher
                    .search_by_mode(query, limit, type_filter, opts.mode.as_deref())
                    .await?
            }
            _ => {
                // Default hybrid search
                if type_filter.is_empty() {
                    searcher.search(query, limit).await?
                } else {
                    searcher
                        .search_by_mode(query, limit, type_filter, None)
                        .await?
                }
            }
        };

        // Convert to SearchResult
        let mut converted: Vec<SearchResult> =
            results.into_iter().map(SearchResult::from).collect();

        // Apply minimum score filter if specified
        if let Some(min_score) = opts.min_score {
            converted.retain(|r| r.score >= min_score);
        }

        Ok(converted)
    }

    async fn get_node(&self, node_id: &str) -> Result<NodeInfo, BackendError> {
        self.with_graph(|graph| {
            graph
                .get_node(node_id)
                .map(NodeInfo::from_node)
                .ok_or_else(|| BackendError::node_not_found(node_id))
        })
        .await
    }

    async fn get_connected_nodes(
        &self,
        node_id: &str,
        edge_type: Option<&str>,
        direction: &str,
    ) -> Result<Vec<NodeInfo>, BackendError> {
        let edge_type_filter = edge_type.and_then(Self::parse_edge_type);

        self.with_graph(|graph| {
            // Verify node exists
            if graph.get_node(node_id).is_none() {
                return Err(BackendError::node_not_found(node_id));
            }

            let mut nodes = Vec::new();

            // Get edges based on direction
            let include_outgoing = direction == "outgoing" || direction == "both";
            let include_incoming = direction == "incoming" || direction == "both";

            for edge in graph.iter_edges() {
                let matches_filter =
                    edge_type_filter.is_none() || edge_type_filter == Some(edge.edge_type);

                if include_outgoing && edge.source == node_id && matches_filter {
                    if let Some(target) = graph.get_node(&edge.target) {
                        nodes.push(NodeInfo::from_node(target));
                    }
                }

                if include_incoming && edge.target == node_id && matches_filter {
                    if let Some(source) = graph.get_node(&edge.source) {
                        nodes.push(NodeInfo::from_node(source));
                    }
                }
            }

            Ok(nodes)
        })
        .await
    }

    async fn get_edges(
        &self,
        node_id: &str,
        edge_type: Option<&str>,
        direction: &str,
    ) -> Result<Vec<EdgeInfo>, BackendError> {
        let edge_type_filter = edge_type.and_then(Self::parse_edge_type);

        self.with_graph(|graph| {
            // Verify node exists
            if graph.get_node(node_id).is_none() {
                return Err(BackendError::node_not_found(node_id));
            }

            let mut edges = Vec::new();

            let include_outgoing = direction == "outgoing" || direction == "both";
            let include_incoming = direction == "incoming" || direction == "both";

            for edge in graph.iter_edges() {
                let is_outgoing = edge.source == node_id;
                let is_incoming = edge.target == node_id;

                if (is_outgoing && include_outgoing) || (is_incoming && include_incoming) {
                    let matches_filter =
                        edge_type_filter.is_none() || edge_type_filter == Some(edge.edge_type);

                    if matches_filter {
                        let mut metadata = std::collections::HashMap::new();
                        if let Some(ref ident) = edge.ident {
                            metadata.insert("ident".to_string(), ident.clone());
                        }
                        if let Some(ref version) = edge.version_spec {
                            metadata.insert("version_spec".to_string(), version.clone());
                        }
                        if let Some(dev) = edge.is_dev_dependency {
                            metadata.insert("is_dev_dependency".to_string(), dev.to_string());
                        }

                        edges.push(EdgeInfo {
                            from_id: edge.source.clone(),
                            to_id: edge.target.clone(),
                            edge_type: format!("{:?}", edge.edge_type),
                            metadata,
                        });
                    }
                }
            }

            Ok(edges)
        })
        .await
    }

    async fn index_status(&self) -> Result<IndexStatus, BackendError> {
        let searcher = self.create_searcher().await?;

        match searcher.index_status().await? {
            Some((semantic, code)) => Ok(IndexStatus::existing(semantic, code)),
            None => Ok(IndexStatus::empty()),
        }
    }

    async fn graph_stats(&self) -> Result<GraphStats, BackendError> {
        // Need to load all partitions to get accurate stats
        self.ensure_graph().await?;

        let guard = self.graph_manager.read().await;
        let manager = guard
            .as_ref()
            .ok_or_else(|| BackendError::with_context("graph stats", "graph not loaded"))?;

        // Load all partitions for accurate count
        manager.load_all_partitions()?;

        let graph_guard = manager.graph_read();
        Ok(GraphStats::from_graph(&graph_guard))
    }

    async fn read_code(&self, node_id: &str, context_lines: usize) -> Result<String, BackendError> {
        let (file_path, start_line, end_line) = self
            .with_graph(|graph| {
                let node = graph
                    .get_node(node_id)
                    .ok_or_else(|| BackendError::node_not_found(node_id))?;

                Ok((node.file.clone(), node.line, node.end_line))
            })
            .await?;

        self.read_file_content(&file_path, start_line, end_line, context_lines)
    }

    async fn find_nodes(
        &self,
        pattern: &str,
        node_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<NodeInfo>, BackendError> {
        let node_type_filter: Option<NodeType> =
            node_type.and_then(|t| match t.to_lowercase().as_str() {
                "container" => Some(NodeType::Container),
                "callable" => Some(NodeType::Callable),
                "data" => Some(NodeType::Data),
                _ => None,
            });

        self.with_graph(|graph| {
            let mut results = Vec::new();

            // Simple pattern matching (supports * as wildcard)
            let regex_pattern = pattern.replace("*", ".*");
            let re = Regex::new(&format!("^{}$", regex_pattern))
                .map_err(|e| BackendError::with_context("pattern parsing", e.to_string()))?;

            for node in graph.iter_nodes() {
                if node_type_filter.is_some() && Some(node.node_type) != node_type_filter {
                    continue;
                }

                if re.is_match(&node.name) {
                    results.push(NodeInfo::from_node(node));
                    if results.len() >= limit {
                        break;
                    }
                }
            }

            Ok(results)
        })
        .await
    }

    async fn index(&self, force: bool) -> Result<usize, BackendError> {
        // Load graph first
        self.ensure_graph().await?;

        // Create indexer with embedding config
        let embedding_config = self.to_embedding_config();
        let mut indexer = GraphIndexer::from_config(
            self.qdrant_config.clone(),
            &embedding_config,
            &self.repo_id,
            &self.workspace_root,
        )
        .await?;

        // Check if we need to re-index
        if !force {
            let status = indexer.store().collection_info("semantic_search").await?;
            if status.is_some() {
                info!("Index already exists, skipping (use force=true to re-index)");
                return Ok(0);
            }
        }

        info!(
            "Indexing graph for '{}' using partition-by-partition approach",
            self.repo_id
        );

        // Ensure collections exist and clear existing points
        indexer.store().ensure_collections().await?;
        indexer.clear_repo_points().await?;

        // Get partition IDs upfront
        let partition_ids = {
            let guard = self.graph_manager.read().await;
            let manager = guard
                .as_ref()
                .ok_or_else(|| BackendError::with_context("indexing", "graph not loaded"))?;
            manager.partition_ids()
        };

        let total_partitions = partition_ids.len();
        info!("Found {} partitions to index", total_partitions);

        let mut total_indexed = 0usize;

        // Process each partition independently to minimize memory usage
        for (idx, partition_id) in partition_ids.iter().enumerate() {
            // Load partition and extract its nodes (clone to avoid holding lock)
            let (nodes, graph_clone) = {
                let guard = self.graph_manager.read().await;
                let manager = guard
                    .as_ref()
                    .ok_or_else(|| BackendError::with_context("indexing", "graph not loaded"))?;

                // Load the partition
                manager.load_partition(partition_id).map_err(|e| {
                    BackendError::with_context(
                        "indexing",
                        format!("Failed to load partition {}: {}", partition_id, e),
                    )
                })?;

                // Get node IDs for this partition
                let node_ids = manager
                    .node_ids_in_partition(partition_id)
                    .unwrap_or_default();

                // Get actual nodes from graph
                let graph_guard = manager.graph_read();
                let nodes: Vec<_> = node_ids
                    .iter()
                    .filter_map(|id| graph_guard.get_node(id).cloned())
                    .collect();

                // Clone graph for SemanticTextBuilder context (needed for parent info)
                // Note: This is still a full clone per partition - we could optimize further
                // by loading only the current partition's data into a mini-graph
                let graph_clone = (*graph_guard).clone();

                (nodes, graph_clone)
            };
            // Lock is dropped here

            if nodes.is_empty() {
                continue;
            }

            // Index this partition's nodes
            let stats = indexer.index_nodes(&nodes, &graph_clone).await?;
            total_indexed += stats.total_indexed;

            if (idx + 1) % 10 == 0 || idx == total_partitions - 1 {
                info!(
                    "Progress: {}/{} partitions, {} entities indexed",
                    idx + 1,
                    total_partitions,
                    total_indexed
                );
            }

            // Unload the partition to free memory
            {
                let guard = self.graph_manager.read().await;
                if let Some(manager) = guard.as_ref() {
                    manager.unload_partition(partition_id);
                }
            }
        }

        info!(
            "Indexed {} entities across {} partitions",
            total_indexed, total_partitions
        );
        Ok(total_indexed)
    }

    async fn sync(&self) -> Result<bool, BackendError> {
        // Force reload of graph
        let prism_dir = self.prism_dir();

        if !prism_dir.exists() {
            warn!("CodePrysm directory not found at {:?}", prism_dir);
            return Ok(false);
        }

        info!("Syncing graph from {:?}", prism_dir);

        let new_manager = LazyGraphManager::open(&prism_dir)?;

        let mut guard = self.graph_manager.write().await;
        *guard = Some(new_manager);

        Ok(true)
    }

    fn repo_id(&self) -> &str {
        &self.repo_id
    }

    async fn health_check(&self) -> Result<bool, BackendError> {
        // Check graph exists
        if !self.graph_exists() {
            return Ok(false);
        }

        // Try to connect to Qdrant
        let store =
            codeprysm_search::QdrantStore::connect(self.qdrant_config.clone(), &self.repo_id).await;

        Ok(store.is_ok())
    }

    async fn check_provider(&self) -> Result<codeprysm_search::ProviderStatus, BackendError> {
        // Create provider from config and check status
        let embedding_config = self.to_embedding_config();
        let provider = codeprysm_search::create_provider(&embedding_config).map_err(|e| {
            BackendError::with_context(
                "check_provider",
                format!("Failed to create embedding provider: {}", e),
            )
        })?;

        provider.check_status().await.map_err(|e| {
            BackendError::with_context(
                "check_provider",
                format!("Provider status check failed: {}", e),
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_edge_type() {
        assert_eq!(
            LocalBackend::parse_edge_type("Contains"),
            Some(EdgeType::Contains)
        );
        assert_eq!(LocalBackend::parse_edge_type("uses"), Some(EdgeType::Uses));
        assert_eq!(
            LocalBackend::parse_edge_type("DEFINES"),
            Some(EdgeType::Defines)
        );
        assert_eq!(
            LocalBackend::parse_edge_type("depends_on"),
            Some(EdgeType::DependsOn)
        );
        assert_eq!(
            LocalBackend::parse_edge_type("DependsOn"),
            Some(EdgeType::DependsOn)
        );
        assert_eq!(LocalBackend::parse_edge_type("invalid"), None);
    }
}
