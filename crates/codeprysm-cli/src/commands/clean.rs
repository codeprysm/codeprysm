//! Clean command - Remove CodePrysm data for a workspace
//!
//! Provides cleanup functionality for:
//! - Local .codeprysm/ directory and all graph data
//! - Backend index data (Qdrant points for this workspace)

use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result};
use clap::Args;
use codeprysm_search::{schema::collections, QdrantConfig, QdrantStore};
use serde::Serialize;

use super::{load_config, resolve_workspace};
use crate::GlobalOptions;

/// Arguments for the clean command
#[derive(Args, Debug)]
pub struct CleanArgs {
    /// Skip confirmation prompt
    #[arg(long, short = 'f')]
    force: bool,

    /// Show what would be deleted without actually deleting
    #[arg(long, short = 'n')]
    dry_run: bool,

    /// Only clean local data (.codeprysm/ directory), skip backend cleanup
    #[arg(long)]
    local_only: bool,

    /// Only clean backend data (Qdrant), skip local cleanup
    #[arg(long)]
    backend_only: bool,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

/// Result of the clean operation
#[derive(Debug, Clone, Serialize)]
pub struct CleanResult {
    /// Whether this was a dry run
    pub dry_run: bool,
    /// Local cleanup result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub local: Option<LocalCleanResult>,
    /// Backend cleanup result
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backend: Option<BackendCleanResult>,
}

/// Result of local cleanup
#[derive(Debug, Clone, Serialize)]
pub struct LocalCleanResult {
    /// Path that was/would be deleted
    pub path: String,
    /// Whether cleanup succeeded
    pub success: bool,
    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Size of deleted data in bytes (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

/// Result of backend cleanup
#[derive(Debug, Clone, Serialize)]
pub struct BackendCleanResult {
    /// Repository ID that was cleaned
    pub repo_id: String,
    /// Whether cleanup succeeded
    pub success: bool,
    /// Error message if failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Number of points deleted from semantic collection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub semantic_points: Option<u64>,
    /// Number of points deleted from code collection
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code_points: Option<u64>,
}

/// Execute the clean command
pub async fn execute(args: CleanArgs, global: GlobalOptions) -> Result<()> {
    let workspace_path = resolve_workspace(&global).await?;
    let config = load_config(&global, &workspace_path)?;
    let prism_dir = config.prism_dir(&workspace_path);

    // Determine what to clean
    let clean_local = !args.backend_only;
    let clean_backend = !args.local_only;

    // Gather information about what will be cleaned
    let local_exists = prism_dir.exists();
    let local_size = if local_exists {
        dir_size(&prism_dir).ok()
    } else {
        None
    };

    // Generate repo_id (same logic as LocalBackend)
    let repo_id = generate_repo_id(&workspace_path);

    // Confirm unless --force or --dry-run
    if !args.force && !args.dry_run {
        print_cleanup_preview(
            clean_local,
            clean_backend,
            &prism_dir,
            local_exists,
            local_size,
            &repo_id,
            &global,
        );

        print!("\nProceed with cleanup? [y/N] ");
        io::stdout().flush()?;

        let mut input = String::new();
        io::stdin().read_line(&mut input)?;

        if !input.trim().eq_ignore_ascii_case("y") {
            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&serde_json::json!({"cancelled": true}))?
                );
            } else {
                println!("Cleanup cancelled.");
            }
            return Ok(());
        }
    }

    let mut result = CleanResult {
        dry_run: args.dry_run,
        local: None,
        backend: None,
    };

    // Clean local data
    if clean_local {
        result.local = Some(clean_local_data(&prism_dir, args.dry_run, &global).await);
    }

    // Clean backend data
    if clean_backend {
        result.backend =
            Some(clean_backend_data(&repo_id, &global.qdrant_url, args.dry_run, &global).await);
    }

    // Output results
    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print_clean_result(&result, &global);
    }

    // Check for errors
    let has_errors = result.local.as_ref().map(|l| !l.success).unwrap_or(false)
        || result.backend.as_ref().map(|b| !b.success).unwrap_or(false);

    if has_errors {
        std::process::exit(1);
    }

    Ok(())
}

fn generate_repo_id(workspace_path: &Path) -> String {
    // Same logic as in LocalBackend::new
    workspace_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown")
        .to_string()
}

