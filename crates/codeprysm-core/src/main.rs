//! CodePrysm Core CLI - Code graph generation and analysis
//!
//! Commands:
//! - `generate` - Build a code graph from a directory
//! - `update` - Incrementally update an existing graph
//! - `stats` - Show statistics about a graph

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use codeprysm_core::lazy::manager::LazyGraphManager;
use codeprysm_core::lazy::partitioner::GraphPartitioner;
use codeprysm_core::{BuilderConfig, GraphBuilder, PetCodeGraph};

/// CodePrysm Core - Code graph generation and analysis
#[derive(Parser)]
#[command(name = "codeprysm-core")]
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
    /// Generate a code graph from a source directory
    Generate {
        /// Source directory to analyze
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,

        /// Output directory for partitioned graph (.codeprysm directory)
        #[arg(short, long, default_value = "./.codeprysm")]
        output: PathBuf,

        /// Path to SCM query files directory
        #[arg(short, long)]
        queries: Option<PathBuf>,

        /// Skip Data nodes (parameters, locals, fields) for smaller graphs
        #[arg(long)]
        skip_data: bool,

        /// Maximum containment depth (useful for large codebases)
        #[arg(long)]
        max_depth: Option<usize>,

        /// Maximum number of files to process
        #[arg(long)]
        max_files: Option<usize>,
    },

    /// Incrementally update an existing graph
    Update {
        /// Source directory to analyze
        #[arg(short, long, default_value = ".")]
        repo: PathBuf,

        /// CodePrysm directory containing partitioned graph
        #[arg(short = 'p', long, default_value = "./.codeprysm")]
        codeprysm_dir: PathBuf,

        /// Path to SCM query files directory
        #[arg(short, long)]
        queries: Option<PathBuf>,

        /// Force full rebuild regardless of changes
        #[arg(short, long)]
        force: bool,
    },

    /// Show statistics about a graph
    Stats {
        /// CodePrysm directory containing partitioned graph
        #[arg(short = 'p', long, default_value = "./.codeprysm")]
        codeprysm_dir: PathBuf,

        /// Show detailed node type breakdown
        #[arg(long)]
        detailed: bool,

        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

fn main() -> Result<()> {
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
        Commands::Generate {
            repo,
            output,
            queries,
            skip_data,
            max_depth,
            max_files,
        } => cmd_generate(repo, output, queries, skip_data, max_depth, max_files),
        Commands::Update {
            repo,
            codeprysm_dir,
            queries,
            force,
        } => cmd_update(repo, codeprysm_dir, queries, force),
        Commands::Stats {
            codeprysm_dir,
            detailed,
            json,
        } => cmd_stats(codeprysm_dir, detailed, json),
    }
}

/// Generate a code graph from a source directory
fn cmd_generate(
    repo: PathBuf,
    output: PathBuf,
    queries: Option<PathBuf>,
    skip_data: bool,
    max_depth: Option<usize>,
    max_files: Option<usize>,
) -> Result<()> {
    let start = Instant::now();

    info!("Generating code graph for {:?}", repo);

    // Resolve queries directory
    let queries_dir = resolve_queries_dir(queries)?;
    info!("Using queries from {:?}", queries_dir);

    // Build configuration
    let config = BuilderConfig {
        skip_data_nodes: skip_data,
        max_containment_depth: max_depth,
        max_files,
        ..Default::default()
    };

    // Create builder and generate PetCodeGraph
    let mut builder = GraphBuilder::with_config(&queries_dir, config)
        .context("Failed to create graph builder")?;

    let pet_graph = builder
        .build_from_directory(&repo)
        .context("Failed to build graph")?;

    // Determine root name from repo path
    let root_name = repo
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".to_string());

    // Partition and save to output directory
    let (_, stats) = GraphPartitioner::partition_with_stats(&pet_graph, &output, Some(&root_name))
        .context("Failed to partition graph")?;

    let elapsed = start.elapsed();

    println!("\nGraph generation complete!");
    println!("  Output: {:?}", output);
    println!("  Partitions: {}", stats.partition_count);
    println!("  Nodes: {}", stats.total_nodes);
    println!("  Intra-partition edges: {}", stats.intra_partition_edges);
    println!("  Cross-partition edges: {}", stats.cross_partition_edges);
    println!("  Time: {:.2}s", elapsed.as_secs_f64());

    Ok(())
}

