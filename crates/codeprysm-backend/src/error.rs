//! Backend error types.

use std::path::PathBuf;
use thiserror::Error;

/// Errors that can occur during backend operations.
#[derive(Error, Debug)]
pub enum BackendError {
    /// Search operation failed
    #[error("search failed: {0}")]
    Search(#[from] codeprysm_search::SearchError),

    /// Graph loading failed
    #[error("graph loading failed: {0}")]
    GraphLoad(#[from] codeprysm_core::lazy::LazyGraphError),

    /// Graph builder failed
    #[error("graph builder failed: {0}")]
    GraphBuilder(#[from] codeprysm_core::BuilderError),

    /// Configuration error
    #[error("configuration error: {0}")]
    Config(#[from] codeprysm_config::ConfigError),

    /// I/O error
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Graph file not found
    #[error("graph not found at '{path}'. Run 'codeprysm init' to generate the graph.")]
    GraphNotFound { path: PathBuf },

    /// Index not available
    #[error("search index not available: {message}")]
    IndexNotAvailable { message: String },

    /// Node not found in graph
    #[error("node '{id}' not found in graph")]
    NodeNotFound { id: String },

    /// Remote server error
    #[error("remote server error: {status} - {message}")]
    RemoteServer { status: u16, message: String },

    /// Connection error
    #[error("connection failed: {0}")]
    Connection(String),

    /// Serialization error
    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// Generic error with context
    #[error("{context}: {message}")]
    WithContext { context: String, message: String },
}

impl BackendError {
    /// Create a GraphNotFound error.
    pub fn graph_not_found(path: impl Into<PathBuf>) -> Self {
        Self::GraphNotFound { path: path.into() }
    }

    /// Create an IndexNotAvailable error.
    pub fn index_not_available(message: impl Into<String>) -> Self {
        Self::IndexNotAvailable {
            message: message.into(),
        }
    }

    /// Create a NodeNotFound error.
    pub fn node_not_found(id: impl Into<String>) -> Self {
        Self::NodeNotFound { id: id.into() }
    }

    /// Create a RemoteServer error.
    pub fn remote_server(status: u16, message: impl Into<String>) -> Self {
        Self::RemoteServer {
            status,
            message: message.into(),
        }
    }

    /// Create a Connection error.
    pub fn connection(message: impl Into<String>) -> Self {
        Self::Connection(message.into())
    }

    /// Add context to any error.
    pub fn with_context(context: impl Into<String>, message: impl Into<String>) -> Self {
        Self::WithContext {
            context: context.into(),
            message: message.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display() {
        let err = BackendError::graph_not_found("/path/to/graph");
        assert!(err.to_string().contains("graph not found"));
        assert!(err.to_string().contains("/path/to/graph"));

        let err = BackendError::node_not_found("src/lib.rs:MyStruct");
        assert!(err.to_string().contains("not found"));
        assert!(err.to_string().contains("MyStruct"));
    }

    #[test]
    fn test_remote_server_error() {
        let err = BackendError::remote_server(500, "Internal server error");
        assert!(err.to_string().contains("500"));
        assert!(err.to_string().contains("Internal server error"));
    }

    #[test]
    fn test_with_context() {
        let err = BackendError::with_context("loading graph", "file corrupted");
        assert!(err.to_string().contains("loading graph"));
        assert!(err.to_string().contains("file corrupted"));
    }
}
