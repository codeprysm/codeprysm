//! CodePrism Configuration Management
//!
//! Provides configuration loading with support for:
//! - Global config: `~/.codeprysm/config.toml`
//! - Local config: `.codeprysm/config.toml` (in workspace)
//! - CLI overrides via `ConfigOverrides`
//!
//! Configuration is merged in order: global → local → CLI overrides.

mod error;
mod loader;

pub use error::ConfigError;
pub use loader::ConfigLoader;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Root configuration for CodePrism.
///
/// Represents the fully merged configuration from all sources.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct PrismConfig {
    /// Storage configuration
    pub storage: StorageConfig,

    /// Backend configuration
    pub backend: BackendConfig,

    /// Embedding provider configuration
    pub embedding: EmbeddingConfig,

    /// Analysis configuration
    pub analysis: AnalysisConfig,

    /// Workspace configuration
    pub workspace: WorkspaceConfig,

    /// Logging configuration
    pub logging: LoggingConfig,
}

/// Embedding provider configuration.
///
/// Controls which provider generates embeddings for semantic search.
///
/// # Example TOML
///
/// ```toml
/// [embedding]
/// provider = "local"  # or "azure-ml" or "openai"
///
/// [embedding.azure_ml]
/// semantic_endpoint = "https://..."
/// code_endpoint = "https://..."
/// auth_key_env = "AZURE_ML_KEY"
///
/// [embedding.openai]
/// url = "https://api.openai.com/v1"
/// api_key_env = "OPENAI_API_KEY"
/// semantic_model = "text-embedding-3-small"
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct EmbeddingConfig {
    /// Which embedding provider to use
    pub provider: EmbeddingProviderType,

    /// Azure ML provider settings (required when provider = "azure-ml")
    pub azure_ml: Option<AzureMLSettings>,

    /// OpenAI-compatible provider settings (required when provider = "openai")
    pub openai: Option<OpenAISettings>,
}

impl EmbeddingConfig {
    /// Validate that required settings exist for the selected provider.
    pub fn validate(&self) -> Result<(), ConfigError> {
        match self.provider {
            EmbeddingProviderType::Local => Ok(()),
            EmbeddingProviderType::AzureMl => {
                if self.azure_ml.is_none() {
                    return Err(ConfigError::ValidationError(
                        "embedding.provider is 'azure-ml' but [embedding.azure_ml] section is missing".to_string()
                    ));
                }
                let settings = self.azure_ml.as_ref().unwrap();
                if settings.semantic_endpoint.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "embedding.azure_ml.semantic_endpoint is required".to_string(),
                    ));
                }
                if settings.code_endpoint.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "embedding.azure_ml.code_endpoint is required".to_string(),
                    ));
                }
                Ok(())
            }
            EmbeddingProviderType::Openai => {
                if self.openai.is_none() {
                    return Err(ConfigError::ValidationError(
                        "embedding.provider is 'openai' but [embedding.openai] section is missing"
                            .to_string(),
                    ));
                }
                let settings = self.openai.as_ref().unwrap();
                if settings.url.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "embedding.openai.url is required".to_string(),
                    ));
                }
                if settings.semantic_model.is_empty() {
                    return Err(ConfigError::ValidationError(
                        "embedding.openai.semantic_model is required".to_string(),
                    ));
                }
                Ok(())
            }
        }
    }
}

/// Embedding provider type selection.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EmbeddingProviderType {
    /// Local provider using Candle with Jina models (default)
    #[default]
    Local,
    /// Azure ML Online Endpoints
    AzureMl,
    /// OpenAI-compatible API (OpenAI, Azure OpenAI, Ollama, etc.)
    Openai,
}

impl std::fmt::Display for EmbeddingProviderType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local => write!(f, "local"),
            Self::AzureMl => write!(f, "azure-ml"),
            Self::Openai => write!(f, "openai"),
        }
    }
}

