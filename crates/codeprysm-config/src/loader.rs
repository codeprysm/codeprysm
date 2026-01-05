//! Configuration loader with inheritance support.
//!
//! Loads configuration from multiple sources and merges them:
//! 1. Global config: `~/.codeprysm/config.toml`
//! 2. Local config: `.codeprysm/config.toml` (in workspace)
//! 3. CLI overrides
//!
//! Later sources override earlier ones.

use crate::error::ConfigError;
use crate::{ConfigOverrides, PrismConfig};
use std::path::{Path, PathBuf};
use tracing::{debug, trace};

/// Configuration file name.
const CONFIG_FILE_NAME: &str = "config.toml";

/// Global configuration directory name.
const GLOBAL_CONFIG_DIR: &str = ".codeprysm";

/// Local configuration directory name.
const LOCAL_CONFIG_DIR: &str = ".codeprysm";

/// Configuration loader with caching and inheritance support.
#[derive(Debug, Clone)]
pub struct ConfigLoader {
    /// Global config directory (e.g., `~/.codeprysm`)
    global_config_dir: Option<PathBuf>,

    /// Cached global config
    global_config: Option<PrismConfig>,
}

impl Default for ConfigLoader {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigLoader {
    /// Create a new configuration loader.
    ///
    /// Automatically detects the global config directory (`~/.codeprysm`).
    pub fn new() -> Self {
        let global_config_dir = dirs::home_dir().map(|h| h.join(GLOBAL_CONFIG_DIR));

        Self {
            global_config_dir,
            global_config: None,
        }
    }

    /// Create a loader with a custom global config directory.
    ///
    /// Useful for testing.
    pub fn with_global_dir(global_dir: impl Into<PathBuf>) -> Self {
        Self {
            global_config_dir: Some(global_dir.into()),
            global_config: None,
        }
    }

    /// Get the global config file path.
    pub fn global_config_path(&self) -> Option<PathBuf> {
        self.global_config_dir
            .as_ref()
            .map(|d| d.join(CONFIG_FILE_NAME))
    }

    /// Get the local config file path for a workspace.
    pub fn local_config_path(&self, workspace_root: &Path) -> PathBuf {
        workspace_root.join(LOCAL_CONFIG_DIR).join(CONFIG_FILE_NAME)
    }

    /// Load configuration for a workspace with optional CLI overrides.
    ///
    /// Merges config in order: global → local → overrides.
    pub fn load(
        &mut self,
        workspace_root: &Path,
        overrides: Option<&ConfigOverrides>,
    ) -> Result<PrismConfig, ConfigError> {
        // Start with default config
        let mut config = PrismConfig::default();

        // Apply global config if available
        if let Some(global_config) = self.load_global()? {
            config = merge_configs(config, global_config);
        }

        // Apply local config if available
        if let Some(local_config) = self.load_local(workspace_root)? {
            config = merge_configs(config, local_config);
        }

        // Apply CLI overrides
        if let Some(ovr) = overrides {
            config.apply_overrides(ovr);
        }

        Ok(config)
    }

    /// Load only the global configuration.
    pub fn load_global(&mut self) -> Result<Option<PrismConfig>, ConfigError> {
        // Return cached global config if available
        if let Some(ref config) = self.global_config {
            return Ok(Some(config.clone()));
        }

        let Some(global_path) = self.global_config_path() else {
            debug!("No home directory found, skipping global config");
            return Ok(None);
        };

        if !global_path.exists() {
            trace!("Global config not found at {:?}", global_path);
            return Ok(None);
        }

        debug!("Loading global config from {:?}", global_path);
        let config = load_config_file(&global_path)?;

        // Cache the global config
        self.global_config = Some(config.clone());

        Ok(Some(config))
    }

    /// Load only the local configuration for a workspace.
    pub fn load_local(&self, workspace_root: &Path) -> Result<Option<PrismConfig>, ConfigError> {
        let local_path = self.local_config_path(workspace_root);

        if !local_path.exists() {
            trace!("Local config not found at {:?}", local_path);
            return Ok(None);
        }

        debug!("Loading local config from {:?}", local_path);
        load_config_file(&local_path).map(Some)
    }

    /// Save configuration to the global config file.
    pub fn save_global(&self, config: &PrismConfig) -> Result<(), ConfigError> {
        let Some(ref global_dir) = self.global_config_dir else {
            return Err(ConfigError::NoHomeDir);
        };

        let global_path = global_dir.join(CONFIG_FILE_NAME);
        save_config_file(&global_path, config)
    }

    /// Save configuration to the local config file for a workspace.
    pub fn save_local(
        &self,
        workspace_root: &Path,
        config: &PrismConfig,
    ) -> Result<(), ConfigError> {
        let local_path = self.local_config_path(workspace_root);
        save_config_file(&local_path, config)
    }