fn print_cleanup_preview(
    clean_local: bool,
    clean_backend: bool,
    prism_dir: &Path,
    local_exists: bool,
    local_size: Option<u64>,
    repo_id: &str,
    global: &GlobalOptions,
) {
    println!("Prism Clean Preview");
    println!("===================\n");

    if clean_local {
        println!("Local data:");
        if local_exists {
            println!("  Path: {}", prism_dir.display());
            if let Some(size) = local_size {
                println!("  Size: {}", format_size(size));
            }
            println!("  Action: Will be deleted");
        } else {
            println!("  Path: {}", prism_dir.display());
            println!("  Status: Does not exist (nothing to delete)");
        }
    }

    if clean_backend {
        println!("\nBackend data:");
        println!("  Qdrant URL: {}", global.qdrant_url);
        println!("  Repo ID: {}", repo_id);
        println!("  Collections: semantic_search, code_search");
        println!("  Action: Points for this repo will be deleted");
    }
}

async fn clean_local_data(
    prism_dir: &Path,
    dry_run: bool,
    global: &GlobalOptions,
) -> LocalCleanResult {
    if !prism_dir.exists() {
        return LocalCleanResult {
            path: prism_dir.display().to_string(),
            success: true,
            error: Some("Directory does not exist".to_string()),
            size_bytes: None,
        };
    }

    let size = dir_size(prism_dir).ok();

    if dry_run {
        if !global.quiet {
            println!(
                "Would delete: {} ({})",
                prism_dir.display(),
                size.map(format_size)
                    .unwrap_or_else(|| "unknown size".to_string())
            );
        }
        return LocalCleanResult {
            path: prism_dir.display().to_string(),
            success: true,
            error: None,
            size_bytes: size,
        };
    }

    match std::fs::remove_dir_all(prism_dir) {
        Ok(()) => {
            if !global.quiet {
                println!(
                    "Deleted: {} ({})",
                    prism_dir.display(),
                    size.map(format_size)
                        .unwrap_or_else(|| "unknown size".to_string())
                );
            }
            LocalCleanResult {
                path: prism_dir.display().to_string(),
                success: true,
                error: None,
                size_bytes: size,
            }
        }
        Err(e) => LocalCleanResult {
            path: prism_dir.display().to_string(),
            success: false,
            error: Some(e.to_string()),
            size_bytes: size,
        },
    }
}

async fn clean_backend_data(
    repo_id: &str,
    qdrant_url: &str,
    dry_run: bool,
    global: &GlobalOptions,
) -> BackendCleanResult {
    // Connect to Qdrant
    let config = QdrantConfig::with_url(qdrant_url);
    let client = match QdrantStore::connect(config, repo_id).await {
        Ok(c) => c,
        Err(e) => {
            return BackendCleanResult {
                repo_id: repo_id.to_string(),
                success: false,
                error: Some(format!("Cannot connect to Qdrant: {}", e)),
                semantic_points: None,
                code_points: None,
            };
        }
    };

    // Get point counts before deletion
    let semantic_count = get_repo_point_count(&client, collections::SEMANTIC).await;
    let code_count = get_repo_point_count(&client, collections::CODE).await;

    if dry_run {
        if !global.quiet {
            println!(
                "Would delete {} points from semantic_search",
                semantic_count.unwrap_or(0)
            );
            println!(
                "Would delete {} points from code_search",
                code_count.unwrap_or(0)
            );
        }
        return BackendCleanResult {
            repo_id: repo_id.to_string(),
            success: true,
            error: None,
            semantic_points: semantic_count,
            code_points: code_count,
        };
    }

    // Delete points from both collections
    let mut errors = Vec::new();

    if let Err(e) = client.delete_repo_points(collections::SEMANTIC).await {
        errors.push(format!("semantic_search: {}", e));
    } else if !global.quiet {
        println!(
            "Deleted {} points from semantic_search",
            semantic_count.unwrap_or(0)
        );
    }

    if let Err(e) = client.delete_repo_points(collections::CODE).await {
        errors.push(format!("code_search: {}", e));
    } else if !global.quiet {
        println!(
            "Deleted {} points from code_search",
            code_count.unwrap_or(0)
        );
    }

    if errors.is_empty() {
        BackendCleanResult {
            repo_id: repo_id.to_string(),
            success: true,
            error: None,
            semantic_points: semantic_count,
            code_points: code_count,
        }
    } else {
        BackendCleanResult {
            repo_id: repo_id.to_string(),
            success: false,
            error: Some(errors.join("; ")),
            semantic_points: semantic_count,
            code_points: code_count,
        }
    }
}