/// Incrementally update an existing graph
fn cmd_update(
    repo: PathBuf,
    codeprysm_dir: PathBuf,
    queries: Option<PathBuf>,
    force: bool,
) -> Result<()> {
    let start = Instant::now();

    info!("Updating code graph for {:?}", repo);

    // Resolve queries directory
    let queries_dir = resolve_queries_dir(queries)?;
    info!("Using queries from {:?}", queries_dir);

    // For now, partition-aware incremental updates are not implemented
    // Fall back to full regeneration
    if !force {
        info!(
            "Note: Partition-aware incremental updates not yet implemented, performing full rebuild"
        );
    }

    // Build configuration
    let config = BuilderConfig::default();

    // Create builder and generate PetCodeGraph
    let mut builder = GraphBuilder::with_config(&queries_dir, config)
        .context("Failed to create graph builder")?;

    let pet_graph = builder
        .build_from_directory(&repo)
        .context("Failed to build graph")?;

    // Determine root name from repo path
    let root_name = repo
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "default".to_string());

    // Partition and save
    let (_, stats) =
        GraphPartitioner::partition_with_stats(&pet_graph, &codeprysm_dir, Some(&root_name))
            .context("Failed to partition graph")?;

    let elapsed = start.elapsed();

    println!("\nUpdate complete!");
    println!("  Mode: Full rebuild");
    println!("  Partitions: {}", stats.partition_count);
    println!("  Nodes: {}", stats.total_nodes);
    println!("  Time: {:.2}s", elapsed.as_secs_f64());

    Ok(())
}

/// Show statistics about a graph
fn cmd_stats(codeprysm_dir: PathBuf, detailed: bool, json_output: bool) -> Result<()> {
    info!("Loading graph from {:?}", codeprysm_dir);

    // Open lazy graph manager
    let manager =
        LazyGraphManager::open(&codeprysm_dir).context("Failed to open partitioned graph")?;

    // Load all partitions for full stats
    manager
        .load_all_partitions()
        .context("Failed to load partitions")?;

    // Get the PetCodeGraph (acquire read lock)
    let graph = manager.graph_read();

    // Compute statistics
    let stats = compute_stats(&graph, detailed);

    if json_output {
        let json = serde_json::to_string_pretty(&stats)?;
        println!("{}", json);
    } else {
        // Print partition stats first
        let lazy_stats = manager.stats();
        println!("\nPartition Statistics");
        println!("====================");
        println!("  Partitions: {}", lazy_stats.total_partitions);
        println!("  Files tracked: {}", lazy_stats.total_files);
        println!(
            "  Cross-partition edges: {}",
            lazy_stats.cross_partition_edges
        );
        println!();

        print_stats(&stats, detailed);
    }

    Ok(())
}

/// Resolve the queries directory path
fn resolve_queries_dir(queries: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = queries {
        if path.exists() {
            return Ok(path);
        }
        anyhow::bail!("Queries directory not found: {:?}", path);
    }

    // Try common locations
    let candidates = [
        PathBuf::from("queries"),
        PathBuf::from("src/queries"),
        PathBuf::from("crates/codeprysm-core/queries"),
    ];

    for candidate in &candidates {
        if candidate.exists() {
            return Ok(candidate.clone());
        }
    }

    // Try to find from CARGO_MANIFEST_DIR environment variable
    if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
        let manifest_path = PathBuf::from(manifest_dir);
        let queries_path = manifest_path.join("queries");
        if queries_path.exists() {
            return Ok(queries_path);
        }
    }

    anyhow::bail!(
        "Could not find queries directory. Please specify with --queries flag.\n\
         Tried: {:?}",
        candidates
    )
}