    /// Initialize global configuration directory.
    ///
    /// Creates `~/.codeprysm/config.toml` with default configuration.
    pub fn init_global(&self) -> Result<PathBuf, ConfigError> {
        let Some(ref global_dir) = self.global_config_dir else {
            return Err(ConfigError::NoHomeDir);
        };

        // Create directory if it doesn't exist
        if !global_dir.exists() {
            std::fs::create_dir_all(global_dir)
                .map_err(|e| ConfigError::create_dir(global_dir, e))?;
        }

        let config_path = global_dir.join(CONFIG_FILE_NAME);
        if !config_path.exists() {
            let default_config = PrismConfig::default();
            save_config_file(&config_path, &default_config)?;
        }

        Ok(config_path)
    }

    /// Initialize local configuration for a workspace.
    ///
    /// Creates `.codeprysm/config.toml` with default configuration.
    pub fn init_local(&self, workspace_root: &Path) -> Result<PathBuf, ConfigError> {
        let local_dir = workspace_root.join(LOCAL_CONFIG_DIR);

        // Create directory if it doesn't exist
        if !local_dir.exists() {
            std::fs::create_dir_all(&local_dir)
                .map_err(|e| ConfigError::create_dir(&local_dir, e))?;
        }

        let config_path = local_dir.join(CONFIG_FILE_NAME);
        if !config_path.exists() {
            let default_config = PrismConfig::default();
            save_config_file(&config_path, &default_config)?;
        }

        Ok(config_path)
    }

    /// Clear cached global configuration.
    ///
    /// Forces reload on next `load_global()` call.
    pub fn clear_cache(&mut self) {
        self.global_config = None;
    }
}

/// Load a configuration file from disk.
fn load_config_file(path: &Path) -> Result<PrismConfig, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|e| ConfigError::read_file(path, e))?;

    toml::from_str(&content).map_err(|e| ConfigError::parse_toml(path, e))
}

/// Save a configuration file to disk.
fn save_config_file(path: &Path, config: &PrismConfig) -> Result<(), ConfigError> {
    // Ensure parent directory exists
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| ConfigError::create_dir(parent, e))?;
        }
    }

    let content = toml::to_string_pretty(config)?;
    std::fs::write(path, content).map_err(|e| ConfigError::write_file(path, e))
}

/// Merge two configurations, with `overlay` taking precedence.
///
/// This performs a field-by-field merge, allowing partial configs.
fn merge_configs(base: PrismConfig, overlay: PrismConfig) -> PrismConfig {
    PrismConfig {
        storage: merge_storage(base.storage, overlay.storage),
        backend: merge_backend(base.backend, overlay.backend),
        embedding: merge_embedding(base.embedding, overlay.embedding),
        analysis: merge_analysis(base.analysis, overlay.analysis),
        workspace: merge_workspace(base.workspace, overlay.workspace),
        logging: merge_logging(base.logging, overlay.logging),
    }
}

/// Merge storage config, overlay values override base.
fn merge_storage(
    base: crate::StorageConfig,
    overlay: crate::StorageConfig,
) -> crate::StorageConfig {
    crate::StorageConfig {
        // Use overlay if it differs from default, otherwise keep base
        prism_dir: if overlay.prism_dir != Path::new(".codeprysm") {
            overlay.prism_dir
        } else {
            base.prism_dir
        },
        graph_format: overlay.graph_format, // Always use overlay
        compression: overlay.compression,
        max_partition_size_mb: if overlay.max_partition_size_mb != 100 {
            overlay.max_partition_size_mb
        } else {
            base.max_partition_size_mb
        },
    }
}

/// Merge backend config.
fn merge_backend(
    base: crate::BackendConfig,
    overlay: crate::BackendConfig,
) -> crate::BackendConfig {
    crate::BackendConfig {
        backend_type: overlay.backend_type,
        qdrant: merge_qdrant(base.qdrant, overlay.qdrant),
        remote: overlay.remote.or(base.remote),
    }
}

/// Merge Qdrant config.
fn merge_qdrant(base: crate::QdrantConfig, overlay: crate::QdrantConfig) -> crate::QdrantConfig {
    crate::QdrantConfig {
        url: if overlay.url != "http://localhost:6334" {
            overlay.url
        } else {
            base.url
        },
        api_key: overlay.api_key.or(base.api_key),
        collection_prefix: if overlay.collection_prefix != "codeprysm" {
            overlay.collection_prefix
        } else {
            base.collection_prefix
        },
        vector_dimension: if overlay.vector_dimension != 768 {
            overlay.vector_dimension
        } else {
            base.vector_dimension
        },
        hnsw_enabled: overlay.hnsw_enabled,
    }
}

