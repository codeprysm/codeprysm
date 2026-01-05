//! OpenAI-compatible embedding provider
//!
//! Provides embedding generation via OpenAI-compatible APIs including:
//! - OpenAI API
//! - Azure OpenAI
//! - Ollama
//! - Prism SaaS
//!
//! # Endpoint Format
//!
//! - POST `{base_url}/v1/embeddings`
//! - Request: `{"model": "...", "input": ["text1", "text2", ...]}`
//! - Response: `{"data": [{"embedding": [...], "index": 0}, ...], ...}`
//!
//! # Example
//!
//! ```ignore
//! use codeprysm_search::embeddings::openai::{OpenAIProvider, OpenAIConfig};
//!
//! let config = OpenAIConfig {
//!     base_url: "http://localhost:11434/v1".into(),
//!     api_key: None,  // Ollama doesn't need auth
//!     semantic_model: "nomic-embed-text".into(),
//!     code_model: None,  // Use semantic model for both
//!     timeout_secs: 30,
//!     max_retries: 3,
//!     azure_mode: false,
//! };
//!
//! let provider = OpenAIProvider::new(config)?;
//! let embeddings = provider.encode_semantic(vec!["hello world".into()]).await?;
//! ```

use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use async_trait::async_trait;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};

#[cfg(feature = "rate-limit")]
use governor::{
    clock::DefaultClock,
    state::{InMemoryState, NotKeyed},
    Quota, RateLimiter,
};
#[cfg(feature = "rate-limit")]
use std::num::NonZeroU32;
#[cfg(feature = "rate-limit")]
use std::sync::Arc;

use super::provider::{EmbeddingProvider, EmbeddingProviderType, ProviderStatus};
use crate::error::{Result, SearchError};

/// Default timeout in seconds
const DEFAULT_TIMEOUT_SECS: u64 = 30;

/// Default max retries
const DEFAULT_MAX_RETRIES: u32 = 3;

/// Base delay for exponential backoff (milliseconds)
const RETRY_BASE_DELAY_MS: u64 = 500;

/// Default requests per second limit
#[cfg(feature = "rate-limit")]
const DEFAULT_REQUESTS_PER_SECOND: u32 = 10;

/// Type alias for the rate limiter
#[cfg(feature = "rate-limit")]
type OpenAIRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Configuration for OpenAI-compatible provider
#[derive(Debug, Clone)]
pub struct OpenAIConfig {
    /// Base URL for the API (e.g., "https://api.openai.com/v1" or "http://localhost:11434/v1")
    pub base_url: String,
    /// API key (optional for local providers like Ollama)
    pub api_key: Option<String>,
    /// Model for semantic embeddings (e.g., "text-embedding-3-small", "nomic-embed-text")
    pub semantic_model: String,
    /// Model for code embeddings (None = use semantic model for both)
    pub code_model: Option<String>,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Maximum retry attempts for transient failures
    pub max_retries: u32,
    /// Use Azure OpenAI header format (api-key instead of Bearer)
    pub azure_mode: bool,
    /// Requests per second limit (when rate-limit feature enabled)
    #[cfg(feature = "rate-limit")]
    pub requests_per_second: u32,
}

impl OpenAIConfig {
    /// Create config for Ollama local endpoint
    pub fn ollama() -> Self {
        Self {
            base_url: "http://localhost:11434/v1".into(),
            api_key: None,
            semantic_model: "nomic-embed-text".into(),
            code_model: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_retries: DEFAULT_MAX_RETRIES,
            azure_mode: false,
            #[cfg(feature = "rate-limit")]
            requests_per_second: DEFAULT_REQUESTS_PER_SECOND,
        }
    }

    /// Create config for OpenAI API
    pub fn openai(api_key: impl Into<String>) -> Self {
        Self {
            base_url: "https://api.openai.com/v1".into(),
            api_key: Some(api_key.into()),
            semantic_model: "text-embedding-3-small".into(),
            code_model: None,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_retries: DEFAULT_MAX_RETRIES,
            azure_mode: false,
            #[cfg(feature = "rate-limit")]
            requests_per_second: DEFAULT_REQUESTS_PER_SECOND,
        }
    }