/// Statistics about a code graph
#[derive(Debug, serde::Serialize)]
struct GraphStats {
    total_nodes: usize,
    total_edges: usize,
    node_types: HashMap<String, usize>,
    edge_types: HashMap<String, usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    kind_breakdown: Option<HashMap<String, HashMap<String, usize>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    file_count: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    top_files_by_entities: Option<Vec<(String, usize)>>,
}

/// Compute statistics for a graph
fn compute_stats(graph: &PetCodeGraph, detailed: bool) -> GraphStats {
    let mut node_types: HashMap<String, usize> = HashMap::new();
    let mut edge_types: HashMap<String, usize> = HashMap::new();
    let mut kind_breakdown: HashMap<String, HashMap<String, usize>> = HashMap::new();
    let mut entities_per_file: HashMap<String, usize> = HashMap::new();

    // Count node types
    for node in graph.iter_nodes() {
        let type_str = node.node_type.as_str().to_string();
        *node_types.entry(type_str.clone()).or_insert(0) += 1;

        // Track kind breakdown
        if let Some(kind) = &node.kind {
            kind_breakdown
                .entry(type_str)
                .or_default()
                .entry(kind.clone())
                .and_modify(|c| *c += 1)
                .or_insert(1);
        }

        // Track entities per file (exclude file nodes themselves)
        if !node.is_file() {
            *entities_per_file.entry(node.file.clone()).or_insert(0) += 1;
        }
    }

    // Count edge types (PetCodeGraph returns owned Edge values)
    for edge in graph.iter_edges() {
        let type_str = edge.edge_type.as_str().to_string();
        *edge_types.entry(type_str).or_insert(0) += 1;
    }

    // File nodes are Container with kind="file"
    let file_count = kind_breakdown
        .get("Container")
        .and_then(|kinds| kinds.get("file").copied());

    // Top files by entity count (if detailed)
    let top_files = if detailed {
        let mut files: Vec<_> = entities_per_file.into_iter().collect();
        files.sort_by(|a, b| b.1.cmp(&a.1));
        files.truncate(10);
        Some(files)
    } else {
        None
    };

    GraphStats {
        total_nodes: graph.node_count(),
        total_edges: graph.edge_count(),
        node_types,
        edge_types,
        kind_breakdown: if detailed { Some(kind_breakdown) } else { None },
        file_count,
        top_files_by_entities: top_files,
    }
}

/// Print statistics to stdout
fn print_stats(stats: &GraphStats, detailed: bool) {
    println!("\nGraph Statistics");
    println!("================");
    println!();
    println!("Overview:");
    println!("  Total nodes: {}", stats.total_nodes);
    println!("  Total edges: {}", stats.total_edges);
    if let Some(files) = stats.file_count {
        println!("  Files: {}", files);
    }

    println!();
    println!("Node Types:");
    let mut types: Vec<_> = stats.node_types.iter().collect();
    types.sort_by(|a, b| b.1.cmp(a.1));
    for (type_name, count) in types {
        println!("  {}: {}", type_name, count);
    }

    println!();
    println!("Edge Types:");
    let mut types: Vec<_> = stats.edge_types.iter().collect();
    types.sort_by(|a, b| b.1.cmp(a.1));
    for (type_name, count) in types {
        println!("  {}: {}", type_name, count);
    }

    if detailed {
        if let Some(ref kind_breakdown) = stats.kind_breakdown {
            println!();
            println!("Kind Breakdown:");
            for (node_type, kinds) in kind_breakdown {
                println!("  {}:", node_type);
                let mut kinds: Vec<_> = kinds.iter().collect();
                kinds.sort_by(|a, b| b.1.cmp(a.1));
                for (kind, count) in kinds {
                    println!("    {}: {}", kind, count);
                }
            }
        }

        if let Some(ref top_files) = stats.top_files_by_entities {
            println!();
            println!("Top Files by Entity Count:");
            for (file, count) in top_files {
                println!("  {} ({})", file, count);
            }
        }
    }
}
