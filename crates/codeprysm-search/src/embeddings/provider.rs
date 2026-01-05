//! Embedding provider trait and types
//!
//! Defines the core abstraction for embedding generation with multiple provider implementations:
//! - `LocalProvider` - Candle-based local inference (CPU/Metal/CUDA)
//! - `AzureMLProvider` - Azure ML Online Endpoints
//! - `OpenAIProvider` - OpenAI-compatible APIs (OpenAI, Azure OpenAI, Ollama, Prism SaaS)

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::error::Result;

/// Type of embedding provider
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "kebab-case")]
pub enum EmbeddingProviderType {
    /// Local inference using Candle (CPU/Metal/CUDA)
    #[default]
    Local,
    /// Azure ML Online Endpoints
    AzureMl,
    /// OpenAI-compatible API (OpenAI, Azure OpenAI, Ollama, Prism SaaS)
    Openai,
}

impl std::fmt::Display for EmbeddingProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EmbeddingProviderType::Local => write!(f, "local"),
            EmbeddingProviderType::AzureMl => write!(f, "azure-ml"),
            EmbeddingProviderType::Openai => write!(f, "openai"),
        }
    }
}

/// Status of an embedding provider
///
/// Contains health and capability information for diagnostics.
#[derive(Debug, Clone)]
pub struct ProviderStatus {
    /// Whether the provider is available and responding
    pub available: bool,
    /// Type of provider
    pub provider_type: EmbeddingProviderType,
    /// Device/endpoint being used ("CPU", "Metal", "CUDA", "Remote")
    pub device: String,
    /// Last health check latency in milliseconds
    pub latency_ms: Option<u64>,
    /// Whether semantic embeddings are ready
    pub semantic_ready: bool,
    /// Whether code embeddings are ready
    pub code_ready: bool,
    /// Error message if provider is unavailable
    pub error: Option<String>,
}

impl ProviderStatus {
    /// Create a status for a healthy provider
    pub fn healthy(provider_type: EmbeddingProviderType, device: impl Into<String>) -> Self {
        Self {
            available: true,
            provider_type,
            device: device.into(),
            latency_ms: None,
            semantic_ready: true,
            code_ready: true,
            error: None,
        }
    }

    /// Create a status for an unavailable provider
    pub fn unavailable(provider_type: EmbeddingProviderType, error: impl Into<String>) -> Self {
        Self {
            available: false,
            provider_type,
            device: "N/A".into(),
            latency_ms: None,
            semantic_ready: false,
            code_ready: false,
            error: Some(error.into()),
        }
    }

    /// Set latency from a health check
    pub fn with_latency(mut self, latency_ms: u64) -> Self {
        self.latency_ms = Some(latency_ms);
        self
    }

    /// Check if all capabilities are ready
    pub fn all_ready(&self) -> bool {
        self.available && self.semantic_ready && self.code_ready
    }
}

/// Embedding provider trait
///
/// Core abstraction for generating embeddings from text. Implementations may use:
/// - Local inference (Candle + Jina models)
/// - Remote APIs (Azure ML, OpenAI-compatible)
///
/// All methods are async to support both local (spawn_blocking) and remote (HTTP) providers.
///
/// # Thread Safety
///
/// Implementations must be `Send + Sync` for use with async runtimes and concurrent access.
///
/// # Example
///
/// ```ignore
/// use codeprysm_search::embeddings::{EmbeddingProvider, ProviderStatus};
///
/// async fn example(provider: &dyn EmbeddingProvider) -> Result<(), codeprysm_search::SearchError> {
///     // Check provider health
///     let status = provider.check_status().await?;
///     if !status.available {
///         return Err(codeprysm_search::SearchError::ProviderUnavailable(
///             status.error.unwrap_or_default()
///         ));
///     }
///
///     // Generate embeddings
///     let texts = vec!["hello world".to_string()];
///     let embeddings = provider.encode_semantic(texts).await?;
///     assert_eq!(embeddings[0].len(), provider.embedding_dim());
///
///     Ok(())
/// }
/// ```
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Generate semantic embeddings for natural language text
    ///
    /// Uses a model optimized for general text similarity (e.g., jina-embeddings-v2-base-en).
    /// Best for queries like "authentication logic" or "error handling".
    ///
    /// # Arguments
    /// * `texts` - Vector of text strings to embed (owned for async compatibility)
    ///
    /// # Returns
    /// Vector of embeddings, each with `embedding_dim()` dimensions
    async fn encode_semantic(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;

    /// Generate code embeddings optimized for source code
    ///
    /// Uses a model optimized for code understanding (e.g., jina-embeddings-v2-base-code).
    /// Best for code snippets and code-aware search.
    ///
    /// # Arguments
    /// * `texts` - Vector of code strings to embed (owned for async compatibility)
    ///
    /// # Returns
    /// Vector of embeddings, each with `embedding_dim()` dimensions
    async fn encode_code(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>>;

    /// Check provider connectivity and status
    ///
    /// For local providers, checks model availability.
    /// For remote providers, performs a health check request.
    async fn check_status(&self) -> Result<ProviderStatus>;

    /// Warm up the provider
    ///
    /// For local providers, preloads models into memory.
    /// For remote providers, establishes connections and measures latency.
    async fn warmup(&self) -> Result<()>;

    /// Get the embedding dimension
    ///
    /// Returns the dimensionality of generated embeddings (e.g., 768 for Jina, 1536 for ada-002).
    fn embedding_dim(&self) -> usize;

    /// Get the provider type identifier
    fn provider_type(&self) -> EmbeddingProviderType;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_type_display() {
        assert_eq!(EmbeddingProviderType::Local.to_string(), "local");
        assert_eq!(EmbeddingProviderType::AzureMl.to_string(), "azure-ml");
        assert_eq!(EmbeddingProviderType::Openai.to_string(), "openai");
    }

    #[test]
    fn test_provider_type_default() {
        assert_eq!(
            EmbeddingProviderType::default(),
            EmbeddingProviderType::Local
        );
    }

    #[test]
    fn test_provider_status_healthy() {
        let status = ProviderStatus::healthy(EmbeddingProviderType::Local, "Metal");
        assert!(status.available);
        assert!(status.semantic_ready);
        assert!(status.code_ready);
        assert!(status.all_ready());
        assert_eq!(status.device, "Metal");
        assert!(status.error.is_none());
    }

    #[test]
    fn test_provider_status_unavailable() {
        let status =
            ProviderStatus::unavailable(EmbeddingProviderType::AzureMl, "Connection timeout");
        assert!(!status.available);
        assert!(!status.all_ready());
        assert_eq!(status.error, Some("Connection timeout".to_string()));
    }

    #[test]
    fn test_provider_status_with_latency() {
        let status =
            ProviderStatus::healthy(EmbeddingProviderType::Openai, "Remote").with_latency(150);
        assert_eq!(status.latency_ms, Some(150));
    }
}
