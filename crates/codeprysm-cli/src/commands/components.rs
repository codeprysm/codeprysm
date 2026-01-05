//! Components command - Component management and analysis
//!
//! Provides commands for listing components, analyzing dependencies,
//! and determining affected components from file changes.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use codeprysm_backend::Backend;

use super::create_backend;
use crate::GlobalOptions;

/// Component management commands
#[derive(Subcommand, Debug)]
pub enum ComponentsCommand {
    /// List all detected components
    List(ListArgs),

    /// Show dependencies for a component
    Deps(DepsArgs),

    /// Show components affected by file changes
    Affected(AffectedArgs),

    /// Visualize component dependency graph
    Graph(GraphVisualizationArgs),
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Filter by component type (e.g., rust, npm, python)
    #[arg(long, short = 't')]
    component_type: Option<String>,

    /// Show only workspace root components
    #[arg(long)]
    roots_only: bool,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct DepsArgs {
    /// Component name or path pattern
    name: String,

    /// Show transitive dependencies (full dependency tree)
    #[arg(long, short = 'a')]
    all: bool,

    /// Show reverse dependencies (what depends on this)
    #[arg(long, short = 'r')]
    reverse: bool,

    /// Maximum depth for transitive dependencies
    #[arg(long, short = 'd', default_value = "10")]
    depth: usize,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct AffectedArgs {
    /// Paths to check for affected components (files or directories)
    #[arg(required = true)]
    paths: Vec<PathBuf>,

    /// Compare against a git ref (e.g., HEAD, main)
    #[arg(long)]
    base: Option<String>,

    /// Include transitive dependents (components that depend on affected)
    #[arg(long, short = 'a')]
    all: bool,

    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct GraphVisualizationArgs {
    /// Output format: dot, mermaid, or ascii
    #[arg(long, short = 'f', default_value = "ascii")]
    format: GraphFormat,

    /// Include only these components (comma-separated)
    #[arg(long)]
    include: Option<String>,

    /// Exclude these components (comma-separated)
    #[arg(long)]
    exclude: Option<String>,

    /// Maximum depth from root components
    #[arg(long, short = 'd')]
    depth: Option<usize>,
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum GraphFormat {
    /// Graphviz DOT format
    Dot,
    /// Mermaid diagram format
    Mermaid,
    /// ASCII art tree
    Ascii,
}

/// Execute component commands
pub async fn execute(cmd: ComponentsCommand, global: GlobalOptions) -> Result<()> {
    match cmd {
        ComponentsCommand::List(args) => execute_list(args, global).await,
        ComponentsCommand::Deps(args) => execute_deps(args, global).await,
        ComponentsCommand::Affected(args) => execute_affected(args, global).await,
        ComponentsCommand::Graph(args) => execute_graph(args, global).await,
    }
}

async fn execute_list(args: ListArgs, global: GlobalOptions) -> Result<()> {
    let backend = create_backend(&global).await?;

    // Find all component nodes
    let nodes = backend
        .find_nodes("*", Some("Container"), 1000)
        .await
        .context("Failed to query components")?;

    let components: Vec<_> = nodes
        .into_iter()
        .filter(|n| n.kind.as_deref() == Some("component"))
        .filter(|n| {
            if args.roots_only {
                n.metadata
                    .get("is_workspace_root")
                    .map(|v| v == "true")
                    .unwrap_or(false)
            } else {
                true
            }
        })
        .filter(|n| {
            if let Some(ref comp_type) = args.component_type {
                // Filter by inferred component type from manifest_path
                if let Some(manifest) = n.metadata.get("manifest_path") {
                    manifest.contains(comp_type)
                } else {
                    false
                }
            } else {
                true
            }
        })
        .collect();

    if args.json {
        let json_output: Vec<_> = components
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "path": c.file_path,
                    "is_workspace_root": c.metadata.get("is_workspace_root").map(|v| v == "true").unwrap_or(false),
                    "manifest_path": c.metadata.get("manifest_path"),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        if components.is_empty() {
            if !global.quiet {
                println!("No components found.");
                println!(
                    "\nHint: Components are detected from manifest files (Cargo.toml, package.json, etc.)"
                );
                println!(
                    "Run 'codeprysm update' to refresh the graph if you've added manifest files."
                );
            }
            return Ok(());
        }

        if !global.quiet {
            println!("Found {} components:\n", components.len());
        }

        for comp in &components {
            let root_marker = if comp
                .metadata
                .get("is_workspace_root")
                .map(|v| v == "true")
                .unwrap_or(false)
            {
                " [root]"
            } else {
                ""
            };

            println!(
                "  {}{} - {}",
                comp.name,
                root_marker,
                comp.file_path.as_deref().unwrap_or("unknown")
            );
        }
    }

    Ok(())
}

async fn execute_deps(args: DepsArgs, global: GlobalOptions) -> Result<()> {
    let backend = create_backend(&global).await?;

    // Find the component by name pattern
    let nodes = backend
        .find_nodes(&args.name, Some("Container"), 10)
        .await
        .context("Failed to find component")?;

    let component = nodes
        .into_iter()
        .find(|n| n.kind.as_deref() == Some("component"))
        .ok_or_else(|| anyhow::anyhow!("Component '{}' not found", args.name))?;

    let direction = if args.reverse { "incoming" } else { "outgoing" };

    if args.all {
        // Transitive dependencies
        let mut visited = HashSet::new();
        let mut deps = Vec::new();

        collect_transitive_deps(
            &*backend,
            &component.id,
            direction,
            args.depth,
            0,
            &mut visited,
            &mut deps,
        )
        .await?;

        if args.json {
            println!("{}", serde_json::to_string_pretty(&deps)?);
        } else {
            let dep_type = if args.reverse {
                "reverse dependencies (what depends on this)"
            } else {
                "dependencies"
            };

            if !global.quiet {
                println!("{} for '{}':\n", dep_type, component.name);
            }

            if deps.is_empty() {
                println!("  (none)");
            } else {
                print_dependency_tree(&deps, 0);
            }
        }
    } else {
        // Direct dependencies only
        let edges = backend
            .get_edges(&component.id, Some("DependsOn"), direction)
            .await
            .context("Failed to get dependencies")?;

        if args.json {
            let json_output: Vec<_> = edges
                .iter()
                .map(|e| {
                    let dep_id = if args.reverse { &e.from_id } else { &e.to_id };
                    serde_json::json!({
                        "id": dep_id,
                        "version_spec": e.metadata.get("version_spec"),
                        "is_dev": e.metadata.get("is_dev_dependency").map(|v| v == "true").unwrap_or(false),
                    })
                })
                .collect();
            println!("{}", serde_json::to_string_pretty(&json_output)?);
        } else {
            let dep_type = if args.reverse {
                "Dependents of"
            } else {
                "Dependencies of"
            };

            if !global.quiet {
                println!("{} '{}':\n", dep_type, component.name);
            }

            if edges.is_empty() {
                println!("  (none)");
            } else {
                for edge in &edges {
                    let dep_id = if args.reverse {
                        &edge.from_id
                    } else {
                        &edge.to_id
                    };
                    let version = edge
                        .metadata
                        .get("version_spec")
                        .map(|v| format!(" ({})", v))
                        .unwrap_or_default();
                    let dev_marker = if edge
                        .metadata
                        .get("is_dev_dependency")
                        .map(|v| v == "true")
                        .unwrap_or(false)
                    {
                        " [dev]"
                    } else {
                        ""
                    };

                    // Extract just the component name from the node ID
                    let name = dep_id.rsplit(':').next().unwrap_or(dep_id);
                    println!("  {}{}{}", name, version, dev_marker);
                }
            }
        }
    }

    Ok(())
}

/// Dependency info for tree building
#[derive(Debug, serde::Serialize)]
struct DepInfo {
    name: String,
    id: String,
    depth: usize,
    version_spec: Option<String>,
    is_dev: bool,
    children: Vec<DepInfo>,
}

fn collect_transitive_deps<'a, B: Backend + 'a>(
    backend: &'a B,
    node_id: &'a str,
    direction: &'a str,
    max_depth: usize,
    current_depth: usize,
    visited: &'a mut HashSet<String>,
    deps: &'a mut Vec<DepInfo>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
    Box::pin(async move {
        if current_depth >= max_depth || visited.contains(node_id) {
            return Ok(());
        }
        visited.insert(node_id.to_string());

        let edges = backend
            .get_edges(node_id, Some("DependsOn"), direction)
            .await?;

        for edge in edges {
            let dep_id = if direction == "incoming" {
                &edge.from_id
            } else {
                &edge.to_id
            };

            let name = dep_id.rsplit(':').next().unwrap_or(dep_id).to_string();

            let mut dep = DepInfo {
                name,
                id: dep_id.clone(),
                depth: current_depth + 1,
                version_spec: edge.metadata.get("version_spec").cloned(),
                is_dev: edge
                    .metadata
                    .get("is_dev_dependency")
                    .map(|v| v == "true")
                    .unwrap_or(false),
                children: Vec::new(),
            };

            // Recurse for transitive dependencies
            let mut children = Vec::new();
            collect_transitive_deps(
                backend,
                dep_id,
                direction,
                max_depth,
                current_depth + 1,
                visited,
                &mut children,
            )
            .await?;
            dep.children = children;

            deps.push(dep);
        }

        Ok(())
    })
}

fn print_dependency_tree(deps: &[DepInfo], indent: usize) {
    for dep in deps {
        let prefix = "  ".repeat(indent);
        let version = dep
            .version_spec
            .as_ref()
            .map(|v| format!(" ({})", v))
            .unwrap_or_default();
        let dev = if dep.is_dev { " [dev]" } else { "" };

        println!("{}{}{}{}", prefix, dep.name, version, dev);

        if !dep.children.is_empty() {
            print_dependency_tree(&dep.children, indent + 1);
        }
    }
}

async fn execute_affected(args: AffectedArgs, global: GlobalOptions) -> Result<()> {
    let backend = create_backend(&global).await?;
    let workspace = super::resolve_workspace(&global).await?;

    // Determine which files changed
    let changed_files: Vec<PathBuf> = if let Some(ref base) = args.base {
        // Use git diff to find changed files
        let output = std::process::Command::new("git")
            .args(["diff", "--name-only", base])
            .current_dir(&workspace)
            .output()
            .context("Failed to run git diff")?;

        if !output.status.success() {
            anyhow::bail!(
                "git diff failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }

        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|l| workspace.join(l))
            .collect()
    } else {
        // Use the provided paths directly
        args.paths
            .iter()
            .map(|p| {
                if p.is_absolute() {
                    p.clone()
                } else {
                    workspace.join(p)
                }
            })
            .collect()
    };

    if changed_files.is_empty() {
        if !global.quiet {
            println!("No changed files found.");
        }
        return Ok(());
    }

    // Find components that contain the changed files
    let mut affected_components = HashSet::new();

    // Get all components
    let nodes = backend
        .find_nodes("*", Some("Container"), 1000)
        .await
        .context("Failed to query components")?;

    let components: Vec<_> = nodes
        .into_iter()
        .filter(|n| n.kind.as_deref() == Some("component"))
        .collect();

    // For each changed file, find which component contains it
    for file in &changed_files {
        let relative = file
            .strip_prefix(&workspace)
            .unwrap_or(file)
            .to_string_lossy();

        // Find the component whose path is a prefix of this file
        for comp in &components {
            if let Some(ref comp_path) = comp.file_path {
                if relative.starts_with(comp_path.trim_end_matches('/')) {
                    affected_components.insert(comp.id.clone());
                }
            }
        }
    }

    // If --all, also find transitive dependents
    if args.all {
        let mut to_check: Vec<_> = affected_components.iter().cloned().collect();
        let mut checked = HashSet::new();

        while let Some(comp_id) = to_check.pop() {
            if checked.contains(&comp_id) {
                continue;
            }
            checked.insert(comp_id.clone());

            let edges = backend
                .get_edges(&comp_id, Some("DependsOn"), "incoming")
                .await?;

            for edge in edges {
                if !affected_components.contains(&edge.from_id) {
                    affected_components.insert(edge.from_id.clone());
                    to_check.push(edge.from_id);
                }
            }
        }
    }

    // Map IDs back to component info
    let affected: Vec<_> = components
        .iter()
        .filter(|c| affected_components.contains(&c.id))
        .collect();

    if args.json {
        let json_output: Vec<_> = affected
            .iter()
            .map(|c| {
                serde_json::json!({
                    "name": c.name,
                    "path": c.file_path,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json_output)?);
    } else {
        if !global.quiet {
            println!(
                "Affected components ({} from {} changed files):\n",
                affected.len(),
                changed_files.len()
            );
        }

        if affected.is_empty() {
            println!("  (none)");
        } else {
            for comp in &affected {
                println!(
                    "  {} - {}",
                    comp.name,
                    comp.file_path.as_deref().unwrap_or("unknown")
                );
            }
        }
    }

    Ok(())
}

async fn execute_graph(args: GraphVisualizationArgs, global: GlobalOptions) -> Result<()> {
    let backend = create_backend(&global).await?;

    // Get all components
    let nodes = backend
        .find_nodes("*", Some("Container"), 1000)
        .await
        .context("Failed to query components")?;

    let components: Vec<_> = nodes
        .into_iter()
        .filter(|n| n.kind.as_deref() == Some("component"))
        .collect();

    // Apply include/exclude filters
    let include_set: Option<HashSet<_>> = args
        .include
        .as_ref()
        .map(|s| s.split(',').map(|n| n.trim().to_string()).collect());

    let exclude_set: HashSet<_> = args
        .exclude
        .as_ref()
        .map(|s| s.split(',').map(|n| n.trim().to_string()).collect())
        .unwrap_or_default();

    let filtered_components: Vec<_> = components
        .iter()
        .filter(|c| {
            if let Some(ref include) = include_set {
                include.contains(&c.name)
            } else {
                true
            }
        })
        .filter(|c| !exclude_set.contains(&c.name))
        .collect();

    // Build edges map
    let mut edges: Vec<(String, String)> = Vec::new();

    for comp in &filtered_components {
        let deps = backend
            .get_edges(&comp.id, Some("DependsOn"), "outgoing")
            .await?;

        for edge in deps {
            let target_name = edge.to_id.rsplit(':').next().unwrap_or(&edge.to_id);
            edges.push((comp.name.clone(), target_name.to_string()));
        }
    }

    match args.format {
        GraphFormat::Dot => {
            println!("digraph components {{");
            println!("  rankdir=LR;");
            println!("  node [shape=box];");
            for comp in &filtered_components {
                println!("  \"{}\";", comp.name);
            }
            for (from, to) in &edges {
                println!("  \"{}\" -> \"{}\";", from, to);
            }
            println!("}}");
        }
        GraphFormat::Mermaid => {
            println!("graph LR");
            for (from, to) in &edges {
                println!("  {} --> {}", from.replace('-', "_"), to.replace('-', "_"));
            }
        }
        GraphFormat::Ascii => {
            if filtered_components.is_empty() {
                println!("No components found.");
                return Ok(());
            }

            // Build dependency tree from roots
            let roots: Vec<_> = filtered_components
                .iter()
                .filter(|c| {
                    c.metadata
                        .get("is_workspace_root")
                        .map(|v| v == "true")
                        .unwrap_or(false)
                })
                .collect();

            if roots.is_empty() {
                // Just list all components
                println!("Components (no root detected):");
                for comp in &filtered_components {
                    println!("  {}", comp.name);
                }
            } else {
                println!("Component dependency tree:\n");

                // Build adjacency list
                let mut adj: HashMap<String, Vec<String>> = HashMap::new();
                for (from, to) in &edges {
                    adj.entry(from.clone()).or_default().push(to.clone());
                }

                for root in &roots {
                    print_ascii_tree(&root.name, &adj, 0, &mut HashSet::new());
                }
            }
        }
    }

    Ok(())
}

fn print_ascii_tree(
    name: &str,
    adj: &HashMap<String, Vec<String>>,
    depth: usize,
    visited: &mut HashSet<String>,
) {
    let indent = "  ".repeat(depth);
    let marker = if depth == 0 { "" } else { "|- " };

    if visited.contains(name) {
        println!("{}{}{} (circular)", indent, marker, name);
        return;
    }
    visited.insert(name.to_string());

    println!("{}{}{}", indent, marker, name);

    if let Some(deps) = adj.get(name) {
        for dep in deps {
            print_ascii_tree(dep, adj, depth + 1, visited);
        }
    }

    visited.remove(name);
}
