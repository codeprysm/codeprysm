//! Azure ML Online Endpoint embedding provider
//!
//! Provides embedding generation via Azure ML-hosted Jina models.
//!
//! # Endpoint Format
//!
//! - POST `https://<endpoint>.inference.ml.azure.com/score`
//! - Request: `{"inputs": ["text1", "text2", ...]}`
//! - Response: `{"embeddings": [[...], [...]], "dimension": 768, "model": "...", "count": N}`
//!
//! # Authentication
//!
//! Supports API key authentication with Bearer token.
//!
//! # Example
//!
//! ```ignore
//! use codeprysm_search::embeddings::azure_ml::{AzureMLProvider, AzureMLConfig, AzureMLAuth};
//!
//! let config = AzureMLConfig {
//!     semantic_endpoint: "https://jina-semantic.eastus2.inference.ml.azure.com/score".into(),
//!     code_endpoint: "https://jina-code.eastus2.inference.ml.azure.com/score".into(),
//!     semantic_auth: AzureMLAuth::ApiKey("semantic-api-key".into()),
//!     code_auth: Some(AzureMLAuth::ApiKey("code-api-key".into())), // Optional, different key for code
//!     timeout_secs: 30,
//!     max_retries: 3,
//! };
//!
//! let provider = AzureMLProvider::new(config)?;
//! let embeddings = provider.encode_semantic(vec!["hello world".into()]).await?;
//! ```

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

/// Embedding dimension for Azure ML Jina models
pub const AZURE_ML_DIM: usize = 768;

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
type AzureRateLimiter = RateLimiter<NotKeyed, InMemoryState, DefaultClock>;

/// Authentication method for Azure ML endpoints
#[derive(Debug, Clone)]
pub enum AzureMLAuth {
    /// Direct API key
    ApiKey(String),
    /// Read API key from environment variable
    ApiKeyEnv(String),
    // Future: ManagedIdentity, AadToken
}

impl AzureMLAuth {
    /// Resolve the API key from the auth method
    fn resolve_key(&self) -> Result<String> {
        match self {
            AzureMLAuth::ApiKey(key) => Ok(key.clone()),
            AzureMLAuth::ApiKeyEnv(var_name) => std::env::var(var_name).map_err(|_| {
                SearchError::AzureMLAuth(format!("Environment variable '{}' not set", var_name))
            }),
        }
    }
}

/// Configuration for Azure ML provider
#[derive(Debug, Clone)]
pub struct AzureMLConfig {
    /// Endpoint URL for semantic embeddings (jina-embeddings-v2-base-en)
    pub semantic_endpoint: String,
    /// Endpoint URL for code embeddings (jina-embeddings-v2-base-code)
    pub code_endpoint: String,
    /// Authentication method for semantic endpoint
    pub semantic_auth: AzureMLAuth,
    /// Authentication method for code endpoint (None = use semantic_auth)
    pub code_auth: Option<AzureMLAuth>,
    /// Request timeout in seconds
    pub timeout_secs: u64,
    /// Maximum retry attempts for transient failures
    pub max_retries: u32,
    /// Requests per second limit (when rate-limit feature enabled)
    #[cfg(feature = "rate-limit")]
    pub requests_per_second: u32,
}

