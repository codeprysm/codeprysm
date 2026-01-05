//! Error types for the MCP server

use thiserror::Error;

/// Result type for MCP operations
pub type Result<T> = std::result::Result<T, McpError>;

/// Errors that can occur in the MCP server
#[derive(Error, Debug)]
pub enum McpError {
    /// Graph file not found or failed to load
    #[error("Failed to load graph: {0}")]
    GraphLoad(String),

    /// Node not found in graph
    #[error("Node not found: {0}")]
    NodeNotFound(String),

    /// File not found when reading code
    #[error("File not found: {0}")]
    FileNotFound(String),

    /// Search operation failed
    #[error("Search failed: {0}")]
    SearchError(String),

    /// Invalid parameters provided
    #[error("Invalid parameters: {0}")]
    InvalidParams(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON serialization error
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// Internal server error
    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<codeprysm_core::BuilderError> for McpError {
    fn from(e: codeprysm_core::BuilderError) -> Self {
        McpError::GraphLoad(e.to_string())
    }
}

impl From<codeprysm_core::UpdaterError> for McpError {
    fn from(e: codeprysm_core::UpdaterError) -> Self {
        McpError::Internal(e.to_string())
    }
}

impl From<codeprysm_search::SearchError> for McpError {
    fn from(e: codeprysm_search::SearchError) -> Self {
        McpError::SearchError(e.to_string())
    }
}