    /// Create config from environment variables
    ///
    /// Expected environment variables:
    /// - `CODEPRYSM_OPENAI_BASE_URL` - API base URL (default: https://api.openai.com/v1)
    /// - `CODEPRYSM_OPENAI_API_KEY` - API key (optional)
    /// - `CODEPRYSM_OPENAI_SEMANTIC_MODEL` - Semantic model name (default: text-embedding-3-small)
    /// - `CODEPRYSM_OPENAI_CODE_MODEL` - Code model name (optional)
    /// - `CODEPRYSM_OPENAI_AZURE_MODE` - Use Azure header format (default: false)
    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("CODEPRYSM_OPENAI_BASE_URL")
            .unwrap_or_else(|_| "https://api.openai.com/v1".into());

        let api_key = std::env::var("CODEPRYSM_OPENAI_API_KEY").ok();

        let semantic_model = std::env::var("CODEPRYSM_OPENAI_SEMANTIC_MODEL")
            .unwrap_or_else(|_| "text-embedding-3-small".into());

        let code_model = std::env::var("CODEPRYSM_OPENAI_CODE_MODEL").ok();

        let azure_mode = std::env::var("CODEPRYSM_OPENAI_AZURE_MODE")
            .map(|v| v.to_lowercase() == "true" || v == "1")
            .unwrap_or(false);

        Ok(Self {
            base_url,
            api_key,
            semantic_model,
            code_model,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_retries: DEFAULT_MAX_RETRIES,
            azure_mode,
            #[cfg(feature = "rate-limit")]
            requests_per_second: DEFAULT_REQUESTS_PER_SECOND,
        })
    }

    /// Set request timeout
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self
    }

    /// Set max retries
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    /// Set requests per second limit (when rate-limit feature enabled)
    #[cfg(feature = "rate-limit")]
    pub fn with_requests_per_second(mut self, rps: u32) -> Self {
        self.requests_per_second = rps;
        self
    }

    /// Get the effective code model (falls back to semantic model)
    pub fn effective_code_model(&self) -> &str {
        self.code_model.as_deref().unwrap_or(&self.semantic_model)
    }
}

/// Request body for OpenAI /v1/embeddings endpoint
#[derive(Debug, Serialize)]
struct EmbeddingsRequest {
    model: String,
    input: Vec<String>,
}

/// Single embedding in OpenAI response
#[derive(Debug, Deserialize)]
struct EmbeddingData {
    embedding: Vec<f32>,
    #[allow(dead_code)]
    index: usize,
}

/// Response from OpenAI /v1/embeddings endpoint
#[derive(Debug, Deserialize)]
struct EmbeddingsResponse {
    data: Vec<EmbeddingData>,
    #[allow(dead_code)]
    model: Option<String>,
}

/// OpenAI-compatible embedding provider
///
/// Connects to OpenAI-compatible APIs including OpenAI, Azure OpenAI, Ollama, and others.
pub struct OpenAIProvider {
    client: Client,
    config: OpenAIConfig,
    /// Cached embedding dimension (detected from first response)
    dimension: AtomicUsize,
    #[cfg(feature = "rate-limit")]
    rate_limiter: Arc<OpenAIRateLimiter>,
}

impl Clone for OpenAIProvider {
    fn clone(&self) -> Self {
        Self {
            client: self.client.clone(),
            config: self.config.clone(),
            dimension: AtomicUsize::new(self.dimension.load(Ordering::Relaxed)),
            #[cfg(feature = "rate-limit")]
            rate_limiter: self.rate_limiter.clone(),
        }
    }
}

