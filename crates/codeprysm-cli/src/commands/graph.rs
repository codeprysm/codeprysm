//! Graph command - Graph query and navigation

use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use codeprysm_backend::Backend;

use super::create_backend;
use crate::GlobalOptions;

/// Arguments for the graph command
#[derive(Args, Debug)]
pub struct GraphArgs {
    #[command(subcommand)]
    command: GraphSubcommand,
}

#[derive(Subcommand, Debug)]
pub enum GraphSubcommand {
    /// Show graph statistics
    Stats,

    /// Show node information
    Node {
        /// Node ID to show
        node_id: String,
    },

    /// Find nodes by pattern
    Find {
        /// Name pattern (supports * wildcards)
        pattern: String,

        /// Filter by node type (Container, Callable, Data)
        #[arg(long, short = 't')]
        node_type: Option<String>,

        /// Maximum results
        #[arg(long, short = 'n', default_value = "20")]
        limit: usize,
    },

    /// Show edges connected to a node
    Edges {
        /// Node ID to query
        node_id: String,

        /// Edge type filter (Contains, Uses, Defines, DependsOn)
        #[arg(long, short = 'e')]
        edge_type: Option<String>,

        /// Direction: outgoing, incoming, or both
        #[arg(long, short = 'd', default_value = "both")]
        direction: EdgeDirection,
    },

    /// Show connected nodes
    Connected {
        /// Node ID to query
        node_id: String,

        /// Edge type filter
        #[arg(long, short = 'e')]
        edge_type: Option<String>,

        /// Direction: outgoing, incoming, or both
        #[arg(long, short = 'd', default_value = "outgoing")]
        direction: EdgeDirection,
    },

