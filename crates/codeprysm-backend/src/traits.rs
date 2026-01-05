//! Backend trait definition.
//!
//! Defines the async interface for code search and graph operations.

use async_trait::async_trait;
use codeprysm_search::ProviderStatus;

use crate::error::BackendError;
use crate::types::{EdgeInfo, GraphStats, IndexStatus, NodeInfo, SearchOptions, SearchResult};

/// Backend for code search and graph operations.
///
/// This trait defines the unified interface implemented by both local and remote backends.
/// All operations are async to support network-based backends.
#[async_trait]
pub trait Backend: Send + Sync {
    /// Perform a code search with the given query.
    ///
    /// # Arguments
    /// * `query` - Search query (natural language or code pattern)
    /// * `limit` - Maximum number of results
    /// * `options` - Optional search filters and settings
    ///
    /// # Returns
    /// Vector of search results sorted by relevance score.
    async fn search(
        &self,
        query: &str,
        limit: usize,
        options: Option<SearchOptions>,
    ) -> Result<Vec<SearchResult>, BackendError>;

    /// Get information about a specific node.
    ///
    /// # Arguments
    /// * `node_id` - The node ID (e.g., "src/lib.rs:MyStruct")
    ///
    /// # Returns
    /// Node information if found.
    async fn get_node(&self, node_id: &str) -> Result<NodeInfo, BackendError>;

    /// Get nodes connected to the given node.
    ///
    /// # Arguments
    /// * `node_id` - The source node ID
    /// * `edge_type` - Optional edge type filter (e.g., "Contains", "Uses")
    /// * `direction` - Edge direction: "outgoing", "incoming", or "both"
    ///
    /// # Returns
    /// Vector of connected nodes.
    async fn get_connected_nodes(
        &self,
        node_id: &str,
        edge_type: Option<&str>,
        direction: &str,
    ) -> Result<Vec<NodeInfo>, BackendError>;

    /// Get edges for a node.
    ///
    /// # Arguments
    /// * `node_id` - The node ID
    /// * `edge_type` - Optional edge type filter
    /// * `direction` - Edge direction: "outgoing", "incoming", or "both"
    ///
    /// # Returns
    /// Vector of edges.
    async fn get_edges(
        &self,
        node_id: &str,
        edge_type: Option<&str>,
        direction: &str,
    ) -> Result<Vec<EdgeInfo>, BackendError>;

    /// Get the search index status.
    ///
    /// # Returns
    /// Index status with counts and metadata.
    async fn index_status(&self) -> Result<IndexStatus, BackendError>;

    /// Get graph statistics.
    ///
    /// # Returns
    /// Statistics about the code graph.
    async fn graph_stats(&self) -> Result<GraphStats, BackendError>;

    /// Read code content for a node.
    ///
    /// # Arguments
    /// * `node_id` - The node ID
    /// * `context_lines` - Number of context lines before/after
    ///
    /// # Returns
    /// Code content as a string.
    async fn read_code(&self, node_id: &str, context_lines: usize) -> Result<String, BackendError>;

    /// Find nodes by pattern.
    ///
    /// # Arguments
    /// * `pattern` - Name pattern (supports * wildcards)
    /// * `node_type` - Optional node type filter
    /// * `limit` - Maximum results
    ///
    /// # Returns
    /// Matching nodes.
    async fn find_nodes(
        &self,
        pattern: &str,
        node_type: Option<&str>,
        limit: usize,
    ) -> Result<Vec<NodeInfo>, BackendError>;

    /// Index the graph for search.
    ///
    /// # Arguments
    /// * `force` - Force re-indexing even if index exists
    ///
    /// # Returns
    /// Number of entities indexed.
    async fn index(&self, force: bool) -> Result<usize, BackendError>;

    /// Sync the graph (reload from storage).
    ///
    /// # Returns
    /// Whether any changes were detected.
    async fn sync(&self) -> Result<bool, BackendError>;

    /// Get the repository identifier.
    fn repo_id(&self) -> &str;

    /// Check if the backend is healthy and connected.
    async fn health_check(&self) -> Result<bool, BackendError>;

    /// Check embedding provider status.
    ///
    /// # Returns
    /// Status of the embedding provider including health and readiness.
    async fn check_provider(&self) -> Result<ProviderStatus, BackendError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    // Test that the trait is object-safe
    fn _assert_object_safe(_: &dyn Backend) {}
}