impl OpenAIProvider {
    /// Create a new OpenAI-compatible provider
    pub fn new(config: OpenAIConfig) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| SearchError::ProviderUnavailable(format!("HTTP client error: {}", e)))?;

        #[cfg(feature = "rate-limit")]
        let rate_limiter = {
            let rps = NonZeroU32::new(config.requests_per_second)
                .unwrap_or(NonZeroU32::new(DEFAULT_REQUESTS_PER_SECOND).unwrap());
            Arc::new(RateLimiter::direct(Quota::per_second(rps)))
        };

        Ok(Self {
            client,
            config,
            dimension: AtomicUsize::new(0),
            #[cfg(feature = "rate-limit")]
            rate_limiter,
        })
    }

    /// Create provider from environment variables
    pub fn from_env() -> Result<Self> {
        let config = OpenAIConfig::from_env()?;
        Self::new(config)
    }

    /// Wait for rate limiter permission (when feature enabled)
    #[cfg(feature = "rate-limit")]
    async fn wait_for_permit(&self) {
        self.rate_limiter.until_ready().await;
    }

    /// No-op when rate limiting is disabled
    #[cfg(not(feature = "rate-limit"))]
    async fn wait_for_permit(&self) {}

    /// Get the embeddings endpoint URL
    fn embeddings_url(&self) -> String {
        let base = self.config.base_url.trim_end_matches('/');
        // Handle both /v1 and non-/v1 URLs
        if base.ends_with("/v1") {
            format!("{}/embeddings", base)
        } else {
            format!("{}/v1/embeddings", base)
        }
    }

    /// Send request with retry logic
    async fn request_with_retry(&self, model: &str, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let mut last_error = None;
        let mut retry_delay = Duration::from_millis(RETRY_BASE_DELAY_MS);

        for attempt in 0..=self.config.max_retries {
            // Wait for rate limiter before each request attempt
            self.wait_for_permit().await;

            match self.send_request(model, texts.clone()).await {
                Ok(embeddings) => return Ok(embeddings),
                Err(e) => {
                    // Don't retry on auth errors or invalid model
                    if matches!(
                        e,
                        SearchError::OpenAIAuth(_) | SearchError::OpenAIInvalidModel(_)
                    ) {
                        return Err(e);
                    }

                    if attempt < self.config.max_retries {
                        tokio::time::sleep(retry_delay).await;
                        retry_delay *= 2;
                        last_error = Some(e);
                    } else {
                        return Err(e);
                    }
                }
            }
        }

        Err(last_error.unwrap_or_else(|| {
            SearchError::ProviderUnavailable("Request failed after retries".into())
        }))
    }

    /// Send a single request to the endpoint
    async fn send_request(&self, model: &str, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let url = self.embeddings_url();
        let request_body = EmbeddingsRequest {
            model: model.to_string(),
            input: texts,
        };

        let mut request = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request_body);

        // Add authentication header
        if let Some(ref api_key) = self.config.api_key {
            if self.config.azure_mode {
                request = request.header("api-key", api_key);
            } else {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }
        }

        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                SearchError::ProviderUnavailable("Request timed out".into())
            } else if e.is_connect() {
                SearchError::ProviderUnavailable(format!("Connection failed: {}", e))
            } else {
                SearchError::ProviderUnavailable(format!("Request failed: {}", e))
            }
        })?;

        let status = response.status();

        match status {
            StatusCode::OK => {
                let embed_response: EmbeddingsResponse = response.json().await.map_err(|e| {
                    SearchError::ProviderUnavailable(format!("Invalid response: {}", e))
                })?;

                // Extract embeddings and validate/cache dimension
                let embeddings: Vec<Vec<f32>> = embed_response
                    .data
                    .into_iter()
                    .map(|d| d.embedding)
                    .collect();

                if let Some(first) = embeddings.first() {
                    let dim = first.len();
                    let cached = self.dimension.load(Ordering::Relaxed);
                    if cached == 0 {
                        self.dimension.store(dim, Ordering::Relaxed);
                    } else if cached != dim {
                        return Err(SearchError::DimensionMismatch {
                            expected: cached,
                            actual: dim,
                        });
                    }
                }

                Ok(embeddings)
            }
            StatusCode::UNAUTHORIZED => {
                let body = response.text().await.unwrap_or_default();
                Err(SearchError::OpenAIAuth(format!(
                    "Authentication failed: {}",
                    body
                )))
            }
            StatusCode::NOT_FOUND => {
                let body = response.text().await.unwrap_or_default();
                Err(SearchError::OpenAIInvalidModel(format!(
                    "Model not found: {}",
                    body
                )))
            }
            StatusCode::TOO_MANY_REQUESTS => {
                let retry_after = response
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());

                Err(SearchError::OpenAIRateLimit { retry_after })
            }
            StatusCode::SERVICE_UNAVAILABLE | StatusCode::GATEWAY_TIMEOUT => Err(
                SearchError::ProviderUnavailable("Service temporarily unavailable".into()),
            ),
            _ => {
                let body = response.text().await.unwrap_or_default();
                Err(SearchError::ProviderUnavailable(format!(
                    "Request failed with status {}: {}",
                    status, body
                )))
            }
        }
    }

    /// Perform a health check
    async fn health_check(&self) -> Result<Duration> {
        let start = Instant::now();

        // Send a minimal request to check connectivity
        let request_body = EmbeddingsRequest {
            model: self.config.semantic_model.clone(),
            input: vec!["health check".into()],
        };

        let url = self.embeddings_url();
        let mut request = self
            .client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&request_body);

        if let Some(ref api_key) = self.config.api_key {
            if self.config.azure_mode {
                request = request.header("api-key", api_key);
            } else {
                request = request.header("Authorization", format!("Bearer {}", api_key));
            }
        }

        let response = request
            .send()
            .await
            .map_err(|e| SearchError::ProviderUnavailable(format!("Health check failed: {}", e)))?;

        let status = response.status();
        let latency = start.elapsed();

        match status {
            StatusCode::OK => Ok(latency),
            StatusCode::UNAUTHORIZED => Err(SearchError::OpenAIAuth("Invalid API key".into())),
            StatusCode::NOT_FOUND => Err(SearchError::OpenAIInvalidModel(format!(
                "Model '{}' not found",
                self.config.semantic_model
            ))),
            StatusCode::TOO_MANY_REQUESTS => {
                // Even if rate limited, the endpoint is reachable
                Ok(latency)
            }
            _ => Err(SearchError::ProviderUnavailable(format!(
                "Health check failed with status {}",
                status
            ))),
        }
    }
}

