//! Initialize command - Create a new CodePrysm workspace

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::lazy::manager::LazyGraphManager;
use codeprysm_core::lazy::partitioner::GraphPartitioner;
use codeprysm_search::{GraphIndexer, QdrantConfig};
use tracing::info;

use super::{load_config, print_info, to_search_embedding_config};
use crate::progress::{finish_spinner, finish_spinner_warn, spinner};
use crate::GlobalOptions;

/// Arguments for the init command
#[derive(Args, Debug)]
pub struct InitArgs {
    /// Path to initialize (defaults to current directory)
    #[arg(default_value = ".")]
    path: PathBuf,

    /// Force re-initialization even if .codeprysm already exists
    #[arg(long, short = 'f')]
    force: bool,

    /// Skip indexing after graph generation
    #[arg(long)]
    no_index: bool,

    /// Path to custom SCM queries directory
    #[arg(long)]
    queries: Option<PathBuf>,

    /// Skip manifest/component detection
    #[arg(long)]
    no_components: bool,

    /// CI/CD mode (equivalent to --quiet --no-index)
    #[arg(long)]
    ci: bool,

    /// Embedding batch size for API calls (default: 200)
    #[arg(long, default_value = "200")]
    embedding_batch_size: usize,

    /// Use streaming (memory-bounded) indexing mode
    ///
    /// This processes the graph partition-by-partition to limit memory usage.
    /// Recommended for large repositories (>10,000 nodes) or memory-constrained environments.
    /// When set to "auto", streaming is enabled if the graph has >10,000 nodes.
    #[arg(long, default_value = "auto")]
    streaming: StreamingMode,

    /// Maximum memory budget for indexing (e.g., "8GB", "4096MB", "512M")
    ///
    /// Controls the memory budget for partition caching during streaming indexing.
    /// Only applies when streaming mode is enabled.
    /// Default: 512MB
    #[arg(long, value_parser = parse_memory_size)]
    max_index_memory: Option<usize>,
}

/// Parse a human-readable memory size string into bytes.
///
/// Accepts formats like: "8GB", "4096MB", "512M", "1G", "1073741824"
fn parse_memory_size(s: &str) -> Result<usize, String> {
    let s = s.trim().to_uppercase();

    // Try parsing as plain number (bytes)
    if let Ok(bytes) = s.parse::<usize>() {
        return Ok(bytes);
    }

    // Find where the numeric part ends and the unit begins
    let (num_str, unit) = {
        let mut split_idx = 0;
        for (i, c) in s.char_indices() {
            if !c.is_ascii_digit() && c != '.' {
                split_idx = i;
                break;
            }
        }
        if split_idx == 0 {
            return Err(format!("Invalid memory size: {}", s));
        }
        (&s[..split_idx], s[split_idx..].trim())
    };

    let num: f64 = num_str
        .parse()
        .map_err(|_| format!("Invalid number in memory size: {}", num_str))?;

    let multiplier: usize = match unit {
        "B" | "" => 1,
        "K" | "KB" | "KIB" => 1024,
        "M" | "MB" | "MIB" => 1024 * 1024,
        "G" | "GB" | "GIB" => 1024 * 1024 * 1024,
        "T" | "TB" | "TIB" => 1024 * 1024 * 1024 * 1024,
        _ => return Err(format!("Unknown memory unit: {}", unit)),
    };

    Ok((num * multiplier as f64) as usize)
}

/// Streaming mode for indexing
#[derive(Debug, Clone, clap::ValueEnum, Default)]
pub enum StreamingMode {
    /// Enable streaming mode
    On,
    /// Disable streaming mode (load full graph into memory)
    Off,
    /// Auto-detect based on graph size (>10K nodes enables streaming)
    #[default]
    Auto,
}