impl std::str::FromStr for EmbeddingProviderType {
    type Err = ConfigError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "local" => Ok(Self::Local),
            "azure-ml" | "azureml" | "azure_ml" => Ok(Self::AzureMl),
            "openai" => Ok(Self::Openai),
            _ => Err(ConfigError::ValidationError(format!(
                "Unknown embedding provider: '{}'. Valid values: local, azure-ml, openai",
                s
            ))),
        }
    }
}

/// Azure ML provider settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AzureMLSettings {
    /// Semantic embedding endpoint URL
    pub semantic_endpoint: String,

    /// Code embedding endpoint URL
    pub code_endpoint: String,

    /// Environment variable name containing API key for semantic endpoint.
    /// Takes precedence over `auth_key_env` for semantic requests.
    pub semantic_auth_key_env: Option<String>,

    /// Environment variable name containing API key for code endpoint.
    /// If not set, falls back to `semantic_auth_key_env` or `auth_key_env`.
    pub code_auth_key_env: Option<String>,

    /// Legacy: Environment variable name containing shared API key.
    /// Use `semantic_auth_key_env` and `code_auth_key_env` for separate keys.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub auth_key_env: Option<String>,

    /// Request timeout in seconds
    pub timeout_secs: u64,

    /// Maximum retry attempts
    pub max_retries: u32,
}

impl Default for AzureMLSettings {
    fn default() -> Self {
        Self {
            semantic_endpoint: String::new(),
            code_endpoint: String::new(),
            semantic_auth_key_env: Some("CODEPRYSM_AZURE_ML_SEMANTIC_API_KEY".to_string()),
            code_auth_key_env: Some("CODEPRYSM_AZURE_ML_CODE_API_KEY".to_string()),
            auth_key_env: None,
            timeout_secs: 30,
            max_retries: 3,
        }
    }
}

/// OpenAI-compatible provider settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OpenAISettings {
    /// API base URL (e.g., "https://api.openai.com/v1")
    pub url: String,

    /// Environment variable name containing API key
    pub api_key_env: Option<String>,

    /// Model for semantic embeddings
    pub semantic_model: String,

    /// Model for code embeddings (None = use semantic_model)
    pub code_model: Option<String>,

    /// Request timeout in seconds
    pub timeout_secs: u64,

    /// Maximum retry attempts
    pub max_retries: u32,

    /// Use Azure OpenAI authentication (api-key header)
    pub azure_mode: bool,
}

impl Default for OpenAISettings {
    fn default() -> Self {
        Self {
            url: "https://api.openai.com/v1".to_string(),
            api_key_env: Some("OPENAI_API_KEY".to_string()),
            semantic_model: "text-embedding-3-small".to_string(),
            code_model: None,
            timeout_secs: 30,
            max_retries: 3,
            azure_mode: false,
        }
    }
}

/// Storage configuration for graph and index data.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct StorageConfig {
    /// Directory for CodePrysm data (default: `.codeprysm`)
    pub prism_dir: PathBuf,

    /// Graph storage format
    pub graph_format: GraphFormat,

    /// Enable compression for stored data
    pub compression: bool,

    /// Maximum partition size in MB (for lazy loading)
    pub max_partition_size_mb: u32,
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            prism_dir: PathBuf::from(".codeprysm"),
            graph_format: GraphFormat::default(),
            compression: false,
            max_partition_size_mb: 100,
        }
    }
}

/// Graph storage format.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum GraphFormat {
    /// SQLite-based partitioned storage (default)
    #[default]
    Sqlite,
    /// JSON format (for debugging/export)
    Json,
}

/// Backend configuration for search and query operations.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BackendConfig {
    /// Backend type to use
    pub backend_type: BackendType,

    /// Qdrant configuration (for local backend)
    pub qdrant: QdrantConfig,

    /// Remote server configuration (for HTTP backend)
    pub remote: Option<RemoteConfig>,
}

/// Backend type selection.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum BackendType {
    /// Local backend with direct file system and Qdrant access
    #[default]
    Local,
    /// Remote HTTP backend (connects to CodePrysm server)
    Remote,
}