#[async_trait]
impl EmbeddingProvider for OpenAIProvider {
    async fn encode_semantic(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        self.request_with_retry(&self.config.semantic_model, texts)
            .await
    }

    async fn encode_code(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        let model = self.config.effective_code_model();
        self.request_with_retry(model, texts).await
    }

    async fn check_status(&self) -> Result<ProviderStatus> {
        match self.health_check().await {
            Ok(latency) => {
                let dim = self.dimension.load(Ordering::Relaxed);
                let status = ProviderStatus::healthy(EmbeddingProviderType::Openai, "Remote")
                    .with_latency(latency.as_millis() as u64);

                // If we have different models for semantic/code, we'd check both
                // For now, assume both use the same endpoint
                if dim > 0 {
                    // Dimension was detected
                }

                Ok(status)
            }
            Err(e) => Ok(ProviderStatus::unavailable(
                EmbeddingProviderType::Openai,
                e.to_string(),
            )),
        }
    }

    async fn warmup(&self) -> Result<()> {
        let status = self.check_status().await?;

        if !status.available {
            return Err(SearchError::ProviderUnavailable(
                status.error.unwrap_or_else(|| "Provider not ready".into()),
            ));
        }

        Ok(())
    }

    fn embedding_dim(&self) -> usize {
        let dim = self.dimension.load(Ordering::Relaxed);
        if dim > 0 {
            dim
        } else {
            // Default dimensions for common models
            // This will be updated once we make an actual request
            match self.config.semantic_model.as_str() {
                "text-embedding-3-small" => 1536,
                "text-embedding-3-large" => 3072,
                "text-embedding-ada-002" => 1536,
                "nomic-embed-text" => 768,
                _ => 768, // Default for Jina/Prism models
            }
        }
    }

    fn provider_type(&self) -> EmbeddingProviderType {
        EmbeddingProviderType::Openai
    }
}

