//! Config command - View and manage configuration
//!
//! Provides commands for viewing and modifying Prism configuration:
//! - List all configuration with sources
//! - Get specific configuration values
//! - Set configuration values (local or global)

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Subcommand;
use codeprysm_config::{ConfigLoader, PrismConfig};
use serde::Serialize;

use super::resolve_workspace;
use crate::GlobalOptions;

/// Config management commands
#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// List all configuration values with their sources
    List(ListArgs),

    /// Get a specific configuration value
    Get(GetArgs),

    /// Set a configuration value
    Set(SetArgs),

    /// Show configuration file paths
    Path(PathArgs),
}

/// Arguments for the list command
#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,

    /// Show only effective values (hide sources)
    #[arg(long)]
    effective: bool,
}

/// Arguments for the get command
#[derive(clap::Args, Debug)]
pub struct GetArgs {
    /// Configuration key (e.g., "backend.qdrant.url")
    key: String,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

/// Arguments for the set command
#[derive(clap::Args, Debug)]
pub struct SetArgs {
    /// Configuration key (e.g., "backend.qdrant.url")
    key: String,

    /// Value to set
    value: String,

    /// Set in global config (~/.codeprysm/config.toml) instead of local
    #[arg(long)]
    global: bool,
}

/// Arguments for the path command
#[derive(clap::Args, Debug)]
pub struct PathArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

/// Configuration value with source information
#[derive(Debug, Clone, Serialize)]
pub struct ConfigValue {
    /// Configuration key
    pub key: String,
    /// Current value
    pub value: serde_json::Value,
    /// Source of this value (default, global, local, cli)
    pub source: String,
}

/// Configuration paths
#[derive(Debug, Clone, Serialize)]
pub struct ConfigPaths {
    /// Global config file path
    pub global: Option<PathBuf>,
    /// Local config file path
    pub local: PathBuf,
    /// Whether global config exists
    pub global_exists: bool,
    /// Whether local config exists
    pub local_exists: bool,
}

/// Execute the config command
pub async fn execute(cmd: ConfigCommand, global: GlobalOptions) -> Result<()> {
    match cmd {
        ConfigCommand::List(args) => execute_list(args, global).await,
        ConfigCommand::Get(args) => execute_get(args, global).await,
        ConfigCommand::Set(args) => execute_set(args, global).await,
        ConfigCommand::Path(args) => execute_path(args, global).await,
    }
}

async fn execute_list(args: ListArgs, global: GlobalOptions) -> Result<()> {
    let workspace_path = resolve_workspace(&global).await?;
    let mut loader = ConfigLoader::new();

    // Load configs from different sources
    let default_config = PrismConfig::default();
    let global_config = loader.load_global()?.unwrap_or_default();
    let local_config = loader.load_local(&workspace_path)?.unwrap_or_default();
    let effective = loader.load(&workspace_path, None)?;

    if args.json {
        if args.effective {
            println!("{}", serde_json::to_string_pretty(&effective)?);
        } else {
            let values = collect_config_values(&default_config, &global_config, &local_config);
            println!("{}", serde_json::to_string_pretty(&values)?);
        }
    } else {
        print_config_list(
            &default_config,
            &global_config,
            &local_config,
            &loader,
            &workspace_path,
        );
    }

    Ok(())
}

async fn execute_get(args: GetArgs, global: GlobalOptions) -> Result<()> {
    let workspace_path = resolve_workspace(&global).await?;
    let mut loader = ConfigLoader::new();
    let config = loader.load(&workspace_path, None)?;

    let value = get_config_value(&config, &args.key)
        .ok_or_else(|| anyhow::anyhow!("Unknown configuration key: {}", args.key))?;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&value)?);
    } else {
        match value {
            serde_json::Value::String(s) => println!("{}", s),
            serde_json::Value::Bool(b) => println!("{}", b),
            serde_json::Value::Number(n) => println!("{}", n),
            serde_json::Value::Null => println!("null"),
            other => println!("{}", serde_json::to_string_pretty(&other)?),
        }
    }

    Ok(())
}

async fn execute_set(args: SetArgs, global: GlobalOptions) -> Result<()> {
    let workspace_path = resolve_workspace(&global).await?;
    let mut loader = ConfigLoader::new();

    // Load existing config
    let mut config = if args.global {
        loader.load_global().ok().flatten().unwrap_or_default()
    } else {
        loader
            .load_local(&workspace_path)
            .ok()
            .flatten()
            .unwrap_or_default()
    };

    // Set the value
    set_config_value(&mut config, &args.key, &args.value)
        .context(format!("Failed to set configuration key: {}", args.key))?;

    // Save config
    if args.global {
        loader.save_global(&config)?;
        println!("Set {} = {} in global config", args.key, args.value);
    } else {
        loader.save_local(&workspace_path, &config)?;
        println!("Set {} = {} in local config", args.key, args.value);
    }

    Ok(())
}