/// Qdrant vector database configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct QdrantConfig {
    /// Qdrant server URL
    pub url: String,

    /// API key for authentication (optional)
    pub api_key: Option<String>,

    /// Collection name prefix
    pub collection_prefix: String,

    /// Vector dimension (must match embedding model)
    pub vector_dimension: u32,

    /// Enable HNSW indexing
    pub hnsw_enabled: bool,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:6334".to_string(),
            api_key: None,
            collection_prefix: "codeprysm".to_string(),
            vector_dimension: 768, // jina-base-code default
            hnsw_enabled: true,
        }
    }
}

/// Remote CodePrysm server configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// Remote server URL
    pub url: String,

    /// API key for authentication
    pub api_key: Option<String>,

    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for RemoteConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:8080".to_string(),
            api_key: None,
            timeout_secs: 30,
        }
    }
}

/// Analysis configuration for code parsing.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AnalysisConfig {
    /// Maximum file size to analyze (in KB)
    pub max_file_size_kb: u64,

    /// File patterns to exclude (glob patterns)
    pub exclude_patterns: Vec<String>,

    /// Additional file patterns to include
    pub include_patterns: Vec<String>,

    /// Enable component detection (manifests)
    pub detect_components: bool,

    /// Parallelism level (0 = auto-detect)
    pub parallelism: usize,

    /// Language-specific settings
    pub languages: HashMap<String, LanguageConfig>,
}

impl Default for AnalysisConfig {
    fn default() -> Self {
        Self {
            max_file_size_kb: 1024, // 1MB default
            exclude_patterns: vec![
                "**/node_modules/**".to_string(),
                "**/target/**".to_string(),
                "**/.git/**".to_string(),
                "**/vendor/**".to_string(),
                "**/__pycache__/**".to_string(),
                "**/dist/**".to_string(),
                "**/build/**".to_string(),
            ],
            include_patterns: Vec::new(),
            detect_components: true,
            parallelism: 0, // auto-detect
            languages: HashMap::new(),
        }
    }
}

/// Language-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct LanguageConfig {
    /// Enable this language
    pub enabled: bool,

    /// Custom query file path
    pub query_file: Option<PathBuf>,
}

/// Workspace configuration for multi-repo support.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkspaceConfig {
    /// Registered workspaces (name → path mapping)
    pub workspaces: HashMap<String, PathBuf>,

    /// Active workspace name (for CLI operations)
    pub active: Option<String>,

    /// Enable cross-workspace search
    pub cross_workspace_search: bool,
}

impl Default for WorkspaceConfig {
    fn default() -> Self {
        Self {
            workspaces: HashMap::new(),
            active: None,
            cross_workspace_search: true,
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    pub level: String,

    /// Log format (text, json)
    pub format: LogFormat,

    /// Log file path (optional)
    pub file: Option<PathBuf>,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: "info".to_string(),
            format: LogFormat::default(),
            file: None,
        }
    }
}

/// Log output format.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable text format
    #[default]
    Text,
    /// JSON structured logging
    Json,
}

/// CLI overrides for configuration values.
///
/// Used to apply command-line arguments over file-based config.
#[derive(Debug, Clone, Default)]
pub struct ConfigOverrides {
    /// Override workspace root directory
    pub workspace_root: Option<PathBuf>,

    /// Override CodePrysm data directory
    pub prism_dir: Option<PathBuf>,

    /// Override Qdrant URL
    pub qdrant_url: Option<String>,

    /// Override backend type
    pub backend_type: Option<BackendType>,

    /// Override embedding provider type
    pub embedding_provider: Option<EmbeddingProviderType>,

    /// Override log level
    pub log_level: Option<String>,

    /// Override parallelism
    pub parallelism: Option<usize>,
}

impl PrismConfig {
    /// Apply CLI overrides to this configuration.
    pub fn apply_overrides(&mut self, overrides: &ConfigOverrides) {
        if let Some(ref dir) = overrides.prism_dir {
            self.storage.prism_dir = dir.clone();
        }

        if let Some(ref url) = overrides.qdrant_url {
            self.backend.qdrant.url = url.clone();
        }

        if let Some(ref backend_type) = overrides.backend_type {
            self.backend.backend_type = backend_type.clone();
        }

        if let Some(embedding_provider) = overrides.embedding_provider {
            self.embedding.provider = embedding_provider;
        }

        if let Some(ref level) = overrides.log_level {
            self.logging.level = level.clone();
        }

        if let Some(parallelism) = overrides.parallelism {
            self.analysis.parallelism = parallelism;
        }
    }