    /// Read code for a node
    Code {
        /// Node ID to read
        node_id: String,

        /// Lines of context before/after
        #[arg(long, default_value = "0")]
        context: usize,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum EdgeDirection {
    Outgoing,
    Incoming,
    Both,
}

impl EdgeDirection {
    fn as_str(&self) -> &'static str {
        match self {
            EdgeDirection::Outgoing => "outgoing",
            EdgeDirection::Incoming => "incoming",
            EdgeDirection::Both => "both",
        }
    }
}

/// Execute the graph command
pub async fn execute(args: GraphArgs, global: GlobalOptions) -> Result<()> {
    let backend = create_backend(&global).await?;

    match args.command {
        GraphSubcommand::Stats => execute_stats(&*backend, &global).await,
        GraphSubcommand::Node { node_id } => execute_node(&*backend, &node_id, &global).await,
        GraphSubcommand::Find {
            pattern,
            node_type,
            limit,
        } => execute_find(&*backend, &pattern, node_type.as_deref(), limit, &global).await,
        GraphSubcommand::Edges {
            node_id,
            edge_type,
            direction,
        } => {
            execute_edges(
                &*backend,
                &node_id,
                edge_type.as_deref(),
                direction,
                &global,
            )
            .await
        }
        GraphSubcommand::Connected {
            node_id,
            edge_type,
            direction,
        } => {
            execute_connected(
                &*backend,
                &node_id,
                edge_type.as_deref(),
                direction,
                &global,
            )
            .await
        }
        GraphSubcommand::Code { node_id, context } => {
            execute_code(&*backend, &node_id, context, &global).await
        }
    }
}

async fn execute_stats<B: Backend>(backend: &B, global: &GlobalOptions) -> Result<()> {
    let stats = backend.graph_stats().await.context("Failed to get stats")?;

    if !global.quiet {
        println!("Graph Statistics");
        println!("================");
        println!("Total nodes: {}", stats.node_count);
        println!("Total edges: {}", stats.edge_count);
        println!("Files: {}", stats.file_count);
        println!("Components: {}", stats.component_count);

        println!("\nNodes by type:");
        for (node_type, count) in &stats.nodes_by_type {
            println!("  {}: {}", node_type, count);
        }

        println!("\nEdges by type:");
        for (edge_type, count) in &stats.edges_by_type {
            println!("  {}: {}", edge_type, count);
        }
    } else {
        // Machine-readable
        println!("{}", serde_json::to_string(&stats)?);
    }

    Ok(())
}

async fn execute_node<B: Backend>(
    backend: &B,
    node_id: &str,
    global: &GlobalOptions,
) -> Result<()> {
    let node = backend
        .get_node(node_id)
        .await
        .context("Failed to get node")?;

    if global.quiet {
        println!("{}", serde_json::to_string(&node)?);
    } else {
        println!("Node: {}", node.id);
        println!("  Name: {}", node.name);
        println!("  Type: {}", node.node_type);
        if let Some(ref kind) = node.kind {
            println!("  Kind: {}", kind);
        }
        if let Some(ref path) = node.file_path {
            println!("  File: {}", path);
        }
        if let (Some(start), Some(end)) = (node.start_line, node.end_line) {
            println!("  Lines: {}-{}", start, end);
        }
        if !node.metadata.is_empty() {
            println!("  Metadata:");
            for (key, value) in &node.metadata {
                println!("    {}: {}", key, value);
            }
        }
    }

    Ok(())
}

async fn execute_find<B: Backend>(
    backend: &B,
    pattern: &str,
    node_type: Option<&str>,
    limit: usize,
    global: &GlobalOptions,
) -> Result<()> {
    let nodes = backend
        .find_nodes(pattern, node_type, limit)
        .await
        .context("Failed to find nodes")?;

    if nodes.is_empty() {
        if !global.quiet {
            println!("No nodes found matching pattern: {}", pattern);
        }
        return Ok(());
    }

    if global.quiet {
        for node in &nodes {
            println!("{}", node.id);
        }
    } else {
        println!("Found {} nodes matching '{}':\n", nodes.len(), pattern);
        for node in &nodes {
            println!(
                "  {} ({}) - {}:{}",
                node.name,
                node.kind.as_deref().unwrap_or(&node.node_type),
                node.file_path.as_deref().unwrap_or("unknown"),
                node.start_line.unwrap_or(0)
            );
        }
    }

    Ok(())
}

async fn execute_edges<B: Backend>(
    backend: &B,
    node_id: &str,
    edge_type: Option<&str>,
    direction: EdgeDirection,
    global: &GlobalOptions,
) -> Result<()> {
    let edges = backend
        .get_edges(node_id, edge_type, direction.as_str())
        .await
        .context("Failed to get edges")?;

    if edges.is_empty() {
        if !global.quiet {
            println!("No edges found for node: {}", node_id);
        }
        return Ok(());
    }

    if global.quiet {
        println!("{}", serde_json::to_string(&edges)?);
    } else {
        println!("Edges for '{}':\n", node_id);
        for edge in &edges {
            let arrow = if edge.from_id == node_id {
                format!("--[{}]-->", edge.edge_type)
            } else {
                format!("<--[{}]--", edge.edge_type)
            };

            let other = if edge.from_id == node_id {
                &edge.to_id
            } else {
                &edge.from_id
            };

            println!("  {} {}", arrow, other);

            if !edge.metadata.is_empty() && global.verbose {
                for (key, value) in &edge.metadata {
                    println!("      {}: {}", key, value);
                }
            }
        }
    }

    Ok(())
}

async fn execute_connected<B: Backend>(
    backend: &B,
    node_id: &str,
    edge_type: Option<&str>,
    direction: EdgeDirection,
    global: &GlobalOptions,
) -> Result<()> {
    let nodes = backend
        .get_connected_nodes(node_id, edge_type, direction.as_str())
        .await
        .context("Failed to get connected nodes")?;

    if nodes.is_empty() {
        if !global.quiet {
            println!("No connected nodes found for: {}", node_id);
        }
        return Ok(());
    }

    if global.quiet {
        for node in &nodes {
            println!("{}", node.id);
        }
    } else {
        let dir_str = match direction {
            EdgeDirection::Outgoing => "outgoing",
            EdgeDirection::Incoming => "incoming",
            EdgeDirection::Both => "connected",
        };
        println!("{} {} nodes for '{}':\n", nodes.len(), dir_str, node_id);

        for node in &nodes {
            println!(
                "  {} ({}) - {}",
                node.name,
                node.kind.as_deref().unwrap_or(&node.node_type),
                node.file_path.as_deref().unwrap_or("unknown")
            );
        }
    }

    Ok(())
}

async fn execute_code<B: Backend>(
    backend: &B,
    node_id: &str,
    context: usize,
    _global: &GlobalOptions,
) -> Result<()> {
    let code = backend
        .read_code(node_id, context)
        .await
        .context("Failed to read code")?;

    println!("{}", code);
    Ok(())
}
