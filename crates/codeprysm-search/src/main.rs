//! CodePrysm Search CLI - Semantic code search indexing and querying
//!
//! Commands:
//! - `index` - Index a code graph to Qdrant for semantic search
//! - `search` - Search the indexed codebase

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use codeprysm_core::lazy::manager::LazyGraphManager;
use codeprysm_search::{GraphIndexer, HybridSearcher, QdrantConfig};

/// CodePrysm Search - Semantic code search indexing and querying
#[derive(Parser)]
#[command(name = "codeprysm-search")]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Enable verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index a code graph to Qdrant for semantic search
    Index {
        /// Path to .codeprysm directory containing the partitioned graph
        #[arg(short = 'p', long, default_value = "./.codeprysm")]
        codeprysm_dir: PathBuf,

        /// Root directory of the repository (used for relative paths)
        #[arg(short, long, default_value = ".")]
        root: PathBuf,

        /// Qdrant server URL
        #[arg(
            long,
            default_value = "http://localhost:6334",
            env = "CODEPRYSM_QDRANT_URL"
        )]
        qdrant_url: String,

        /// Repository ID for multi-tenant search (default: derived from root path)
        #[arg(long, env = "CODEPRYSM_REPO_ID")]
        repo_id: Option<String>,

        /// Force re-indexing even if collection exists
        #[arg(short, long)]
        force: bool,
    },

    /// Search the indexed codebase
    Search {
        /// Search query
        query: String,

        /// Qdrant server URL
        #[arg(
            long,
            default_value = "http://localhost:6334",
            env = "CODEPRYSM_QDRANT_URL"
        )]
        qdrant_url: String,

        /// Repository ID to search
        #[arg(long, env = "CODEPRYSM_REPO_ID")]
        repo_id: Option<String>,

        /// Root directory of the repository (used to derive repo_id if not specified)
        #[arg(short, long, default_value = ".")]
        root: PathBuf,

        /// Maximum number of results
        #[arg(short, long, default_value = "10")]
        limit: usize,

        /// Output as JSON
        #[arg(long)]
        json: bool,

        /// Search mode: "code" for identifiers, "info" for natural language, or omit for hybrid
        #[arg(short, long)]
        mode: Option<String>,
    },

    /// Show index status for a repository
    Status {
        /// Qdrant server URL
        #[arg(
            long,
            default_value = "http://localhost:6334",
            env = "CODEPRYSM_QDRANT_URL"
        )]
        qdrant_url: String,

        /// Repository ID to check
        #[arg(long, env = "CODEPRYSM_REPO_ID")]
        repo_id: Option<String>,

        /// Root directory of the repository (used to derive repo_id if not specified)
        #[arg(short, long, default_value = ".")]
        root: PathBuf,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Setup logging
    let level = if cli.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };
    let subscriber = FmtSubscriber::builder().with_max_level(level).finish();
    tracing::subscriber::set_global_default(subscriber)?;

    match cli.command {
        Commands::Index {
            codeprysm_dir,
            root,
            qdrant_url,
            repo_id,
            force,
        } => cmd_index(codeprysm_dir, root, qdrant_url, repo_id, force).await,
        Commands::Search {
            query,
            qdrant_url,
            repo_id,
            root,
            limit,
            json,
            mode,
        } => cmd_search(query, qdrant_url, repo_id, root, limit, json, mode).await,
        Commands::Status {
            qdrant_url,
            repo_id,
            root,
        } => cmd_status(qdrant_url, repo_id, root).await,
    }
}

/// Index a code graph to Qdrant
#[allow(clippy::await_holding_lock)]
async fn cmd_index(
    codeprysm_dir: PathBuf,
    root: PathBuf,
    qdrant_url: String,
    repo_id: Option<String>,
    force: bool,
) -> Result<()> {
    let total_start = Instant::now();

    // Derive repo_id from root path if not specified
    let repo_id = repo_id.unwrap_or_else(|| derive_repo_id(&root));
    info!("Indexing to Qdrant with repo_id: {}", repo_id);

    // Load graph from partitioned storage
    println!("Loading graph from {}...", codeprysm_dir.display());
    let start = Instant::now();
    let manager =
        LazyGraphManager::open(&codeprysm_dir).context("Failed to open partitioned graph")?;
    manager
        .load_all_partitions()
        .context("Failed to load partitions")?;
    let graph = manager.graph_read();
    println!(
        "  Loaded {} nodes, {} edges in {:.2}s",
        graph.node_count(),
        graph.edge_count(),
        start.elapsed().as_secs_f64()
    );

    // Configure Qdrant
    let qdrant_config = QdrantConfig {
        url: qdrant_url,
        ..Default::default()
    };

    // Check if collection exists
    if !force {
        // TODO: Add check for existing collection and skip if up-to-date
    }

    // Create indexer and index graph
    println!("\nIndexing to Qdrant (this may take a while for large codebases)...");
    let start = Instant::now();

    let mut indexer = GraphIndexer::new(qdrant_config, &repo_id, &root)
        .await
        .context("Failed to create indexer")?;
    println!(
        "  Indexer initialized in {:.2}s",
        start.elapsed().as_secs_f64()
    );

    let start = Instant::now();
    let node_count = graph.node_count();
    indexer
        .index_graph(&graph)
        .await
        .context("Failed to index graph")?;

    let indexing_time = start.elapsed();
    let total_time = total_start.elapsed();

    println!("\nIndexing complete!");
    println!("  Repository: {}", repo_id);
    println!("  Nodes indexed: {}", node_count);
    println!("  Indexing time: {:.2}s", indexing_time.as_secs_f64());
    println!("  Total time: {:.2}s", total_time.as_secs_f64());

    Ok(())
}