/// Merge embedding config.
fn merge_embedding(
    base: crate::EmbeddingConfig,
    overlay: crate::EmbeddingConfig,
) -> crate::EmbeddingConfig {
    crate::EmbeddingConfig {
        // Provider type from overlay if it differs from default
        provider: if overlay.provider != crate::EmbeddingProviderType::Local {
            overlay.provider
        } else {
            base.provider
        },
        // Overlay azure_ml takes precedence if set
        azure_ml: overlay.azure_ml.or(base.azure_ml),
        // Overlay openai takes precedence if set
        openai: overlay.openai.or(base.openai),
    }
}

/// Merge analysis config.
fn merge_analysis(
    base: crate::AnalysisConfig,
    overlay: crate::AnalysisConfig,
) -> crate::AnalysisConfig {
    crate::AnalysisConfig {
        max_file_size_kb: if overlay.max_file_size_kb != 1024 {
            overlay.max_file_size_kb
        } else {
            base.max_file_size_kb
        },
        // Merge patterns: overlay patterns extend base patterns
        exclude_patterns: if overlay.exclude_patterns.is_empty() {
            base.exclude_patterns
        } else {
            // Combine both, with overlay patterns added
            let mut patterns = base.exclude_patterns;
            for pattern in overlay.exclude_patterns {
                if !patterns.contains(&pattern) {
                    patterns.push(pattern);
                }
            }
            patterns
        },
        include_patterns: if overlay.include_patterns.is_empty() {
            base.include_patterns
        } else {
            let mut patterns = base.include_patterns;
            for pattern in overlay.include_patterns {
                if !patterns.contains(&pattern) {
                    patterns.push(pattern);
                }
            }
            patterns
        },
        detect_components: overlay.detect_components,
        parallelism: if overlay.parallelism != 0 {
            overlay.parallelism
        } else {
            base.parallelism
        },
        languages: {
            let mut langs = base.languages;
            langs.extend(overlay.languages);
            langs
        },
    }
}

/// Merge workspace config.
fn merge_workspace(
    base: crate::WorkspaceConfig,
    overlay: crate::WorkspaceConfig,
) -> crate::WorkspaceConfig {
    crate::WorkspaceConfig {
        workspaces: {
            let mut ws = base.workspaces;
            ws.extend(overlay.workspaces);
            ws
        },
        active: overlay.active.or(base.active),
        cross_workspace_search: overlay.cross_workspace_search,
    }
}