/// Execute the init command
pub async fn execute(args: InitArgs, global: GlobalOptions) -> Result<()> {
    // Apply --ci mode: equivalent to --quiet --no-index
    let quiet = global.quiet || args.ci;
    let no_index = args.no_index || args.ci;

    let workspace_path = if args.path.is_absolute() {
        args.path.clone()
    } else {
        std::env::current_dir()?.join(&args.path)
    };

    let workspace_path = workspace_path
        .canonicalize()
        .context("Failed to resolve workspace path")?;

    let mut config = load_config(&global, &workspace_path)?;

    // Apply CLI overrides (e.g., --embedding-provider)
    let overrides = global.to_config_overrides();
    config.apply_overrides(&overrides);

    let prism_dir = config.prism_dir(&workspace_path);
    let manifest_path = prism_dir.join("manifest.json");
    let checkpoint_path = prism_dir.join("index_checkpoint.json");

    // Check if already initialized
    // Allow resume if checkpoint exists (from interrupted indexing)
    let resume_from_checkpoint = manifest_path.exists() && checkpoint_path.exists() && !args.force;

    if manifest_path.exists() && !args.force && !checkpoint_path.exists() {
        anyhow::bail!(
            "Workspace already initialized at {}. Use --force to reinitialize.",
            prism_dir.display()
        );
    }

    if resume_from_checkpoint {
        print_info(
            &format!(
                "Resuming interrupted indexing at {}",
                workspace_path.display()
            ),
            quiet,
        );
    } else {
        print_info(
            &format!(
                "Initializing CodePrysm workspace at {}",
                workspace_path.display()
            ),
            quiet,
        );
    }

    // Create .codeprysm directory
    if !prism_dir.exists() {
        std::fs::create_dir_all(&prism_dir).context("Failed to create .codeprysm directory")?;
        print_info(&format!("Created {}", prism_dir.display()), quiet);
    }

    // Derive root name (needed for both fresh init and resume)
    let root_name = workspace_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".to_string());

    // Track graph and stats for non-resume path
    let mut graph_for_indexing: Option<codeprysm_core::PetCodeGraph> = None;
    let mut node_count_for_streaming: usize = 0;

    // Skip graph generation if resuming from checkpoint
    if !resume_from_checkpoint {
        // Build configuration
        let builder_config = BuilderConfig {
            skip_data_nodes: false,
            max_containment_depth: None,
            max_files: None,
            exclude_patterns: config.analysis.exclude_patterns.clone(),
        };

        // Create builder
        let mut builder = match &args.queries {
            Some(queries_dir) => {
                info!("Using custom queries from: {}", queries_dir.display());
                GraphBuilder::with_config(queries_dir, builder_config)
                    .context("Failed to create graph builder with custom queries")?
            }
            None => {
                info!("Using embedded queries");
                GraphBuilder::with_embedded_queries(builder_config)
            }
        };

        // Build the graph
        let pb = spinner("Building code graph...", quiet);

        let (graph, roots) = builder
            .build_from_workspace(&workspace_path)
            .context("Failed to build code graph")?;

        finish_spinner(
            pb,
            &format!(
                "Built code graph ({} code root{})",
                roots.len(),
                if roots.len() == 1 { "" } else { "s" }
            ),
        );

        if !quiet && global.verbose {
            println!("  Discovered roots:");
            for root in &roots {
                println!(
                    "    - {} ({}) at {}",
                    root.name,
                    if root.is_git() { "git" } else { "code" },
                    root.relative_path
                );
            }
        }

        // Partition and save the graph
        let pb = spinner("Saving graph to partitioned storage...", quiet);

        let (_, stats) =
            GraphPartitioner::partition_with_stats(&graph, &prism_dir, Some(&root_name))
                .context("Failed to partition graph")?;

        finish_spinner(
            pb,
            &format!(
                "Saved graph ({} nodes, {} partitions)",
                stats.total_nodes, stats.partition_count
            ),
        );

        node_count_for_streaming = stats.total_nodes;
        graph_for_indexing = Some(graph);
    } else {
        // Resuming - get node count from existing manifest for streaming decision
        if let Ok(manager) = LazyGraphManager::open(&prism_dir) {
            node_count_for_streaming = manager.get_total_indexable_node_count().unwrap_or(10_001);
            // Default to streaming if count fails
        }
        print_info(
            "Skipping graph generation (using existing partitions)",
            quiet,
        );
    }

    // Index the graph if not skipped
    if !no_index {
        // Determine whether to use streaming mode
        // Resume always uses streaming (that's where checkpoint support is)
        let use_streaming = if resume_from_checkpoint {
            true
        } else {
            match args.streaming {
                StreamingMode::On => true,
                StreamingMode::Off => false,
                StreamingMode::Auto => {
                    // Auto-detect: use streaming for large graphs (>10K nodes)
                    let threshold = 10_000;
                    if node_count_for_streaming > threshold {
                        print_info(
                            &format!(
                                "Large graph detected ({} nodes > {} threshold), using streaming mode",
                                node_count_for_streaming, threshold
                            ),
                            quiet,
                        );
                        true
                    } else {
                        false
                    }
                }
            }
        };

        let mode_label = if use_streaming {
            "Indexing graph for semantic search (streaming mode)..."
        } else {
            "Indexing graph for semantic search..."
        };
        let pb = spinner(mode_label, quiet);

        // Create indexer with the configured embedding provider
        let qdrant_config = QdrantConfig::with_url(&global.qdrant_url);
        let embedding_config = to_search_embedding_config(&config);

        match GraphIndexer::from_config(
            qdrant_config,
            &embedding_config,
            &root_name,
            &workspace_path,
        )
        .await
        {
            Ok(indexer) => {
                let mut indexer = indexer.with_embedding_batch_size(args.embedding_batch_size);

                let (index_result, pb) = if use_streaming {
                    // Streaming mode: reload from disk and process partition-by-partition
                    // This bounds memory usage regardless of graph size
                    let manager_result = if let Some(budget) = args.max_index_memory {
                        let budget_mb = budget / (1024 * 1024);
                        print_info(&format!("Using memory budget: {}MB", budget_mb), quiet);
                        LazyGraphManager::open_with_memory_budget(&prism_dir, Some(budget))
                    } else {
                        LazyGraphManager::open(&prism_dir)
                    };

                    match manager_result {
                        Ok(manager) => {
                            // Use resumable indexing with checkpoint support
                            // The --force flag clears any existing checkpoint
                            (
                                indexer
                                    .index_graph_lazy_resumable(&manager, &prism_dir, args.force)
                                    .await,
                                pb,
                            )
                        }
                        Err(e) => {
                            // In resume mode, we can't fall back to in-memory (no graph loaded)
                            if resume_from_checkpoint {
                                finish_spinner_warn(pb, "Failed to open graph for streaming");
                                anyhow::bail!(
                                    "Cannot resume: failed to open partitions: {}. Use --force to reinitialize.",
                                    e
                                );
                            }
                            finish_spinner_warn(pb, "Failed to open graph for streaming");
                            if !quiet {
                                eprintln!("  Warning: {}", e);
                                eprintln!("  Falling back to in-memory indexing...");
                            }
                            // Fallback to in-memory indexing - create new spinner
                            let pb = spinner("Indexing graph for semantic search...", quiet);
                            let graph = graph_for_indexing.as_ref().unwrap();
                            (indexer.index_graph(graph).await, pb)
                        }
                    }
                } else {
                    // Standard mode: use the in-memory graph directly
                    let graph = graph_for_indexing.as_ref().unwrap();
                    (indexer.index_graph(graph).await, pb)
                };

                match index_result {
                    Ok(stats) => {
                        finish_spinner(
                            pb,
                            &format!("Indexed {} entities for search", stats.total_indexed),
                        );
                    }
                    Err(e) => {
                        finish_spinner_warn(pb, "Indexing failed");
                        if !quiet {
                            eprintln!("  Warning: {}", e);
                            eprintln!("  You can index later with: codeprysm update --reindex");
                        }
                    }
                }
            }
            Err(e) => {
                finish_spinner_warn(pb, "Indexing skipped (Qdrant may not be running)");
                if !quiet {
                    eprintln!("  Warning: {}", e);
                    eprintln!("  You can index later with: codeprysm update --reindex");
                }
            }
        }
    }

    // Create local config file if it doesn't exist
    let local_config_path = prism_dir.join("config.toml");
    if !local_config_path.exists() {
        let default_local = r#"# Prism local configuration
# This file overrides global settings for this workspace

[analysis]
# exclude_patterns = ["**/generated/**"]

[storage]
# graph_dir = ".codeprysm"
"#;
        std::fs::write(&local_config_path, default_local)
            .context("Failed to write local config")?;
        print_info(&format!("Created {}", local_config_path.display()), quiet);
    }

    if !quiet {
        println!("\nWorkspace initialized successfully!");
        println!("\nNext steps:");
        println!("  codeprysm search \"your query\"    - Search the codebase");
        println!("  codeprysm components list         - List detected components");
        println!("  codeprysm status                  - Check workspace status");
    }

    Ok(())
}
