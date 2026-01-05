//! Initialize command - Create a new CodePrysm workspace

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
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

    // Check if already initialized
    if manifest_path.exists() && !args.force {
        anyhow::bail!(
            "Workspace already initialized at {}. Use --force to reinitialize.",
            prism_dir.display()
        );
    }

    print_info(
        &format!(
            "Initializing CodePrysm workspace at {}",
            workspace_path.display()
        ),
        quiet,
    );

    // Create .codeprysm directory
    if !prism_dir.exists() {
        std::fs::create_dir_all(&prism_dir).context("Failed to create .codeprysm directory")?;
        print_info(&format!("Created {}", prism_dir.display()), quiet);
    }

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

    // Derive root name
    let root_name = workspace_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "workspace".to_string());

    // Partition and save the graph
    let pb = spinner("Saving graph to partitioned storage...", quiet);

    let (_, stats) = GraphPartitioner::partition_with_stats(&graph, &prism_dir, Some(&root_name))
        .context("Failed to partition graph")?;

    finish_spinner(
        pb,
        &format!(
            "Saved graph ({} nodes, {} partitions)",
            stats.total_nodes, stats.partition_count
        ),
    );

    // Index the graph if not skipped
    // Use the in-memory graph directly instead of reloading from disk via backend
    if !no_index {
        let pb = spinner("Indexing graph for semantic search...", quiet);

        // Create indexer with the configured embedding provider
        // This avoids the overhead of reloading all partitions from disk
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
                match indexer.index_graph(&graph).await {
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
