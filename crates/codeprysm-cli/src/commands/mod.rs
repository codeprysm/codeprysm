//! CLI command implementations
//!
//! This module contains all Prism CLI command implementations.

pub mod backend;
pub mod clean;
pub mod components;
pub mod config;
pub mod doctor;
pub mod graph;
pub mod init;
pub mod mcp;
pub mod search;
pub mod status;
pub mod update;
pub mod workspace;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result};
use codeprysm_backend::{LocalBackend, WorkspaceRegistry};
use codeprysm_config::{ConfigLoader, PrismConfig};
use codeprysm_search::embeddings::{
    AzureMLAuth, AzureMLConfig, EmbeddingConfig as SearchEmbeddingConfig, OpenAIConfig,
};

use crate::GlobalOptions;

/// Resolve the workspace path from options or current directory.
pub async fn resolve_workspace(global: &GlobalOptions) -> Result<PathBuf> {
    if let Some(ref ws) = global.workspace {
        // Could be a name (from registry) or a path
        let path = PathBuf::from(ws);
        if path.exists() {
            return Ok(path.canonicalize()?);
        }

        // Try to look up in registry
        let registry = WorkspaceRegistry::new()
            .await
            .context("Failed to load workspace registry")?;

        if let Some(path) = registry.get(ws).await {
            return Ok(path);
        }

        anyhow::bail!(
            "Workspace '{}' not found (not a valid path or registered name)",
            ws
        );
    }

    // Default to current directory
    std::env::current_dir().context("Failed to get current directory")
}

/// Load configuration with optional config file override.
pub fn load_config(global: &GlobalOptions, workspace: &Path) -> Result<PrismConfig> {
    let mut loader = ConfigLoader::new();

    // If a config file is specified, load from its parent directory as workspace
    if let Some(ref config_path) = global.config {
        if let Some(parent) = config_path.parent() {
            return loader
                .load_local(parent)
                .context("Failed to load config file")?
                .ok_or_else(|| {
                    anyhow::anyhow!("Config file not found: {}", config_path.display())
                });
        }
    }

    // Otherwise use standard loading (global -> local merge)
    loader
        .load(workspace, None)
        .context("Failed to load configuration")
}

/// Create a backend for the resolved workspace.
pub async fn create_backend(global: &GlobalOptions) -> Result<Arc<LocalBackend>> {
    let workspace = resolve_workspace(global).await?;
    let mut config = load_config(global, &workspace)?;

    // Apply CLI overrides
    let overrides = global.to_config_overrides();
    config.apply_overrides(&overrides);

    let backend = LocalBackend::new(&config, &workspace)
        .await
        .context("Failed to create backend")?;

    Ok(Arc::new(backend))
}

/// Print a result in a consistent format.
#[allow(dead_code)]
pub fn print_result<T: std::fmt::Display>(result: T, quiet: bool) {
    if !quiet {
        println!("{}", result);
    }
}

/// Print an error message to stderr.
#[allow(dead_code)]
pub fn print_error(message: &str) {
    eprintln!("error: {}", message);
}

/// Print a warning message to stderr.
#[allow(dead_code)]
pub fn print_warning(message: &str) {
    eprintln!("warning: {}", message);
}

/// Print an info message (respects quiet flag).
pub fn print_info(message: &str, quiet: bool) {
    if !quiet {
        eprintln!("{}", message);
    }
}

/// Convert codeprysm_config's embedding settings to codeprysm_search's EmbeddingConfig.
///
/// This function translates between the configuration crate's embedding settings
/// and the search crate's embedding provider configuration.
pub fn to_search_embedding_config(config: &PrismConfig) -> SearchEmbeddingConfig {
    use codeprysm_config::EmbeddingProviderType;

    match config.embedding.provider {
        EmbeddingProviderType::Local => SearchEmbeddingConfig::local(),
        EmbeddingProviderType::AzureMl => {
            if let Some(ref azure) = config.embedding.azure_ml {
                // Resolve semantic auth: use env var name if specified
                let semantic_auth = if let Some(ref env_var) = azure.semantic_auth_key_env {
                    AzureMLAuth::ApiKeyEnv(env_var.clone())
                } else if let Some(ref env_var) = azure.auth_key_env {
                    // Legacy single key support
                    AzureMLAuth::ApiKeyEnv(env_var.clone())
                } else {
                    AzureMLAuth::ApiKeyEnv("CODEPRYSM_AZURE_ML_SEMANTIC_API_KEY".to_string())
                };

                // Resolve code auth: use env var name if specified, else None (falls back to semantic)
                let code_auth = azure
                    .code_auth_key_env
                    .as_ref()
                    .map(|env_var| AzureMLAuth::ApiKeyEnv(env_var.clone()));

                let azure_config = AzureMLConfig {
                    semantic_endpoint: azure.semantic_endpoint.clone(),
                    code_endpoint: azure.code_endpoint.clone(),
                    semantic_auth,
                    code_auth,
                    timeout_secs: azure.timeout_secs,
                    max_retries: azure.max_retries,
                };
                SearchEmbeddingConfig::azure_ml_with_config(azure_config)
            } else {
                // No settings provided, let factory read from environment
                SearchEmbeddingConfig::azure_ml()
            }
        }
        EmbeddingProviderType::Openai => {
            if let Some(ref openai) = config.embedding.openai {
                // Resolve API key from env var if specified
                let api_key = openai
                    .api_key_env
                    .as_ref()
                    .and_then(|env_var| std::env::var(env_var).ok());

                let openai_config = OpenAIConfig {
                    base_url: openai.url.clone(),
                    api_key,
                    semantic_model: openai.semantic_model.clone(),
                    code_model: openai.code_model.clone(),
                    timeout_secs: openai.timeout_secs,
                    max_retries: openai.max_retries,
                    azure_mode: openai.azure_mode,
                };
                SearchEmbeddingConfig::openai_with_config(openai_config)
            } else {
                // No settings provided, let factory read from environment
                SearchEmbeddingConfig::openai()
            }
        }
    }
}