async fn get_repo_point_count(client: &QdrantStore, collection: &str) -> Option<u64> {
    // Try to get count via collection info - this is an approximation
    // since we can't easily filter by repo_id without a scroll
    match client.collection_info(collection).await {
        Ok(Some(info)) => Some(info.points_count.unwrap_or(0)),
        _ => None,
    }
}

fn print_clean_result(result: &CleanResult, global: &GlobalOptions) {
    if global.quiet {
        return;
    }

    if result.dry_run {
        println!("\nDry run complete - no changes made.");
        return;
    }

    println!("\nClean Summary");
    println!("=============");

    if let Some(ref local) = result.local {
        if local.success {
            println!(
                "Local:   Cleaned ({})",
                local
                    .size_bytes
                    .map(format_size)
                    .unwrap_or_else(|| "unknown size".to_string())
            );
        } else if let Some(ref err) = local.error {
            if err == "Directory does not exist" {
                println!("Local:   Already clean");
            } else {
                println!("Local:   Failed - {}", err);
            }
        }
    }

    if let Some(ref backend) = result.backend {
        if backend.success {
            let total = backend.semantic_points.unwrap_or(0) + backend.code_points.unwrap_or(0);
            println!("Backend: Cleaned ({} points)", total);
        } else if let Some(ref err) = backend.error {
            println!("Backend: Failed - {}", err);
        }
    }
}

/// Calculate the total size of a directory
fn dir_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;

    if path.is_file() {
        return Ok(std::fs::metadata(path)
            .context("Failed to get file metadata")?
            .len());
    }

    for entry in walkdir::WalkDir::new(path)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if entry.file_type().is_file() {
            total += entry.metadata().map(|m| m.len()).unwrap_or(0);
        }
    }

    Ok(total)
}

/// Format a size in bytes as a human-readable string
fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 bytes");
        assert_eq!(format_size(100), "100 bytes");
        assert_eq!(format_size(1023), "1023 bytes");
    }

    #[test]
    fn test_format_size_kb() {
        assert_eq!(format_size(1024), "1.00 KB");
        assert_eq!(format_size(1536), "1.50 KB");
        assert_eq!(format_size(10240), "10.00 KB");
    }

    #[test]
    fn test_format_size_mb() {
        assert_eq!(format_size(1024 * 1024), "1.00 MB");
        assert_eq!(format_size(1024 * 1024 * 5), "5.00 MB");
    }

    #[test]
    fn test_format_size_gb() {
        assert_eq!(format_size(1024 * 1024 * 1024), "1.00 GB");
        assert_eq!(format_size(1024 * 1024 * 1024 * 2), "2.00 GB");
    }

    #[test]
    fn test_generate_repo_id() {
        let path = Path::new("/home/user/my-project");
        assert_eq!(generate_repo_id(path), "my-project");
    }

    #[test]
    fn test_generate_repo_id_root() {
        let path = Path::new("/");
        // Root path should not panic
        let _ = generate_repo_id(path);
    }

    #[test]
    fn test_local_clean_result_serialization() {
        let result = LocalCleanResult {
            path: "/tmp/test".to_string(),
            success: true,
            error: None,
            size_bytes: Some(1024),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"success\":true"));
        assert!(json.contains("\"size_bytes\":1024"));
        // error should be skipped when None
        assert!(!json.contains("\"error\""));
    }

    #[test]
    fn test_backend_clean_result_serialization() {
        let result = BackendCleanResult {
            repo_id: "test-repo".to_string(),
            success: false,
            error: Some("Connection failed".to_string()),
            semantic_points: None,
            code_points: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"success\":false"));
        assert!(json.contains("\"error\":\"Connection failed\""));
    }

    #[test]
    fn test_clean_result_dry_run() {
        let result = CleanResult {
            dry_run: true,
            local: Some(LocalCleanResult {
                path: "/test".to_string(),
                success: true,
                error: None,
                size_bytes: Some(100),
            }),
            backend: None,
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"dry_run\":true"));
        assert!(!json.contains("\"backend\""));
    }
}
