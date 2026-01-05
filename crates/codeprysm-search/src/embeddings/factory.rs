//! Provider factory for creating embedding providers from configuration
//!
//! Creates the appropriate provider implementation based on configuration.
//! Supports Local, Azure ML, and OpenAI-compatible providers.

use std::sync::Arc;

use crate::error::{Result, SearchError};
use crate::schema::CollectionConfig;

use super::azure_ml::{AzureMLConfig, AzureMLProvider};
use super::local::LocalProvider;
use super::openai::{OpenAIConfig, OpenAIProvider};
use super::provider::{EmbeddingProvider, EmbeddingProviderType};

/// Expected embedding dimension for Prism collections
pub const EXPECTED_DIM: usize = CollectionConfig::SEMANTIC.dimension as usize;

/// Configuration for embedding providers
///
/// Specifies which provider to use and optionally provides provider-specific
/// configuration. If provider-specific config is not provided, the factory
/// will attempt to read configuration from environment variables.
#[derive(Debug, Clone, Default)]
pub struct EmbeddingConfig {
    /// Which provider to use
    pub provider: EmbeddingProviderType,
    /// Azure ML provider settings (used when provider = AzureMl)
    pub azure_ml: Option<AzureMLConfig>,
    /// OpenAI provider settings (used when provider = Openai)
    pub openai: Option<OpenAIConfig>,
}

impl EmbeddingConfig {
    /// Create config for local provider (default)
    pub fn local() -> Self {
        Self {
            provider: EmbeddingProviderType::Local,
            azure_ml: None,
            openai: None,
        }
    }

    /// Create config for Azure ML provider with explicit config
    pub fn azure_ml_with_config(config: AzureMLConfig) -> Self {
        Self {
            provider: EmbeddingProviderType::AzureMl,
            azure_ml: Some(config),
            openai: None,
        }
    }

    /// Create config for Azure ML provider (reads from environment)
    pub fn azure_ml() -> Self {
        Self {
            provider: EmbeddingProviderType::AzureMl,
            azure_ml: None,
            openai: None,
        }
    }

    /// Create config for OpenAI provider with explicit config
    pub fn openai_with_config(config: OpenAIConfig) -> Self {
        Self {
            provider: EmbeddingProviderType::Openai,
            azure_ml: None,
            openai: Some(config),
        }
    }

    /// Create config for OpenAI-compatible provider (reads from environment)
    pub fn openai() -> Self {
        Self {
            provider: EmbeddingProviderType::Openai,
            azure_ml: None,
            openai: None,
        }
    }
}

/// Validate that a provider's embedding dimension matches the expected dimension
///
/// Returns an error if the dimensions don't match. This is important because
/// Qdrant collections are created with a specific vector dimension, and attempting
/// to insert vectors with different dimensions will fail.
///
/// # Arguments
/// * `provider` - The provider to validate
///
/// # Returns
/// * `Ok(())` - If dimensions match
/// * `Err(SearchError::DimensionMismatch)` - If dimensions don't match
pub fn validate_dimension(provider: &dyn EmbeddingProvider) -> Result<()> {
    let actual = provider.embedding_dim();
    if actual != EXPECTED_DIM {
        return Err(SearchError::DimensionMismatch {
            expected: EXPECTED_DIM,
            actual,
        });
    }
    Ok(())
}

