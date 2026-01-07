//! MCP server command
//!
//! Starts the CodePrysm MCP server for AI assistant integration.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Args;
use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::lazy::partitioner::GraphPartitioner;
use codeprysm_mcp::{PrismServer, ServerConfig};
use rmcp::{transport::stdio, ServiceExt};
use tokio::signal;
use tracing::{info, warn, Level};
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::FmtSubscriber;

use crate::GlobalOptions;

/// Start the MCP server for AI assistant integration
#[derive(Args, Debug)]
pub struct McpArgs {
    /// Path to workspace root directory (default: current directory or --workspace)
    #[arg(long)]
    root: Option<PathBuf>,

    /// Path to .codeprysm artifacts directory (default: {root}/.codeprysm)
    #[arg(long)]
    codeprysm_dir: Option<PathBuf>,

    /// Repository/workspace ID for multi-tenant search
    #[arg(long)]
    repo_id: Option<String>,

    /// Path to custom tree-sitter queries directory (default: uses embedded queries)
    #[arg(long)]
    queries: Option<PathBuf>,

    /// Skip automatic graph generation if .codeprysm directory doesn't exist
    #[arg(long)]
    no_auto_generate: bool,

    /// Log file path (default: stderr)
    #[arg(long)]
    log_file: Option<PathBuf>,

    /// Enable debug logging
    #[arg(long)]
    debug: bool,
}

/// Execute the MCP server command
pub async fn execute(args: McpArgs, global: GlobalOptions) -> Result<()> {
    // Set up logging (must be stderr - stdout is for MCP JSON-RPC protocol)
    let log_level = if args.debug || global.verbose {
        Level::DEBUG
    } else if global.quiet {
        Level::ERROR
    } else {
        Level::INFO
    };

    // Set up tracing - use try_init() to gracefully handle the case where a global
    // subscriber is already set (e.g., when launched by Claude Code)
    if let Some(ref log_file) = args.log_file {
        let file = std::fs::File::create(log_file)
            .with_context(|| format!("Failed to create log file: {}", log_file.display()))?;
        let subscriber = FmtSubscriber::builder()
            .with_max_level(log_level)
            .with_writer(file)
            .with_ansi(false)
            .finish();
        if subscriber.try_init().is_err() {
            // A global subscriber is already set (e.g., by Claude Code)
            // Logs will go to the existing subscriber's destination instead of our log file
            warn!(
                "Note: Using existing tracing subscriber (--log-file {} ignored)",
                log_file.display()
            );
        }
    } else {
        // Log to stderr (stdout is for MCP protocol)
        let subscriber = FmtSubscriber::builder()
            .with_max_level(log_level)
            .with_writer(std::io::stderr)
            .with_ansi(false)
            .finish();
        // Silently use existing subscriber if one is already set
        let _ = subscriber.try_init();
    }

    // Resolve paths
    let root_path = args
        .root
        .or_else(|| global.workspace.as_ref().map(PathBuf::from))
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

    let root_path = root_path
        .canonicalize()
        .unwrap_or_else(|_| root_path.clone());

    let codeprysm_dir = args
        .codeprysm_dir
        .unwrap_or_else(|| root_path.join(".codeprysm"));

    // Ensure codeprysm directory exists
    if !codeprysm_dir.exists() {
        std::fs::create_dir_all(&codeprysm_dir).with_context(|| {
            format!(
                "Failed to create codeprysm directory: {}",
                codeprysm_dir.display()
            )
        })?;
        info!("Created codeprysm directory: {}", codeprysm_dir.display());
    }

    let manifest_path = codeprysm_dir.join("manifest.json");

    info!("Starting CodePrysm MCP Server");
    info!("  Root: {}", root_path.display());
    info!("  CodePrysm dir: {}", codeprysm_dir.display());
    info!("  Qdrant: {}", global.qdrant_url);

    // Validate root path
    if !root_path.exists() {
        anyhow::bail!("Root path does not exist: {}", root_path.display());
    }

    // Auto-generate graph if manifest doesn't exist
    if !manifest_path.exists() {
        if args.no_auto_generate {
            anyhow::bail!(
                "Graph not found: {}. Remove --no-auto-generate to auto-generate.",
                manifest_path.display()
            );
        }

        info!("Graph not found, generating...");
        generate_graph(&root_path, &codeprysm_dir, args.queries.as_deref())?;
        info!("Graph generated successfully");
    }

    // Build server config
    let mut config = ServerConfig::new(&root_path)
        .with_qdrant_url(&global.qdrant_url)
        .with_codeprysm_dir(&codeprysm_dir);

    if let Some(repo_id) = args.repo_id {
        config = config.with_repo_id(repo_id);
    }

    if let Some(ref queries) = args.queries {
        config = config.with_queries_path(queries);
    }

    // Create and run the server
    let server = PrismServer::new(config)
        .await
        .context("Failed to create MCP server")?;

    info!("Server initialized, starting MCP protocol over stdio");

    // Clone for shutdown handler
    let server_for_shutdown = server.clone();

    // Start serving
    let service = server
        .serve(stdio())
        .await
        .context("Failed to start MCP service")?;

    // Wait for shutdown or service termination
    tokio::select! {
        result = service.waiting() => {
            if let Err(e) = result {
                info!("Service ended with error: {}", e);
            } else {
                info!("Service ended normally");
            }
        }
        _ = shutdown_signal() => {
            info!("Shutdown signal received");
            server_for_shutdown.shutdown();
        }
    }

    info!("Server shutdown complete");
    Ok(())
}

/// Generate a code graph from a workspace root directory
fn generate_graph(
    root_path: &Path,
    codeprysm_dir: &Path,
    queries_dir: Option<&Path>,
) -> Result<()> {
    // Ensure codeprysm directory exists
    if !codeprysm_dir.exists() {
        std::fs::create_dir_all(codeprysm_dir).with_context(|| {
            format!(
                "Failed to create codeprysm directory: {}",
                codeprysm_dir.display()
            )
        })?;
    }

    // Create builder with embedded or custom queries
    let config = BuilderConfig::default();
    let mut builder = match queries_dir {
        Some(dir) => {
            info!("Using custom queries directory: {}", dir.display());
            GraphBuilder::with_config(dir, config).with_context(|| {
                format!(
                    "Failed to create graph builder with queries from {}",
                    dir.display()
                )
            })?
        }
        None => {
            info!("Using embedded queries (compiled into binary)");
            GraphBuilder::with_embedded_queries(config)
        }
    };

    info!("Building workspace graph from: {}", root_path.display());
    let (graph, roots) = builder
        .build_from_workspace(root_path)
        .context("Failed to build graph")?;

    info!("Discovered {} code root(s):", roots.len());
    for root in &roots {
        info!(
            "  - {} ({}) at {}",
            root.name,
            if root.is_git() { "git" } else { "code" },
            root.relative_path
        );
    }

    // Determine root name for partitioner
    let root_name = root_path
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".to_string());

    // Save graph to partitioned storage
    let (_, stats) =
        GraphPartitioner::partition_with_stats(&graph, codeprysm_dir, Some(&root_name))
            .context("Failed to partition graph")?;

    info!(
        "Graph saved: {} nodes, {} partitions, {} cross-partition edges",
        stats.total_nodes, stats.partition_count, stats.cross_partition_edges
    );

    Ok(())
}

/// Wait for shutdown signal (SIGTERM or SIGINT)
async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }
}
