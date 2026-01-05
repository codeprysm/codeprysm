//! Error types for codeprysm-search

use thiserror::Error;

/// Errors that can occur in codeprysm-search operations
#[derive(Error, Debug)]
pub enum SearchError {
    /// Qdrant client error
    #[error("Qdrant error: {0}")]
    Qdrant(String),

    /// Collection not found
    #[error("Collection not found: {0}")]
    CollectionNotFound(String),

    /// Collection already exists
    #[error("Collection already exists: {0}")]
    CollectionExists(String),

    /// Invalid configuration
    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),

    /// Connection error
    #[error("Connection failed: {0}")]
    Connection(String),

    /// Embedding error
    #[error("Embedding error: {0}")]
    Embedding(String),

    // =========================================================================
    // Provider errors
    // =========================================================================
    /// Embedding provider unavailable
    #[error("Embedding provider unavailable: {0}")]
    ProviderUnavailable(String),

    /// Embedding dimension mismatch
    #[error("Embedding dimension mismatch: expected {expected}, got {actual}")]
    DimensionMismatch { expected: usize, actual: usize },

    /// Azure ML authentication failed
    #[error("Azure ML authentication failed: {0}")]
    AzureMLAuth(String),

    /// Azure ML rate limited
    #[error("Azure ML rate limited, retry after {retry_after:?} seconds")]
    AzureMLRateLimit { retry_after: Option<u64> },

    /// Azure ML request timed out
    #[error("Azure ML request timed out")]
    AzureMLTimeout,

    /// OpenAI authentication failed
    #[error("OpenAI authentication failed: {0}")]
    OpenAIAuth(String),

    /// OpenAI rate limited
    #[error("OpenAI rate limited, retry after {retry_after:?} seconds")]
    OpenAIRateLimit { retry_after: Option<u64> },

    /// OpenAI model not found
    #[error("OpenAI model not found: {0}")]
    OpenAIInvalidModel(String),

    /// Serialization error
    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

impl From<qdrant_client::QdrantError> for SearchError {
    fn from(err: qdrant_client::QdrantError) -> Self {
        SearchError::Qdrant(err.to_string())
    }
}

impl From<candle_core::Error> for SearchError {
    fn from(err: candle_core::Error) -> Self {
        SearchError::Embedding(err.to_string())
    }
}

/// Result type for codeprysm-search operations
pub type Result<T> = std::result::Result<T, SearchError>;