/// Merge logging config.
fn merge_logging(
    base: crate::LoggingConfig,
    overlay: crate::LoggingConfig,
) -> crate::LoggingConfig {
    crate::LoggingConfig {
        level: if overlay.level != "info" {
            overlay.level
        } else {
            base.level
        },
        format: overlay.format,
        file: overlay.file.or(base.file),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_config(content: &str, dir: &Path, filename: &str) -> PathBuf {
        let config_dir = dir.join(".codeprysm");
        std::fs::create_dir_all(&config_dir).unwrap();
        let path = config_dir.join(filename);
        std::fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_load_default_config() {
        let temp = TempDir::new().unwrap();
        let mut loader = ConfigLoader::with_global_dir(temp.path().join("global"));

        let config = loader.load(temp.path(), None).unwrap();

        // Should get defaults
        assert_eq!(config.storage.prism_dir, PathBuf::from(".codeprysm"));
        assert_eq!(config.backend.qdrant.url, "http://localhost:6334");
    }

    #[test]
    fn test_load_local_config() {
        let temp = TempDir::new().unwrap();
        let mut loader = ConfigLoader::with_global_dir(temp.path().join("global"));

        // Create local config
        create_test_config(
            r#"
            [storage]
            prism_dir = ".custom-prism"

            [backend.qdrant]
            url = "http://custom:6334"
            "#,
            temp.path(),
            "config.toml",
        );

        let config = loader.load(temp.path(), None).unwrap();

        assert_eq!(config.storage.prism_dir, PathBuf::from(".custom-prism"));
        assert_eq!(config.backend.qdrant.url, "http://custom:6334");
    }

    #[test]
    fn test_global_overrides_default() {
        let temp = TempDir::new().unwrap();
        let global_dir = temp.path().join("global");

        // Create global config
        std::fs::create_dir_all(&global_dir).unwrap();
        std::fs::write(
            global_dir.join("config.toml"),
            r#"
            [logging]
            level = "debug"
            "#,
        )
        .unwrap();

        let mut loader = ConfigLoader::with_global_dir(&global_dir);
        let config = loader.load(temp.path(), None).unwrap();

        assert_eq!(config.logging.level, "debug");
    }

    #[test]
    fn test_local_overrides_global() {
        let temp = TempDir::new().unwrap();
        let global_dir = temp.path().join("global");

        // Create global config
        std::fs::create_dir_all(&global_dir).unwrap();
        std::fs::write(
            global_dir.join("config.toml"),
            r#"
            [logging]
            level = "debug"

            [backend.qdrant]
            url = "http://global:6334"
            "#,
        )
        .unwrap();

        // Create local config that overrides Qdrant URL but not log level
        create_test_config(
            r#"
            [backend.qdrant]
            url = "http://local:6334"
            "#,
            temp.path(),
            "config.toml",
        );

        let mut loader = ConfigLoader::with_global_dir(&global_dir);
        let config = loader.load(temp.path(), None).unwrap();

        // Local override should take effect
        assert_eq!(config.backend.qdrant.url, "http://local:6334");
        // Global value should be preserved (since local doesn't override)
        assert_eq!(config.logging.level, "debug");
    }

    #[test]
    fn test_cli_overrides_all() {
        let temp = TempDir::new().unwrap();

        // Create local config
        create_test_config(
            r#"
            [backend.qdrant]
            url = "http://local:6334"
            "#,
            temp.path(),
            "config.toml",
        );

        let mut loader = ConfigLoader::with_global_dir(temp.path().join("global"));

        let overrides = ConfigOverrides {
            qdrant_url: Some("http://cli:6334".to_string()),
            log_level: Some("trace".to_string()),
            ..Default::default()
        };

        let config = loader.load(temp.path(), Some(&overrides)).unwrap();

        // CLI should override local
        assert_eq!(config.backend.qdrant.url, "http://cli:6334");
        assert_eq!(config.logging.level, "trace");
    }

    #[test]
    fn test_save_and_load_config() {
        let temp = TempDir::new().unwrap();
        let loader = ConfigLoader::with_global_dir(temp.path().join("global"));

        let mut config = PrismConfig::default();
        config.backend.qdrant.url = "http://saved:6334".to_string();
        config.logging.level = "warn".to_string();

        // Save to local
        loader.save_local(temp.path(), &config).unwrap();

        // Load it back
        let mut loader = ConfigLoader::with_global_dir(temp.path().join("global"));
        let loaded = loader.load(temp.path(), None).unwrap();

        assert_eq!(loaded.backend.qdrant.url, "http://saved:6334");
        assert_eq!(loaded.logging.level, "warn");
    }

    #[test]
    fn test_init_local_creates_config() {
        let temp = TempDir::new().unwrap();
        let loader = ConfigLoader::with_global_dir(temp.path().join("global"));

        let config_path = loader.init_local(temp.path()).unwrap();

        assert!(config_path.exists());
        assert!(config_path.ends_with(".codeprysm/config.toml"));

        // Should be valid TOML
        let content = std::fs::read_to_string(&config_path).unwrap();
        let _: PrismConfig = toml::from_str(&content).unwrap();
    }

    #[test]
    fn test_exclude_patterns_merge() {
        let base = crate::AnalysisConfig {
            exclude_patterns: vec!["**/node_modules/**".to_string()],
            ..Default::default()
        };

        let overlay = crate::AnalysisConfig {
            exclude_patterns: vec!["**/custom/**".to_string()],
            ..Default::default()
        };

        let merged = merge_analysis(base, overlay);

        // Should have both patterns
        assert!(merged
            .exclude_patterns
            .contains(&"**/node_modules/**".to_string()));
        assert!(merged
            .exclude_patterns
            .contains(&"**/custom/**".to_string()));
    }

    #[test]
    fn test_workspace_merge() {
        let mut base_ws = std::collections::HashMap::new();
        base_ws.insert("project-a".to_string(), PathBuf::from("/a"));

        let mut overlay_ws = std::collections::HashMap::new();
        overlay_ws.insert("project-b".to_string(), PathBuf::from("/b"));

        let base = crate::WorkspaceConfig {
            workspaces: base_ws,
            active: Some("project-a".to_string()),
            ..Default::default()
        };

        let overlay = crate::WorkspaceConfig {
            workspaces: overlay_ws,
            active: None, // Don't change active
            ..Default::default()
        };

        let merged = merge_workspace(base, overlay);

        // Should have both workspaces
        assert!(merged.workspaces.contains_key("project-a"));
        assert!(merged.workspaces.contains_key("project-b"));
        // Active should be preserved from base
        assert_eq!(merged.active, Some("project-a".to_string()));
    }

    #[test]
    fn test_cache_clearing() {
        let temp = TempDir::new().unwrap();
        let global_dir = temp.path().join("global");

        // Create global config
        std::fs::create_dir_all(&global_dir).unwrap();
        std::fs::write(
            global_dir.join("config.toml"),
            r#"
            [logging]
            level = "debug"
            "#,
        )
        .unwrap();

        let mut loader = ConfigLoader::with_global_dir(&global_dir);

        // First load caches
        let _ = loader.load_global().unwrap();
        assert!(loader.global_config.is_some());

        // Clear cache
        loader.clear_cache();
        assert!(loader.global_config.is_none());
    }
}
