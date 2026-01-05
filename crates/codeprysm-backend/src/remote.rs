//! Remote backend stub for future HTTP-based CodePrysm server.
//!
//! This module provides a placeholder implementation that will be expanded
//! when the Prism HTTP server is implemented.

use async_trait::async_trait;

use crate::error::BackendError;
use crate::traits::Backend;
use crate::types::{EdgeInfo, GraphStats, IndexStatus, NodeInfo, SearchOptions, SearchResult};

/// Remote backend for connecting to a CodePrysm server over HTTP.
///
/// **Note**: This is currently a stub implementation. Full HTTP support
/// will be added in a future release.
pub struct RemoteBackend {
    /// Server URL
    server_url: String,

    /// API key for authentication
    api_key: Option<String>,

    /// Repository identifier
    repo_id: String,

    /// Request timeout in seconds
    timeout_secs: u64,
}

impl RemoteBackend {
    /// Create a new remote backend.
    ///
    /// # Arguments
    /// * `server_url` - CodePrysm server URL (e.g., "http://localhost:8080")
    /// * `repo_id` - Repository identifier
    ///
    /// # Example
    ///
    /// ```ignore
    /// let backend = RemoteBackend::new("http://localhost:8080", "my-repo");
    /// ```
    pub fn new(server_url: impl Into<String>, repo_id: impl Into<String>) -> Self {
        Self {
            server_url: server_url.into(),
            api_key: None,
            repo_id: repo_id.into(),
            timeout_secs: 30,
        }
    }

    /// Set the API key for authentication.
    pub fn with_api_key(mut self, api_key: impl Into<String>) -> Self {
        self.api_key = Some(api_key.into());
        self
    }

    /// Set the request timeout.
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Get the server URL.
    pub fn server_url(&self) -> &str {
        &self.server_url
    }

    /// Placeholder for making HTTP requests.
    ///
    /// This will be implemented when the HTTP client is added.
    #[allow(dead_code)]
    async fn request<T>(&self, _endpoint: &str, _body: Option<&str>) -> Result<T, BackendError>
    where
        T: Default,
    {
        Err(BackendError::with_context(
            "remote backend",
            "HTTP backend not yet implemented",
        ))
    }
}

#[async_trait]
impl Backend for RemoteBackend {
    async fn search(
        &self,
        _query: &str,
        _limit: usize,
        _options: Option<SearchOptions>,
    ) -> Result<Vec<SearchResult>, BackendError> {
        Err(BackendError::with_context(
            "remote search",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }

    async fn get_node(&self, _node_id: &str) -> Result<NodeInfo, BackendError> {
        Err(BackendError::with_context(
            "remote get_node",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }

    async fn get_connected_nodes(
        &self,
        _node_id: &str,
        _edge_type: Option<&str>,
        _direction: &str,
    ) -> Result<Vec<NodeInfo>, BackendError> {
        Err(BackendError::with_context(
            "remote get_connected_nodes",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }

    async fn get_edges(
        &self,
        _node_id: &str,
        _edge_type: Option<&str>,
        _direction: &str,
    ) -> Result<Vec<EdgeInfo>, BackendError> {
        Err(BackendError::with_context(
            "remote get_edges",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }

    async fn index_status(&self) -> Result<IndexStatus, BackendError> {
        Err(BackendError::with_context(
            "remote index_status",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }

    async fn graph_stats(&self) -> Result<GraphStats, BackendError> {
        Err(BackendError::with_context(
            "remote graph_stats",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }

    async fn read_code(
        &self,
        _node_id: &str,
        _context_lines: usize,
    ) -> Result<String, BackendError> {
        Err(BackendError::with_context(
            "remote read_code",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }

    async fn find_nodes(
        &self,
        _pattern: &str,
        _node_type: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<NodeInfo>, BackendError> {
        Err(BackendError::with_context(
            "remote find_nodes",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }

    async fn index(&self, _force: bool) -> Result<usize, BackendError> {
        Err(BackendError::with_context(
            "remote index",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }

    async fn sync(&self) -> Result<bool, BackendError> {
        Err(BackendError::with_context(
            "remote sync",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }

    fn repo_id(&self) -> &str {
        &self.repo_id
    }

    async fn health_check(&self) -> Result<bool, BackendError> {
        // In the future, this will ping the server
        Err(BackendError::with_context(
            "remote health_check",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }

    async fn check_provider(&self) -> Result<codeprysm_search::ProviderStatus, BackendError> {
        // Remote backend would query server for provider status
        Err(BackendError::with_context(
            "remote check_provider",
            "HTTP backend not yet implemented. Use LocalBackend instead.",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_remote_backend_creation() {
        let backend = RemoteBackend::new("http://localhost:8080", "my-repo")
            .with_api_key("secret-key")
            .with_timeout(60);

        assert_eq!(backend.server_url(), "http://localhost:8080");
        assert_eq!(backend.repo_id(), "my-repo");
        assert_eq!(backend.api_key, Some("secret-key".to_string()));
        assert_eq!(backend.timeout_secs, 60);
    }

    #[tokio::test]
    async fn test_remote_backend_not_implemented() {
        let backend = RemoteBackend::new("http://localhost:8080", "my-repo");

        let result = backend.search("test", 10, None).await;
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }
}