/// Search the indexed codebase
async fn cmd_search(
    query: String,
    qdrant_url: String,
    repo_id: Option<String>,
    root: PathBuf,
    limit: usize,
    json_output: bool,
    mode: Option<String>,
) -> Result<()> {
    // Derive repo_id from root path if not specified
    let repo_id = repo_id.unwrap_or_else(|| derive_repo_id(&root));

    // Configure Qdrant
    let qdrant_config = QdrantConfig {
        url: qdrant_url,
        ..Default::default()
    };

    // Create searcher
    let searcher = HybridSearcher::connect(qdrant_config, &repo_id)
        .await
        .context("Failed to connect to Qdrant")?;

    // Run search with mode
    let start = Instant::now();
    let results = searcher
        .search_by_mode(&query, limit, vec![], mode.as_deref())
        .await
        .context("Search failed")?;
    let search_time = start.elapsed();

    if json_output {
        // Simple JSON output
        println!("[");
        for (i, result) in results.iter().enumerate() {
            let comma = if i < results.len() - 1 { "," } else { "" };
            println!(
                r#"  {{"name": "{}", "type": "{}", "kind": "{}", "file": "{}", "line": {}, "score": {:.3}}}{}"#,
                result.name,
                result.entity_type,
                result.kind,
                result.file_path,
                result.line_range.0,
                result.combined_score,
                comma
            );
        }
        println!("]");
    } else {
        let mode_str = mode.as_deref().unwrap_or("hybrid");
        println!(
            "Search [{}]: \"{}\" ({} results in {:.0}ms)\n",
            mode_str,
            query,
            results.len(),
            search_time.as_millis()
        );

        for (i, result) in results.iter().enumerate() {
            println!(
                "{}. {} ({}/{})",
                i + 1,
                result.name,
                result.entity_type,
                result.kind
            );
            println!("   File: {}:{}", result.file_path, result.line_range.0);
            println!(
                "   Score: {:.3} (via: {})",
                result.combined_score,
                result.found_via.join(", ")
            );
            if !result.code_snippet.is_empty() {
                // Show first 100 chars of snippet
                let preview: String = result.code_snippet.chars().take(100).collect();
                let preview = preview.replace('\n', " ");
                println!("   Preview: {}...", preview);
            }
            println!();
        }
    }

    Ok(())
}

/// Show index status
async fn cmd_status(qdrant_url: String, repo_id: Option<String>, root: PathBuf) -> Result<()> {
    // Derive repo_id from root path if not specified
    let repo_id = repo_id.unwrap_or_else(|| derive_repo_id(&root));

    // Configure Qdrant
    let qdrant_config = QdrantConfig {
        url: qdrant_url.clone(),
        ..Default::default()
    };

    println!("Index Status");
    println!("============");
    println!("  Qdrant URL: {}", qdrant_url);
    println!("  Repository: {}", repo_id);

    // Try to connect and get collection info
    match HybridSearcher::connect(qdrant_config, &repo_id).await {
        Ok(searcher) => match searcher.index_status().await? {
            Some((semantic_count, code_count)) => {
                println!("\n  Status: Indexed");
                println!("  Semantic collection points: {}", semantic_count);
                println!("  Code collection points: {}", code_count);
            }
            None => {
                println!("\n  Status: Not indexed (collections don't exist)");
            }
        },
        Err(e) => {
            println!("\n  Status: Connection failed");
            println!("  Error: {}", e);
        }
    }

    Ok(())
}

/// Derive a repo_id from a path
fn derive_repo_id(path: &Path) -> String {
    path.canonicalize()
        .ok()
        .and_then(|p| p.file_name().map(|s| s.to_string_lossy().to_string()))
        .unwrap_or_else(|| "default".to_string())
}