async fn execute_path(args: PathArgs, global: GlobalOptions) -> Result<()> {
    let workspace_path = resolve_workspace(&global).await?;
    let loader = ConfigLoader::new();

    let global_path = loader.global_config_path();
    let local_path = loader.local_config_path(&workspace_path);

    let paths = ConfigPaths {
        global: global_path.clone(),
        local: local_path.clone(),
        global_exists: global_path.as_ref().map(|p| p.exists()).unwrap_or(false),
        local_exists: local_path.exists(),
    };

    if args.json {
        println!("{}", serde_json::to_string_pretty(&paths)?);
    } else {
        println!("Configuration Paths");
        println!("===================\n");

        if let Some(ref gp) = paths.global {
            let status = if paths.global_exists {
                "exists"
            } else {
                "not found"
            };
            println!("Global: {} ({})", gp.display(), status);
        } else {
            println!("Global: not available (no home directory)");
        }

        let status = if paths.local_exists {
            "exists"
        } else {
            "not found"
        };
        println!("Local:  {} ({})", paths.local.display(), status);
    }

    Ok(())
}

/// Get a configuration value by key path
fn get_config_value(config: &PrismConfig, key: &str) -> Option<serde_json::Value> {
    let json = serde_json::to_value(config).ok()?;
    let parts: Vec<&str> = key.split('.').collect();

    let mut current = &json;
    for part in parts {
        match current.get(part) {
            Some(v) => current = v,
            None => return None,
        }
    }

    Some(current.clone())
}

/// Set a configuration value by key path
fn set_config_value(config: &mut PrismConfig, key: &str, value: &str) -> Result<()> {
    match key {
        // Storage
        "storage.prism_dir" => config.storage.prism_dir = PathBuf::from(value),
        "storage.compression" => config.storage.compression = value.parse()?,
        "storage.max_partition_size_mb" => config.storage.max_partition_size_mb = value.parse()?,

        // Backend
        "backend.qdrant.url" => config.backend.qdrant.url = value.to_string(),
        "backend.qdrant.api_key" => config.backend.qdrant.api_key = Some(value.to_string()),
        "backend.qdrant.collection_prefix" => {
            config.backend.qdrant.collection_prefix = value.to_string()
        }
        "backend.qdrant.hnsw_enabled" => config.backend.qdrant.hnsw_enabled = value.parse()?,

        // Analysis
        "analysis.max_file_size_kb" => config.analysis.max_file_size_kb = value.parse()?,
        "analysis.detect_components" => config.analysis.detect_components = value.parse()?,
        "analysis.parallelism" => config.analysis.parallelism = value.parse()?,

        // Workspace
        "workspace.cross_workspace_search" => {
            config.workspace.cross_workspace_search = value.parse()?
        }

        // Logging
        "logging.level" => config.logging.level = value.to_string(),

        _ => anyhow::bail!("Unknown or read-only configuration key: {}", key),
    }

    Ok(())
}

/// Collect configuration values with source information
fn collect_config_values(
    default: &PrismConfig,
    global: &PrismConfig,
    local: &PrismConfig,
) -> Vec<ConfigValue> {
    let mut values = Vec::new();

    // Convert to JSON for comparison
    let default_json = serde_json::to_value(default).unwrap();
    let global_json = serde_json::to_value(global).unwrap();
    let local_json = serde_json::to_value(local).unwrap();

    // Flatten and collect
    flatten_config("", &local_json, &global_json, &default_json, &mut values);

    values
}

/// Recursively flatten config into key-value pairs with sources
fn flatten_config(
    prefix: &str,
    local: &serde_json::Value,
    global: &serde_json::Value,
    default: &serde_json::Value,
    values: &mut Vec<ConfigValue>,
) {
    match local {
        serde_json::Value::Object(map) => {
            for (key, value) in map {
                let new_prefix = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", prefix, key)
                };

                let global_val = global.get(key).unwrap_or(&serde_json::Value::Null);
                let default_val = default.get(key).unwrap_or(&serde_json::Value::Null);

                flatten_config(&new_prefix, value, global_val, default_val, values);
            }
        }
        _ => {
            // Determine source
            let source = if local != default && local != global {
                "local"
            } else if global != default {
                "global"
            } else {
                "default"
            };

            values.push(ConfigValue {
                key: prefix.to_string(),
                value: local.clone(),
                source: source.to_string(),
            });
        }
    }
}