impl AzureMLConfig {
    /// Create config from environment variables
    ///
    /// Expected environment variables:
    /// - `CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT` - Semantic endpoint URL
    /// - `CODEPRYSM_AZURE_ML_CODE_ENDPOINT` - Code endpoint URL
    /// - `CODEPRYSM_AZURE_ML_SEMANTIC_API_KEY` - API key for semantic endpoint
    /// - `CODEPRYSM_AZURE_ML_CODE_API_KEY` - API key for code endpoint (optional, uses semantic key if not set)
    ///
    /// Legacy support (single key for both endpoints):
    /// - `CODEPRYSM_AZURE_ML_API_KEY` - API key for both endpoints
    pub fn from_env() -> Result<Self> {
        let semantic_endpoint =
            std::env::var("CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT").map_err(|_| {
                SearchError::AzureMLAuth(
                    "CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT environment variable not set".into(),
                )
            })?;

        let code_endpoint = std::env::var("CODEPRYSM_AZURE_ML_CODE_ENDPOINT").map_err(|_| {
            SearchError::AzureMLAuth(
                "CODEPRYSM_AZURE_ML_CODE_ENDPOINT environment variable not set".into(),
            )
        })?;

        // Try endpoint-specific keys first, then fall back to shared key
        let semantic_auth = if let Ok(key) = std::env::var("CODEPRYSM_AZURE_ML_SEMANTIC_API_KEY") {
            AzureMLAuth::ApiKey(key)
        } else if let Ok(key) = std::env::var("CODEPRYSM_AZURE_ML_API_KEY") {
            AzureMLAuth::ApiKey(key)
        } else {
            return Err(SearchError::AzureMLAuth(
                "Neither CODEPRYSM_AZURE_ML_SEMANTIC_API_KEY nor CODEPRYSM_AZURE_ML_API_KEY is set"
                    .into(),
            ));
        };

        // Code auth: try code-specific key, fall back to None (will use semantic_auth)
        let code_auth = std::env::var("CODEPRYSM_AZURE_ML_CODE_API_KEY")
            .ok()
            .map(AzureMLAuth::ApiKey);

        Ok(Self {
            semantic_endpoint,
            code_endpoint,
            semantic_auth,
            code_auth,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_retries: DEFAULT_MAX_RETRIES,
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
}

/// Request body for Azure ML /score endpoint
#[derive(Debug, Serialize)]
struct ScoreRequest {
    inputs: Vec<String>,
}

/// Response from Azure ML /score endpoint
#[derive(Debug, Deserialize)]
struct ScoreResponse {
    embeddings: Vec<Vec<f32>>,
    dimension: usize,
    #[allow(dead_code)]
    model: Option<String>,
    #[allow(dead_code)]
    count: Option<usize>,
}

/// Azure ML embedding provider
///
/// Connects to Azure ML Online Endpoints hosting Jina embedding models.
#[derive(Clone)]
pub struct AzureMLProvider {
    client: Client,
    config: AzureMLConfig,
    semantic_api_key: String,
    code_api_key: String,
    #[cfg(feature = "rate-limit")]
    rate_limiter: Arc<AzureRateLimiter>,
}

impl AzureMLProvider {
    /// Create a new Azure ML provider
    ///
    /// # Arguments
    /// * `config` - Provider configuration
    ///
    /// # Returns
    /// * `Ok(AzureMLProvider)` - Configured provider
    /// * `Err(SearchError)` - If API key resolution fails
    pub fn new(config: AzureMLConfig) -> Result<Self> {
        let semantic_api_key = config.semantic_auth.resolve_key()?;
        let code_api_key = config
            .code_auth
            .as_ref()
            .map(|a| a.resolve_key())
            .transpose()?
            .unwrap_or_else(|| semantic_api_key.clone());

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
            semantic_api_key,
            code_api_key,
            #[cfg(feature = "rate-limit")]
            rate_limiter,
        })
    }

    /// Create provider from environment variables
    pub fn from_env() -> Result<Self> {
        let config = AzureMLConfig::from_env()?;
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

    /// Send request to an endpoint with retry logic
    async fn request_with_retry(
        &self,
        endpoint: &str,
        api_key: &str,
        texts: Vec<String>,
    ) -> Result<Vec<Vec<f32>>> {
        let mut last_error = None;
        let mut retry_delay = Duration::from_millis(RETRY_BASE_DELAY_MS);

        for attempt in 0..=self.config.max_retries {
            // Wait for rate limiter before each request attempt
            self.wait_for_permit().await;

            match self.send_request(endpoint, api_key, texts.clone()).await {
                Ok(embeddings) => return Ok(embeddings),
                Err(e) => {
                    // Don't retry on auth errors or client errors
                    if matches!(e, SearchError::AzureMLAuth(_)) {
                        return Err(e);
                    }

                    // Check if we should retry
                    if attempt < self.config.max_retries {
                        // Exponential backoff
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
    async fn send_request(
        &self,
        endpoint: &str,
        api_key: &str,
        texts: Vec<String>,
    ) -> Result<Vec<Vec<f32>>> {
        let request_body = ScoreRequest { inputs: texts };

        let response = self
            .client
            .post(endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    SearchError::AzureMLTimeout
                } else if e.is_connect() {
                    SearchError::ProviderUnavailable(format!("Connection failed: {}", e))
                } else {
                    SearchError::ProviderUnavailable(format!("Request failed: {}", e))
                }
            })?;

        let status = response.status();

        match status {
            StatusCode::OK => {
                let score_response: ScoreResponse = response.json().await.map_err(|e| {
                    SearchError::ProviderUnavailable(format!("Invalid response: {}", e))
                })?;

                // Validate dimension
                if score_response.dimension != AZURE_ML_DIM {
                    return Err(SearchError::DimensionMismatch {
                        expected: AZURE_ML_DIM,
                        actual: score_response.dimension,
                    });
                }

                Ok(score_response.embeddings)
            }
            StatusCode::UNAUTHORIZED => {
                let body = response.text().await.unwrap_or_default();
                Err(SearchError::AzureMLAuth(format!(
                    "Authentication failed: {}",
                    body
                )))
            }
            StatusCode::TOO_MANY_REQUESTS => {
                // Try to parse Retry-After header
                let retry_after = response
                    .headers()
                    .get("Retry-After")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|s| s.parse::<u64>().ok());

                Err(SearchError::AzureMLRateLimit { retry_after })
            }
            StatusCode::REQUEST_TIMEOUT | StatusCode::GATEWAY_TIMEOUT => {
                Err(SearchError::AzureMLTimeout)
            }
            StatusCode::SERVICE_UNAVAILABLE => Err(SearchError::ProviderUnavailable(
                "Service temporarily unavailable".into(),
            )),
            _ => {
                let body = response.text().await.unwrap_or_default();
                Err(SearchError::ProviderUnavailable(format!(
                    "Request failed with status {}: {}",
                    status, body
                )))
            }
        }
    }

    /// Perform a health check on an endpoint
    async fn health_check(&self, endpoint: &str, api_key: &str) -> Result<Duration> {
        let start = Instant::now();

        // Send a minimal request to check connectivity
        let request_body = ScoreRequest {
            inputs: vec!["health check".into()],
        };

        let response = self
            .client
            .post(endpoint)
            .header("Authorization", format!("Bearer {}", api_key))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() {
                    SearchError::AzureMLTimeout
                } else {
                    SearchError::ProviderUnavailable(format!("Health check failed: {}", e))
                }
            })?;

        let status = response.status();
        let latency = start.elapsed();

        match status {
            StatusCode::OK => Ok(latency),
            StatusCode::UNAUTHORIZED => Err(SearchError::AzureMLAuth("Invalid API key".into())),
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
impl EmbeddingProvider for AzureMLProvider {
    async fn encode_semantic(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        self.request_with_retry(
            &self.config.semantic_endpoint,
            &self.semantic_api_key,
            texts,
        )
        .await
    }

    async fn encode_code(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }
        self.request_with_retry(&self.config.code_endpoint, &self.code_api_key, texts)
            .await
    }

    async fn check_status(&self) -> Result<ProviderStatus> {
        // Check both endpoints
        let semantic_result = self
            .health_check(&self.config.semantic_endpoint, &self.semantic_api_key)
            .await;
        let code_result = self
            .health_check(&self.config.code_endpoint, &self.code_api_key)
            .await;

        match (&semantic_result, &code_result) {
            (Ok(semantic_latency), Ok(code_latency)) => {
                // Use the max latency for the overall status
                let max_latency = (*semantic_latency).max(*code_latency);
                Ok(
                    ProviderStatus::healthy(EmbeddingProviderType::AzureMl, "Remote")
                        .with_latency(max_latency.as_millis() as u64),
                )
            }
            (Ok(latency), Err(e)) => {
                let mut status = ProviderStatus::healthy(EmbeddingProviderType::AzureMl, "Remote")
                    .with_latency(latency.as_millis() as u64);
                status.code_ready = false;
                status.error = Some(format!("Code endpoint: {}", e));
                Ok(status)
            }
            (Err(e), Ok(latency)) => {
                let mut status = ProviderStatus::healthy(EmbeddingProviderType::AzureMl, "Remote")
                    .with_latency(latency.as_millis() as u64);
                status.semantic_ready = false;
                status.error = Some(format!("Semantic endpoint: {}", e));
                Ok(status)
            }
            (Err(e1), Err(e2)) => Ok(ProviderStatus::unavailable(
                EmbeddingProviderType::AzureMl,
                format!("Semantic: {}, Code: {}", e1, e2),
            )),
        }
    }

    async fn warmup(&self) -> Result<()> {
        // Perform health checks to establish connections and measure latency
        let status = self.check_status().await?;

        if !status.all_ready() {
            return Err(SearchError::ProviderUnavailable(
                status.error.unwrap_or_else(|| "Provider not ready".into()),
            ));
        }

        Ok(())
    }

    fn embedding_dim(&self) -> usize {
        AZURE_ML_DIM
    }

    fn provider_type(&self) -> EmbeddingProviderType {
        EmbeddingProviderType::AzureMl
    }
}

impl std::fmt::Debug for AzureMLProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AzureMLProvider")
            .field("semantic_endpoint", &self.config.semantic_endpoint)
            .field("code_endpoint", &self.config.code_endpoint)
            .field("timeout_secs", &self.config.timeout_secs)
            .field("max_retries", &self.config.max_retries)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    /// Create a test embedding vector of the given size
    fn make_embedding(dim: usize, value: f32) -> Vec<f32> {
        vec![value; dim]
    }

    /// Create a mock response JSON for embeddings
    fn mock_response(dim: usize, count: usize, model: &str) -> serde_json::Value {
        let embeddings: Vec<Vec<f32>> = (0..count)
            .map(|i| make_embedding(dim, 0.1 + (i as f32 * 0.1)))
            .collect();
        serde_json::json!({
            "embeddings": embeddings,
            "dimension": dim,
            "model": model,
            "count": count
        })
    }

    fn test_config(server: &MockServer) -> AzureMLConfig {
        AzureMLConfig {
            semantic_endpoint: format!("{}/score/semantic", server.uri()),
            code_endpoint: format!("{}/score/code", server.uri()),
            semantic_auth: AzureMLAuth::ApiKey("test-key".into()),
            code_auth: None, // Uses semantic_auth
            timeout_secs: 5,
            max_retries: 1,
            #[cfg(feature = "rate-limit")]
            requests_per_second: 100, // High limit for tests
        }
    }

    #[tokio::test]
    async fn test_encode_semantic_success() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/score/semantic"))
            .and(header("Authorization", "Bearer test-key"))
            .and(body_json(&ScoreRequest {
                inputs: vec!["hello world".into()],
            }))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(
                768,
                1,
                "jina-embeddings-v2-base-en",
            )))
            .mount(&server)
            .await;

        let provider = AzureMLProvider::new(test_config(&server)).unwrap();
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
            .and(path("/score/code"))
            .and(header("Authorization", "Bearer test-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(
                768,
                1,
                "jina-embeddings-v2-base-code",
            )))
            .mount(&server)
            .await;

