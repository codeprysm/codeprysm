//! Status command - Show workspace status and configuration

use std::collections::HashMap;

use anyhow::Result;
use clap::Args;
use codeprysm_backend::Backend;
use serde::Deserialize;

use super::{create_backend, load_config, resolve_workspace};
use crate::GlobalOptions;

/// Manifest structure for reading schema version
#[derive(Debug, Deserialize)]
struct ManifestInfo {
    schema_version: String,
    #[serde(default)]
    partitions: HashMap<String, String>,
    #[serde(default)]
    roots: HashMap<String, serde_json::Value>,
}

/// Arguments for the status command
#[derive(Args, Debug)]
pub struct StatusArgs {
    /// Show configuration details
    #[arg(long = "show-config")]
    show_config: bool,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

/// Execute the status command
pub async fn execute(args: StatusArgs, global: GlobalOptions) -> Result<()> {
    let workspace_path = resolve_workspace(&global).await?;
    let config = load_config(&global, &workspace_path)?;
    let prism_dir = config.prism_dir(&workspace_path);
    let manifest_path = prism_dir.join("manifest.json");

    let is_initialized = manifest_path.exists();

    // Read manifest info if available
    let manifest_info: Option<ManifestInfo> = if manifest_path.exists() {
        std::fs::read_to_string(&manifest_path)
            .ok()
            .and_then(|content| serde_json::from_str(&content).ok())
    } else {
        None
    };

    if args.json {
        let mut status = serde_json::json!({
            "workspace_path": workspace_path,
            "prism_dir": prism_dir,
            "initialized": is_initialized,
            "qdrant_url": config.backend.qdrant.url,
        });

        // Add schema info
        if let Some(ref info) = manifest_info {
            status["schema"] = serde_json::json!({
                "manifest_version": info.schema_version,
                "partition_count": info.partitions.len(),
                "root_count": info.roots.len(),
            });
        }

        if is_initialized {
            // Try to get graph stats
            if let Ok(backend) = create_backend(&global).await {
                if let Ok(stats) = backend.graph_stats().await {
                    status["graph"] = serde_json::json!({
                        "node_count": stats.node_count,
                        "edge_count": stats.edge_count,
                        "file_count": stats.file_count,
                        "component_count": stats.component_count,
                    });
                }

                // Check index status
                if let Ok(index_status) = backend.index_status().await {
                    status["index"] = serde_json::json!({
                        "exists": index_status.exists,
                        "entity_count": index_status.entity_count,
                        "semantic_count": index_status.semantic_count,
                        "code_count": index_status.code_count,
                    });
                }

                // Check health
                if let Ok(healthy) = backend.health_check().await {
                    status["healthy"] = serde_json::json!(healthy);
                }

                // Check provider status
                if let Ok(provider_status) = backend.check_provider().await {
                    let model_status: codeprysm_backend::ModelStatus = provider_status.into();
                    status["models"] = serde_json::json!({
                        "semantic_available": model_status.semantic_available,
                        "code_available": model_status.code_available,
                        "semantic_loaded": model_status.semantic_loaded,
                        "code_loaded": model_status.code_loaded,
                        "device": model_status.device,
                        "semantic_error": model_status.semantic_error,
                        "code_error": model_status.code_error,
                    });
                }
            }
        }

        if args.show_config {
            status["config"] = serde_json::json!({
                "storage": {
                    "prism_dir": config.storage.prism_dir,
                },
                "backend": {
                    "qdrant_url": config.backend.qdrant.url,
                },
                "analysis": {
                    "exclude_patterns": config.analysis.exclude_patterns,
                },
            });
        }

        println!("{}", serde_json::to_string_pretty(&status)?);
        return Ok(());
    }

    // Human-readable output
    println!("CodePrysm Workspace Status");
    println!("======================\n");

    println!("Workspace: {}", workspace_path.display());
    println!("CodePrysm dir: {}", prism_dir.display());
    println!(
        "Status:    {}",
        if is_initialized {
            "Initialized"
        } else {
            "Not initialized"
        }
    );

    // Display schema info
    if let Some(ref info) = manifest_info {
        println!(
            "Schema:    v{} ({} partitions, {} roots)",
            info.schema_version,
            info.partitions.len(),
            info.roots.len()
        );
    }

    if !is_initialized {
        println!("\nWorkspace not initialized. Run 'codeprysm init' to get started.");
        return Ok(());
    }

    // Get backend for stats
    match create_backend(&global).await {
        Ok(backend) => {
            // Graph stats
            println!("\nGraph:");
            match backend.graph_stats().await {
                Ok(stats) => {
                    println!("  Nodes:      {}", stats.node_count);
                    println!("  Edges:      {}", stats.edge_count);
                    println!("  Files:      {}", stats.file_count);
                    println!("  Components: {}", stats.component_count);

                    if global.verbose {
                        println!("\n  Nodes by type:");
                        for (node_type, count) in &stats.nodes_by_type {
                            println!("    {}: {}", node_type, count);
                        }
                        println!("\n  Edges by type:");
                        for (edge_type, count) in &stats.edges_by_type {
                            println!("    {}: {}", edge_type, count);
                        }
                    }
                }
                Err(e) => {
                    println!("  Error loading graph: {}", e);
                }
            }

            // Index status
            println!("\nSearch Index:");
            match backend.index_status().await {
                Ok(status) => {
                    if status.exists {
                        println!("  Status:    Indexed");
                        println!("  Entities:  {}", status.entity_count);
                        println!(
                            "  Semantic:  {} | Code: {}",
                            status.semantic_count, status.code_count
                        );
                        if let Some(ref version) = status.version {
                            println!("  Version:   {}", version);
                        }
                    } else {
                        println!("  Status:    Not indexed");
                        println!("\n  Run 'codeprysm update --reindex' to create search index.");
                    }
                }
                Err(e) => {
                    println!("  Status:    Error");
                    println!("  Error:     {}", e);
                    println!(
                        "\n  Qdrant may not be running. Start with: docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant"
                    );
                }
            }

            // Health check
            println!("\nHealth:");
            match backend.health_check().await {
                Ok(true) => println!("  All systems operational"),
                Ok(false) => println!("  Some services unavailable"),
                Err(e) => println!("  Health check failed: {}", e),
            }

            // Provider/Model status
            println!("\nEmbedding Models:");
            match backend.check_provider().await {
                Ok(provider_status) => {
                    let status: codeprysm_backend::ModelStatus = provider_status.into();
                    let semantic_status = if status.semantic_available {
                        if status.semantic_loaded {
                            "Loaded"
                        } else {
                            "Available"
                        }
                    } else {
                        "Unavailable"
                    };
                    let code_status = if status.code_available {
                        if status.code_loaded {
                            "Loaded"
                        } else {
                            "Available"
                        }
                    } else {
                        "Unavailable"
                    };

                    println!("  Device:    {}", status.device);
                    println!("  Semantic:  {}", semantic_status);
                    println!("  Code:      {}", code_status);

                    if let Some(ref err) = status.semantic_error {
                        println!("  Semantic error: {}", err);
                    }
                    if let Some(ref err) = status.code_error {
                        println!("  Code error: {}", err);
                    }

                    if status.all_available() {
                        println!("  Status:    Ready for search");
                    } else {
                        println!("  Status:    Models unavailable - search may fail");
                        println!(
                            "\n  Models will be downloaded on first search from HuggingFace Hub."
                        );
                    }
                }
                Err(e) => {
                    println!("  Error:     {}", e);
                }
            }
        }
        Err(e) => {
            println!("\nError creating backend: {}", e);
        }
    }

    if args.show_config {
        println!("\nConfiguration:");
        println!("  Qdrant URL: {}", config.backend.qdrant.url);
        println!("  CodePrysm dir:  {}", config.storage.prism_dir.display());
        if !config.analysis.exclude_patterns.is_empty() {
            println!("  Excludes:");
            for pattern in &config.analysis.exclude_patterns {
                println!("    - {}", pattern);
            }
        }
    }

    Ok(())
}
