//! Update command - Incremental graph update

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Args;
use codeprysm_backend::Backend;
use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::lazy::partitioner::GraphPartitioner;
use tracing::info;

use super::{create_backend, load_config, resolve_workspace};
use crate::progress::{finish_spinner, spinner};
use crate::GlobalOptions;

/// Arguments for the update command
#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Force full rebuild instead of incremental update
    #[arg(long, short = 'f')]
    force: bool,

    /// Reindex the graph for search after update
    #[arg(long)]
    reindex: bool,

    /// Only reindex without updating the graph
    #[arg(long)]
    index_only: bool,

    /// Path to custom SCM queries directory
    #[arg(long)]
    queries: Option<PathBuf>,
}

/// Execute the update command
pub async fn execute(args: UpdateArgs, global: GlobalOptions) -> Result<()> {
    let workspace_path = resolve_workspace(&global).await?;
    let config = load_config(&global, &workspace_path)?;
    let prism_dir = config.prism_dir(&workspace_path);
    let manifest_path = prism_dir.join("manifest.json");

    // Check if workspace is initialized
    if !manifest_path.exists() {
        anyhow::bail!(
            "Workspace not initialized. Run 'codeprysm init' first.\n  Path: {}",
            workspace_path.display()
        );
    }

    // Index-only mode
    if args.index_only {
        let pb = spinner("Reindexing graph for semantic search...", global.quiet);

        let backend = create_backend(&global).await?;
        let count = backend.index(true).await.context("Failed to index graph")?;

        finish_spinner(pb, &format!("Indexed {} entities", count));

        return Ok(());
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
                .context("Failed to create graph builder")?
        }
        None => {
            info!("Using embedded queries");
            GraphBuilder::with_embedded_queries(builder_config)
        }
    };

    // For now, we do a full rebuild. Incremental updates would use the merkle tree.
    // TODO: Implement true incremental updates using codeprysm_core::incremental
    let msg = if args.force {
        "Rebuilding code graph..."
    } else {
        "Updating code graph..."
    };

    let pb = spinner(msg, global.quiet);

    let (graph, roots) = builder
        .build_from_workspace(&workspace_path)
        .context("Failed to build code graph")?;

    finish_spinner(
        pb,
        &format!(
            "Built code graph ({} root{})",
            roots.len(),
            if roots.len() == 1 { "" } else { "s" }
        ),
    );

    if !global.quiet && global.verbose {
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

    // Save the graph
    let pb = spinner("Saving graph...", global.quiet);

    let (_, stats) = GraphPartitioner::partition_with_stats(&graph, &prism_dir, Some(&root_name))
        .context("Failed to save graph")?;

    finish_spinner(
        pb,
        &format!(
            "Saved graph ({} nodes, {} partitions)",
            stats.total_nodes, stats.partition_count
        ),
    );

    // Reindex if requested
    if args.reindex {
        let pb = spinner("Reindexing for semantic search...", global.quiet);

        let backend = create_backend(&global).await?;

        // Sync the graph first
        backend.sync().await.context("Failed to sync graph")?;

        let count = backend.index(true).await.context("Failed to index graph")?;

        finish_spinner(pb, &format!("Indexed {} entities", count));
    }

    if !global.quiet {
        println!("\n✓ Update complete!");
    }

    Ok(())
}