        let provider = AzureMLProvider::new(test_config(&server)).unwrap();
        let result = provider.encode_code(vec!["fn main() {}".into()]).await;

        assert!(result.is_ok());
        let embeddings = result.unwrap();
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].len(), 768);
    }

    #[tokio::test]
    async fn test_empty_input() {
        let server = MockServer::start().await;
        let provider = AzureMLProvider::new(test_config(&server)).unwrap();

        let result = provider.encode_semantic(vec![]).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_auth_failure() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/score/semantic"))
            .respond_with(ResponseTemplate::new(401).set_body_string("Invalid API key"))
            .mount(&server)
            .await;

        let provider = AzureMLProvider::new(test_config(&server)).unwrap();
        let result = provider.encode_semantic(vec!["test".into()]).await;

        assert!(matches!(result, Err(SearchError::AzureMLAuth(_))));
    }

    #[tokio::test]
    async fn test_rate_limit() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/score/semantic"))
            .respond_with(
                ResponseTemplate::new(429)
                    .insert_header("Retry-After", "60")
                    .set_body_string("Rate limited"),
            )
            .mount(&server)
            .await;

        let provider = AzureMLProvider::new(test_config(&server)).unwrap();
        let result = provider.encode_semantic(vec!["test".into()]).await;

        match result {
            Err(SearchError::AzureMLRateLimit { retry_after }) => {
                assert_eq!(retry_after, Some(60));
            }
            _ => panic!("Expected AzureMLRateLimit error"),
        }
    }

    #[tokio::test]
    async fn test_dimension_mismatch() {
        let server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/score/semantic"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(
                512,
                1,
                "wrong-model",
            )))
            .mount(&server)
            .await;

        let provider = AzureMLProvider::new(test_config(&server)).unwrap();
        let result = provider.encode_semantic(vec!["test".into()]).await;

        match result {
            Err(SearchError::DimensionMismatch { expected, actual }) => {
                assert_eq!(expected, 768);
                assert_eq!(actual, 512);
            }
            _ => panic!("Expected DimensionMismatch error"),
        }
    }

    #[tokio::test]
    async fn test_provider_type() {
        let server = MockServer::start().await;
        let provider = AzureMLProvider::new(test_config(&server)).unwrap();

        assert_eq!(provider.provider_type(), EmbeddingProviderType::AzureMl);
        assert_eq!(provider.embedding_dim(), 768);
    }

    #[tokio::test]
    async fn test_check_status_healthy() {
        let server = MockServer::start().await;

        // Mock both endpoints
        Mock::given(method("POST"))
            .and(path("/score/semantic"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(
                768,
                1,
                "jina-embeddings-v2-base-en",
            )))
            .mount(&server)
            .await;

        Mock::given(method("POST"))
            .and(path("/score/code"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(
                768,
                1,
                "jina-embeddings-v2-base-code",
            )))
            .mount(&server)
            .await;

        let provider = AzureMLProvider::new(test_config(&server)).unwrap();
        let status = provider.check_status().await.unwrap();

        assert!(status.available);
        assert!(status.semantic_ready);
        assert!(status.code_ready);
        assert!(status.all_ready());
        assert!(status.latency_ms.is_some());
        assert_eq!(status.device, "Remote");
    }

    #[tokio::test]
    async fn test_check_status_partial_failure() {
        let server = MockServer::start().await;

        // Semantic endpoint works
        Mock::given(method("POST"))
            .and(path("/score/semantic"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(
                768,
                1,
                "jina-embeddings-v2-base-en",
            )))
            .mount(&server)
            .await;

        // Code endpoint fails
        Mock::given(method("POST"))
            .and(path("/score/code"))
            .respond_with(ResponseTemplate::new(503).set_body_string("Service unavailable"))
            .mount(&server)
            .await;

        let provider = AzureMLProvider::new(test_config(&server)).unwrap();
        let status = provider.check_status().await.unwrap();

        assert!(status.available);
        assert!(status.semantic_ready);
        assert!(!status.code_ready);
        assert!(!status.all_ready());
        assert!(status.error.is_some());
    }

    #[test]
    fn test_auth_resolve_key() {
        let auth = AzureMLAuth::ApiKey("my-key".into());
        assert_eq!(auth.resolve_key().unwrap(), "my-key");

        // SAFETY: This is a single-threaded test, environment manipulation is safe
        unsafe {
            std::env::set_var("TEST_API_KEY_VAR", "env-key");
        }
        let auth_env = AzureMLAuth::ApiKeyEnv("TEST_API_KEY_VAR".into());
        assert_eq!(auth_env.resolve_key().unwrap(), "env-key");
        // SAFETY: This is a single-threaded test, environment manipulation is safe
        unsafe {
            std::env::remove_var("TEST_API_KEY_VAR");
        }

        let auth_missing = AzureMLAuth::ApiKeyEnv("NONEXISTENT_VAR".into());
        assert!(auth_missing.resolve_key().is_err());
    }

    #[test]
    fn test_config_builder() {
        let config = AzureMLConfig {
            semantic_endpoint: "https://semantic.example.com".into(),
            code_endpoint: "https://code.example.com".into(),
            semantic_auth: AzureMLAuth::ApiKey("key".into()),
            code_auth: None,
            timeout_secs: 30,
            max_retries: 3,
            #[cfg(feature = "rate-limit")]
            requests_per_second: 10,
        }
        .with_timeout(60)
        .with_max_retries(5);

        assert_eq!(config.timeout_secs, 60);
        assert_eq!(config.max_retries, 5);
    }

    #[test]
    fn test_separate_code_auth() {
        let config = AzureMLConfig {
            semantic_endpoint: "https://semantic.example.com".into(),
            code_endpoint: "https://code.example.com".into(),
            semantic_auth: AzureMLAuth::ApiKey("semantic-key".into()),
            code_auth: Some(AzureMLAuth::ApiKey("code-key".into())),
            timeout_secs: 30,
            max_retries: 3,
            #[cfg(feature = "rate-limit")]
            requests_per_second: 10,
        };

        // Provider should be created successfully with separate keys
        let provider = AzureMLProvider::new(config).unwrap();
        assert_eq!(provider.semantic_api_key, "semantic-key");
        assert_eq!(provider.code_api_key, "code-key");
    }

    #[test]
    fn test_shared_auth() {
        let config = AzureMLConfig {
            semantic_endpoint: "https://semantic.example.com".into(),
            code_endpoint: "https://code.example.com".into(),
            semantic_auth: AzureMLAuth::ApiKey("shared-key".into()),
            code_auth: None, // Falls back to semantic_auth
            timeout_secs: 30,
            max_retries: 3,
            #[cfg(feature = "rate-limit")]
            requests_per_second: 10,
        };

        let provider = AzureMLProvider::new(config).unwrap();
        assert_eq!(provider.semantic_api_key, "shared-key");
        assert_eq!(provider.code_api_key, "shared-key");
    }

    #[cfg(feature = "rate-limit")]
    #[test]
    fn test_config_with_rate_limit() {
        let config = AzureMLConfig {
            semantic_endpoint: "https://semantic.example.com".into(),
            code_endpoint: "https://code.example.com".into(),
            semantic_auth: AzureMLAuth::ApiKey("key".into()),
            code_auth: None,
            timeout_secs: 30,
            max_retries: 3,
            requests_per_second: 5,
        }
        .with_requests_per_second(20);

        assert_eq!(config.requests_per_second, 20);
    }

    #[cfg(feature = "rate-limit")]
    #[tokio::test]
    async fn test_rate_limiter_throttles_requests() {
        use std::time::Instant;

        let server = MockServer::start().await;

        // Mock endpoint that returns success quickly
        Mock::given(method("POST"))
            .and(path("/score/semantic"))
            .respond_with(ResponseTemplate::new(200).set_body_json(mock_response(
                768,
                1,
                "jina-embeddings-v2-base-en",
            )))
            .expect(4) // Expect 4 requests
            .mount(&server)
            .await;

        // Create provider with 2 requests per second limit
        // Note: governor allows initial burst equal to quota, so first 2 requests are immediate
        let config = AzureMLConfig {
            semantic_endpoint: format!("{}/score/semantic", server.uri()),
            code_endpoint: format!("{}/score/code", server.uri()),
            semantic_auth: AzureMLAuth::ApiKey("test-key".into()),
            code_auth: None,
            timeout_secs: 5,
            max_retries: 0,
            requests_per_second: 2, // 2 RPS = 500ms between requests after burst
        };

        let provider = AzureMLProvider::new(config).unwrap();

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