/// Create an embedding provider from configuration
///
/// Returns an `Arc<dyn EmbeddingProvider>` that can be shared across
/// async tasks and threads.
///
/// # Arguments
/// * `config` - Provider configuration specifying type and settings
///
/// # Returns
/// * `Ok(Arc<dyn EmbeddingProvider>)` - The configured provider
/// * `Err(SearchError)` - If provider creation fails
///
/// # Example
///
/// ```ignore
/// use codeprysm_search::embeddings::factory::{create, EmbeddingConfig};
///
/// let config = EmbeddingConfig::local();
/// let provider = create(&config)?;
/// println!("Using {} provider", provider.provider_type());
/// ```
pub fn create(config: &EmbeddingConfig) -> Result<Arc<dyn EmbeddingProvider>> {
    let provider: Arc<dyn EmbeddingProvider> = match config.provider {
        EmbeddingProviderType::Local => {
            let provider = LocalProvider::new()?;
            Arc::new(provider)
        }
        EmbeddingProviderType::AzureMl => {
            // Use provided config or fall back to environment variables
            let provider = if let Some(ref azure_config) = config.azure_ml {
                AzureMLProvider::new(azure_config.clone())?
            } else {
                AzureMLProvider::from_env()?
            };
            Arc::new(provider)
        }
        EmbeddingProviderType::Openai => {
            // Use provided config or fall back to environment variables
            let provider = if let Some(ref openai_config) = config.openai {
                OpenAIProvider::new(openai_config.clone())?
            } else {
                OpenAIProvider::from_env()?
            };
            Arc::new(provider)
        }
    };

    // Validate dimension matches what Qdrant collections expect
    // Note: OpenAI provider uses dynamic dimension detection, so we skip validation
    // for providers that might not know their dimension until first call
    if config.provider != EmbeddingProviderType::Openai {
        validate_dimension(provider.as_ref())?;
    }

    Ok(provider)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = EmbeddingConfig::default();
        assert_eq!(config.provider, EmbeddingProviderType::Local);
    }

    #[test]
    fn test_config_constructors() {
        assert_eq!(
            EmbeddingConfig::local().provider,
            EmbeddingProviderType::Local
        );
        assert_eq!(
            EmbeddingConfig::azure_ml().provider,
            EmbeddingProviderType::AzureMl
        );
        assert_eq!(
            EmbeddingConfig::openai().provider,
            EmbeddingProviderType::Openai
        );
    }

    #[test]
    fn test_factory_local() {
        let config = EmbeddingConfig::local();
        let result = create(&config);
        assert!(result.is_ok());
        let provider = result.unwrap();
        assert_eq!(provider.provider_type(), EmbeddingProviderType::Local);
        assert_eq!(provider.embedding_dim(), 768);
    }

    #[test]
    fn test_factory_azure_ml_requires_env() {
        // Without environment variables, Azure ML provider should fail
        // Clear any existing env vars for this test
        // SAFETY: This is a single-threaded test, environment manipulation is safe
        unsafe {
            std::env::remove_var("CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT");
            std::env::remove_var("CODEPRYSM_AZURE_ML_CODE_ENDPOINT");
            std::env::remove_var("CODEPRYSM_AZURE_ML_API_KEY");
            std::env::remove_var("CODEPRYSM_AZURE_ML_API_KEY_ENV");
        }

        let config = EmbeddingConfig::azure_ml();
        let result = create(&config);
        assert!(result.is_err());
    }

    #[test]
    fn test_factory_azure_ml_with_env() {
        // SAFETY: This is a single-threaded test, environment manipulation is safe
        unsafe {
            // Set required environment variables
            std::env::set_var(
                "CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT",
                "https://test-semantic.example.com/score",
            );
            std::env::set_var(
                "CODEPRYSM_AZURE_ML_CODE_ENDPOINT",
                "https://test-code.example.com/score",
            );
            std::env::set_var("CODEPRYSM_AZURE_ML_API_KEY", "test-key");
        }

        let config = EmbeddingConfig::azure_ml();
        let result = create(&config);

        // Clean up
        // SAFETY: This is a single-threaded test, environment manipulation is safe
        unsafe {
            std::env::remove_var("CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT");
            std::env::remove_var("CODEPRYSM_AZURE_ML_CODE_ENDPOINT");
            std::env::remove_var("CODEPRYSM_AZURE_ML_API_KEY");
        }

        assert!(result.is_ok());
        let provider = result.unwrap();
        assert_eq!(provider.provider_type(), EmbeddingProviderType::AzureMl);
        assert_eq!(provider.embedding_dim(), 768);
    }

    #[test]
    fn test_factory_openai_from_env() {
        // OpenAI provider uses environment variables with defaults
        // Should succeed even without explicit env vars (uses defaults)
        let config = EmbeddingConfig::openai();
        let result = create(&config);

        // Should succeed - OpenAI config has sensible defaults
        assert!(result.is_ok());
        let provider = result.unwrap();
        assert_eq!(provider.provider_type(), EmbeddingProviderType::Openai);
    }

    #[test]
    fn test_expected_dim() {
        // Verify EXPECTED_DIM matches collection config
        assert_eq!(EXPECTED_DIM, 768);
    }

    #[test]
    fn test_validate_dimension_local() {
        let provider = LocalProvider::new().unwrap();
        let result = validate_dimension(&provider);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_dimension_azure_ml() {
        // SAFETY: This is a single-threaded test, environment manipulation is safe
        unsafe {
            std::env::set_var(
                "CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT",
                "https://test-semantic.example.com/score",
            );
            std::env::set_var(
                "CODEPRYSM_AZURE_ML_CODE_ENDPOINT",
                "https://test-code.example.com/score",
            );
            std::env::set_var("CODEPRYSM_AZURE_ML_API_KEY", "test-key");
        }

        let provider = AzureMLProvider::from_env().unwrap();
        let result = validate_dimension(&provider);

        // Clean up
        // SAFETY: This is a single-threaded test, environment manipulation is safe
        unsafe {
            std::env::remove_var("CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT");
            std::env::remove_var("CODEPRYSM_AZURE_ML_CODE_ENDPOINT");
            std::env::remove_var("CODEPRYSM_AZURE_ML_API_KEY");
        }

        assert!(result.is_ok());
    }
}