/// Print configuration in a human-readable format
fn print_config_list(
    default: &PrismConfig,
    global: &PrismConfig,
    local: &PrismConfig,
    loader: &ConfigLoader,
    workspace: &std::path::Path,
) {
    println!("Prism Configuration");
    println!("===================\n");

    // Show paths
    if let Some(gp) = loader.global_config_path() {
        let status = if gp.exists() { "" } else { " (not found)" };
        println!("Global config: {}{}", gp.display(), status);
    }
    let lp = loader.local_config_path(workspace);
    let status = if lp.exists() { "" } else { " (not found)" };
    println!("Local config:  {}{}\n", lp.display(), status);

    // Print sections
    println!("[storage]");
    print_value(
        "prism_dir",
        &local.storage.prism_dir,
        &global.storage.prism_dir,
        &default.storage.prism_dir,
    );
    print_value(
        "compression",
        &local.storage.compression,
        &global.storage.compression,
        &default.storage.compression,
    );
    print_value(
        "max_partition_size_mb",
        &local.storage.max_partition_size_mb,
        &global.storage.max_partition_size_mb,
        &default.storage.max_partition_size_mb,
    );

    println!("\n[backend.qdrant]");
    print_value(
        "url",
        &local.backend.qdrant.url,
        &global.backend.qdrant.url,
        &default.backend.qdrant.url,
    );
    print_value(
        "collection_prefix",
        &local.backend.qdrant.collection_prefix,
        &global.backend.qdrant.collection_prefix,
        &default.backend.qdrant.collection_prefix,
    );
    print_value(
        "hnsw_enabled",
        &local.backend.qdrant.hnsw_enabled,
        &global.backend.qdrant.hnsw_enabled,
        &default.backend.qdrant.hnsw_enabled,
    );

    println!("\n[analysis]");
    print_value(
        "max_file_size_kb",
        &local.analysis.max_file_size_kb,
        &global.analysis.max_file_size_kb,
        &default.analysis.max_file_size_kb,
    );
    print_value(
        "detect_components",
        &local.analysis.detect_components,
        &global.analysis.detect_components,
        &default.analysis.detect_components,
    );
    print_value(
        "parallelism",
        &local.analysis.parallelism,
        &global.analysis.parallelism,
        &default.analysis.parallelism,
    );

    println!("\n[workspace]");
    print_value(
        "cross_workspace_search",
        &local.workspace.cross_workspace_search,
        &global.workspace.cross_workspace_search,
        &default.workspace.cross_workspace_search,
    );

    println!("\n[logging]");
    print_value(
        "level",
        &local.logging.level,
        &global.logging.level,
        &default.logging.level,
    );
}

/// Print a configuration value with its source
fn print_value<T: std::fmt::Debug + PartialEq>(key: &str, local: &T, global: &T, default: &T) {
    let source = if local != default && local != global {
        " (local)"
    } else if global != default {
        " (global)"
    } else {
        ""
    };

    println!("  {} = {:?}{}", key, local, source);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_config_value() {
        let config = PrismConfig::default();

        let url = get_config_value(&config, "backend.qdrant.url");
        assert!(url.is_some());
        assert_eq!(url.unwrap(), "http://localhost:6334");

        let invalid = get_config_value(&config, "nonexistent.key");
        assert!(invalid.is_none());
    }

    #[test]
    fn test_set_config_value() {
        let mut config = PrismConfig::default();

        set_config_value(&mut config, "backend.qdrant.url", "http://custom:6334").unwrap();
        assert_eq!(config.backend.qdrant.url, "http://custom:6334");

        set_config_value(&mut config, "logging.level", "debug").unwrap();
        assert_eq!(config.logging.level, "debug");

        set_config_value(&mut config, "storage.compression", "true").unwrap();
        assert!(config.storage.compression);
    }

    #[test]
    fn test_set_config_value_invalid() {
        let mut config = PrismConfig::default();

        // Boolean parse error
        let result = set_config_value(&mut config, "storage.compression", "invalid");
        assert!(result.is_err());

        // Unknown key
        let result = set_config_value(&mut config, "unknown.key", "value");
        assert!(result.is_err());
    }

    #[test]
    fn test_config_paths_serialization() {
        let paths = ConfigPaths {
            global: Some(PathBuf::from("/home/user/.codeprysm/config.toml")),
            local: PathBuf::from("/project/.codeprysm/config.toml"),
            global_exists: true,
            local_exists: false,
        };

        let json = serde_json::to_string(&paths).unwrap();
        assert!(json.contains("\"global_exists\":true"));
        assert!(json.contains("\"local_exists\":false"));
    }

    #[test]
    fn test_config_value_serialization() {
        let value = ConfigValue {
            key: "backend.qdrant.url".to_string(),
            value: serde_json::json!("http://localhost:6334"),
            source: "default".to_string(),
        };

        let json = serde_json::to_string(&value).unwrap();
        assert!(json.contains("\"key\":\"backend.qdrant.url\""));
        assert!(json.contains("\"source\":\"default\""));
    }
}