impl std::fmt::Debug for OpenAIProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAIProvider")
            .field("base_url", &self.config.base_url)
            .field("semantic_model", &self.config.semantic_model)
            .field("code_model", &self.config.code_model)
            .field("timeout_secs", &self.config.timeout_secs)
            .field("azure_mode", &self.config.azure_mode)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Create a mock response JSON for embeddings
    fn mock_response(dim: usize, count: usize) -> serde_json::Value {
        let data: Vec<serde_json::Value> = (0..count)
            .map(|i| {
                serde_json::json!({
                    "object": "embedding",
                    "embedding": vec![0.1_f32; dim],
                    "index": i
                })
            })
            .collect();

        serde_json::json!({
            "object": "list",
            "data": data,
            "model": "test-model",
            "usage": {"prompt_tokens": 10, "total_tokens": 10}
        })
    }

    fn test_config(server: &MockServer) -> OpenAIConfig {
        OpenAIConfig {
            base_url: server.uri(),
            api_key: Some("test-key".into()),
            semantic_model: "test-model".into(),
            code_model: None,
            timeout_secs: 5,
            max_retries: 1,
            azure_mode: false,
            #[cfg(feature = "rate-limit")]
            requests_per_second: 100, // High limit for tests
        }
    }

    #[tokio::test]
    async fn test_encode_semantic_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(header("Authorization", "Bearer test-key"))
            .and(body_json(&EmbeddingsRequest {
                model: "test-model".into(),
                input: vec!["hello world".into()],
            }))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(768, 1)))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::new(test_config(&server)).unwrap();
        let result = provider.encode_semantic(vec!["hello world".into()]).await;

        assert!(result.is_ok());
        let embeddings = result.unwrap();
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].len(), 768);
    }

    #[tokio::test]
    async fn test_encode_code_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(768, 1)))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::new(test_config(&server)).unwrap();
        let result = provider.encode_code(vec!["fn main() {}".into()]).await;

        assert!(result.is_ok());
        let embeddings = result.unwrap();
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].len(), 768);
    }

    #[tokio::test]
    async fn test_separate_code_model() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(body_json(&EmbeddingsRequest {
                model: "code-model".into(),
                input: vec!["fn main() {}".into()],
            }))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(768, 1)))
            .mount(&server)
            .await;

        let mut config = test_config(&server);
        config.code_model = Some("code-model".into());

        let provider = OpenAIProvider::new(config).unwrap();
        let result = provider.encode_code(vec!["fn main() {}".into()]).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_empty_input() {
        let server = MockServer::start().await;
        let provider = OpenAIProvider::new(test_config(&server)).unwrap();

        let result = provider.encode_semantic(vec![]).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_auth_failure() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Invalid API key"))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::new(test_config(&server)).unwrap();
        let result = provider.encode_semantic(vec!["test".into()]).await;

        assert!(matches!(result, Err(SearchError::OpenAIAuth(_))));
    }

    #[tokio::test]
    async fn test_model_not_found() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(404).set_body_string("Model not found"))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::new(test_config(&server)).unwrap();
        let result = provider.encode_semantic(vec!["test".into()]).await;

        assert!(matches!(result, Err(SearchError::OpenAIInvalidModel(_))));
    }

    #[tokio::test]
    async fn test_rate_limit() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("Retry-After", "60")
                    .set_body_string("Rate limited"),
            )
            .mount(&server)
            .await;

        let provider = OpenAIProvider::new(test_config(&server)).unwrap();
        let result = provider.encode_semantic(vec!["test".into()]).await;

        match result {
            Err(SearchError::OpenAIRateLimit { retry_after }) => {
                assert_eq!(retry_after, Some(60));
            }
            _ => panic!("Expected OpenAIRateLimit error"),
        }
    }

    #[tokio::test]
    async fn test_azure_mode_header() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(header("api-key", "azure-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(1536, 1)))
            .mount(&server)
            .await;

        let config = OpenAIConfig {
            base_url: server.uri(),
            api_key: Some("azure-key".into()),
            semantic_model: "text-embedding-ada-002".into(),
            code_model: None,
            timeout_secs: 5,
            max_retries: 1,
            azure_mode: true,
            #[cfg(feature = "rate-limit")]
            requests_per_second: 100,
        };

        let provider = OpenAIProvider::new(config).unwrap();
        let result = provider.encode_semantic(vec!["test".into()]).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_no_auth_ollama() {
        let server = MockServer::start().await;

        // Ollama doesn't require auth header
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(768, 1)))
            .mount(&server)
            .await;

        let config = OpenAIConfig {
            base_url: server.uri(),
            api_key: None,
            semantic_model: "nomic-embed-text".into(),
            code_model: None,
            timeout_secs: 5,
            max_retries: 1,
            azure_mode: false,
            #[cfg(feature = "rate-limit")]
            requests_per_second: 100,
        };

        let provider = OpenAIProvider::new(config).unwrap();
        let result = provider.encode_semantic(vec!["test".into()]).await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_provider_type() {
        let server = MockServer::start().await;
        let provider = OpenAIProvider::new(test_config(&server)).unwrap();

        assert_eq!(provider.provider_type(), EmbeddingProviderType::Openai);
    }

    #[tokio::test]
    async fn test_dimension_detection() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(1536, 1)))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::new(test_config(&server)).unwrap();

        // Before request, dimension is estimated
        let initial_dim = provider.embedding_dim();
        assert!(initial_dim > 0);

        // After request, dimension is detected
        let _ = provider.encode_semantic(vec!["test".into()]).await;
        assert_eq!(provider.embedding_dim(), 1536);
    }

    #[tokio::test]
    async fn test_check_status_healthy() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(768, 1)))
            .mount(&server)
            .await;

        let provider = OpenAIProvider::new(test_config(&server)).unwrap();
        let status = provider.check_status().await.unwrap();

        assert!(status.available);
        assert!(status.all_ready());
        assert!(status.latency_ms.is_some());
    }

    #[test]
    fn test_config_ollama() {
        let config = OpenAIConfig::ollama();
        assert_eq!(config.base_url, "http://localhost:11434/v1");
        assert!(config.api_key.is_none());
        assert_eq!(config.semantic_model, "nomic-embed-text");
    }

    #[test]
    fn test_config_openai() {
        let config = OpenAIConfig::openai("sk-test");
        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert_eq!(config.api_key, Some("sk-test".into()));
        assert_eq!(config.semantic_model, "text-embedding-3-small");
    }

    #[test]
    fn test_effective_code_model() {
        let mut config = OpenAIConfig::ollama();
        assert_eq!(config.effective_code_model(), "nomic-embed-text");

        config.code_model = Some("code-specific-model".into());
        assert_eq!(config.effective_code_model(), "code-specific-model");
    }

    #[cfg(feature = "rate-limit")]
    #[test]
    fn test_config_with_rate_limit() {
        let config = OpenAIConfig::ollama().with_requests_per_second(20);
        assert_eq!(config.requests_per_second, 20);
    }

    #[cfg(feature = "rate-limit")]
    #[tokio::test]
    async fn test_rate_limiter_throttles_requests() {
        use std::time::Instant;

        let server = MockServer::start().await;

        // Mock endpoint that returns success quickly
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(768, 1)))
            .expect(4) // Expect 4 requests
            .mount(&server)
            .await;

        // Create provider with 2 requests per second limit
        // Note: governor allows initial burst equal to quota, so first 2 requests are immediate
        let config = OpenAIConfig {
            base_url: server.uri(),
            api_key: Some("test-key".into()),
            semantic_model: "test-model".into(),
            code_model: None,
            timeout_secs: 5,
            max_retries: 0,
            azure_mode: false,
            requests_per_second: 2, // 2 RPS = 500ms between requests after burst
        };

        let provider = OpenAIProvider::new(config).unwrap();

        let start = Instant::now();

        // Make 4 requests:
        // - Requests 1-2: immediate (burst capacity = 2)
        // - Request 3: waits 500ms
        // - Request 4: waits another 500ms
        for _ in 0..4 {
            let _ = provider.encode_semantic(vec!["test".into()]).await.unwrap();
        }

        let elapsed = start.elapsed();

        // With 2 RPS and burst of 2, 4 requests should take at least 1 second
        // (2 waits of 500ms after initial burst)
        assert!(
            elapsed >= Duration::from_millis(900),
            "Rate limiting should throttle requests. Elapsed: {:?}",
            elapsed
        );
    }
}