    /// Validate the configuration.
    ///
    /// Checks that all required settings exist for the selected providers.
    pub fn validate(&self) -> Result<(), ConfigError> {
        self.embedding.validate()?;
        Ok(())
    }

    /// Get the effective CodePrysm directory for a workspace.
    pub fn prism_dir(&self, workspace_root: &std::path::Path) -> PathBuf {
        if self.storage.prism_dir.is_absolute() {
            self.storage.prism_dir.clone()
        } else {
            workspace_root.join(&self.storage.prism_dir)
        }
    }

    /// Get the graph file path for a workspace.
    pub fn graph_path(&self, workspace_root: &std::path::Path) -> PathBuf {
        let prism_dir = self.prism_dir(workspace_root);
        match self.storage.graph_format {
            GraphFormat::Sqlite => prism_dir.join("manifest.json"),
            GraphFormat::Json => prism_dir.join("graph.json"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = PrismConfig::default();
        assert_eq!(config.storage.prism_dir, PathBuf::from(".codeprysm"));
        assert_eq!(config.storage.graph_format, GraphFormat::Sqlite);
        assert_eq!(config.backend.backend_type, BackendType::Local);
        assert_eq!(config.backend.qdrant.url, "http://localhost:6334");
        assert!(config.analysis.detect_components);
    }

    #[test]
    fn test_apply_overrides() {
        let mut config = PrismConfig::default();
        let overrides = ConfigOverrides {
            prism_dir: Some(PathBuf::from("/custom/prism")),
            qdrant_url: Some("http://remote:6334".to_string()),
            log_level: Some("debug".to_string()),
            ..Default::default()
        };

        config.apply_overrides(&overrides);

        assert_eq!(config.storage.prism_dir, PathBuf::from("/custom/prism"));
        assert_eq!(config.backend.qdrant.url, "http://remote:6334");
        assert_eq!(config.logging.level, "debug");
    }

    #[test]
    fn test_prism_dir_resolution() {
        let config = PrismConfig::default();
        let workspace = PathBuf::from("/home/user/project");

        let prism_dir = config.prism_dir(&workspace);
        assert_eq!(prism_dir, PathBuf::from("/home/user/project/.codeprysm"));
    }

    #[test]
    fn test_prism_dir_absolute() {
        let mut config = PrismConfig::default();
        config.storage.prism_dir = PathBuf::from("/absolute/path/.codeprysm");
        let workspace = PathBuf::from("/home/user/project");

        let prism_dir = config.prism_dir(&workspace);
        assert_eq!(prism_dir, PathBuf::from("/absolute/path/.codeprysm"));
    }

    #[test]
    fn test_graph_path_sqlite() {
        let config = PrismConfig::default();
        let workspace = PathBuf::from("/project");

        let path = config.graph_path(&workspace);
        assert_eq!(path, PathBuf::from("/project/.codeprysm/manifest.json"));
    }

    #[test]
    fn test_graph_path_json() {
        let mut config = PrismConfig::default();
        config.storage.graph_format = GraphFormat::Json;
        let workspace = PathBuf::from("/project");

        let path = config.graph_path(&workspace);
        assert_eq!(path, PathBuf::from("/project/.codeprysm/graph.json"));
    }

    #[test]
    fn test_embedding_config_default() {
        let config = EmbeddingConfig::default();
        assert_eq!(config.provider, EmbeddingProviderType::Local);
        assert!(config.azure_ml.is_none());
        assert!(config.openai.is_none());
    }

    #[test]
    fn test_embedding_provider_type_display() {
        assert_eq!(EmbeddingProviderType::Local.to_string(), "local");
        assert_eq!(EmbeddingProviderType::AzureMl.to_string(), "azure-ml");
        assert_eq!(EmbeddingProviderType::Openai.to_string(), "openai");
    }

    #[test]
    fn test_embedding_provider_type_from_str() {
        assert_eq!(
            "local".parse::<EmbeddingProviderType>().unwrap(),
            EmbeddingProviderType::Local
        );
        assert_eq!(
            "azure-ml".parse::<EmbeddingProviderType>().unwrap(),
            EmbeddingProviderType::AzureMl
        );
        assert_eq!(
            "azureml".parse::<EmbeddingProviderType>().unwrap(),
            EmbeddingProviderType::AzureMl
        );
        assert_eq!(
            "azure_ml".parse::<EmbeddingProviderType>().unwrap(),
            EmbeddingProviderType::AzureMl
        );
        assert_eq!(
            "openai".parse::<EmbeddingProviderType>().unwrap(),
            EmbeddingProviderType::Openai
        );
        assert!("unknown".parse::<EmbeddingProviderType>().is_err());
    }

    #[test]
    fn test_embedding_config_validate_local() {
        let config = EmbeddingConfig::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_embedding_config_validate_azure_ml_missing() {
        let config = EmbeddingConfig {
            provider: EmbeddingProviderType::AzureMl,
            azure_ml: None,
            openai: None,
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("azure_ml"));
    }

    #[test]
    fn test_embedding_config_validate_azure_ml_valid() {
        let config = EmbeddingConfig {
            provider: EmbeddingProviderType::AzureMl,
            azure_ml: Some(AzureMLSettings {
                semantic_endpoint: "https://semantic.example.com/score".to_string(),
                code_endpoint: "https://code.example.com/score".to_string(),
                ..Default::default()
            }),
            openai: None,
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_embedding_config_validate_openai_missing() {
        let config = EmbeddingConfig {
            provider: EmbeddingProviderType::Openai,
            azure_ml: None,
            openai: None,
        };
        let err = config.validate().unwrap_err();
        assert!(err.to_string().contains("openai"));
    }

    #[test]
    fn test_embedding_config_validate_openai_valid() {
        let config = EmbeddingConfig {
            provider: EmbeddingProviderType::Openai,
            azure_ml: None,
            openai: Some(OpenAISettings {
                url: "https://api.openai.com/v1".to_string(),
                semantic_model: "text-embedding-3-small".to_string(),
                ..Default::default()
            }),
        };
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_apply_embedding_provider_override() {
        let mut config = PrismConfig::default();
        assert_eq!(config.embedding.provider, EmbeddingProviderType::Local);

        let overrides = ConfigOverrides {
            embedding_provider: Some(EmbeddingProviderType::AzureMl),
            ..Default::default()
        };
        config.apply_overrides(&overrides);

        assert_eq!(config.embedding.provider, EmbeddingProviderType::AzureMl);
    }

    #[test]
    fn test_embedding_config_toml_roundtrip() {
        let config = EmbeddingConfig {
            provider: EmbeddingProviderType::AzureMl,
            azure_ml: Some(AzureMLSettings {
                semantic_endpoint: "https://semantic.example.com/score".to_string(),
                code_endpoint: "https://code.example.com/score".to_string(),
                semantic_auth_key_env: Some("MY_SEMANTIC_KEY".to_string()),
                code_auth_key_env: Some("MY_CODE_KEY".to_string()),
                auth_key_env: None,
                timeout_secs: 60,
                max_retries: 5,
            }),
            openai: None,
        };

        let toml_str = toml::to_string(&config).unwrap();
        let parsed: EmbeddingConfig = toml::from_str(&toml_str).unwrap();

        assert_eq!(parsed.provider, EmbeddingProviderType::AzureMl);
        assert!(parsed.azure_ml.is_some());
        let azure_ml = parsed.azure_ml.unwrap();
        assert_eq!(
            azure_ml.semantic_endpoint,
            "https://semantic.example.com/score"
        );
        assert_eq!(
            azure_ml.semantic_auth_key_env,
            Some("MY_SEMANTIC_KEY".to_string())
        );
        assert_eq!(azure_ml.code_auth_key_env, Some("MY_CODE_KEY".to_string()));
        assert_eq!(azure_ml.timeout_secs, 60);
    }
}
