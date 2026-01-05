//! Workspace command - Multi-workspace management

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use codeprysm_backend::WorkspaceRegistry;

use crate::GlobalOptions;

/// Workspace management commands
#[derive(Subcommand, Debug)]
pub enum WorkspaceCommand {
    /// List registered workspaces
    List(ListArgs),

    /// Register a new workspace
    Add(AddArgs),

    /// Unregister a workspace
    Remove(RemoveArgs),

    /// Set the active workspace
    Use(UseArgs),

    /// Discover and register workspaces
    Discover(DiscoverArgs),
}

#[derive(Args, Debug)]
pub struct ListArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

#[derive(Args, Debug)]
pub struct AddArgs {
    /// Name for the workspace
    name: String,

    /// Path to the workspace root
    path: PathBuf,
}

#[derive(Args, Debug)]
pub struct RemoveArgs {
    /// Workspace name to remove
    name: String,
}

#[derive(Args, Debug)]
pub struct UseArgs {
    /// Workspace name to set as active
    name: String,
}

#[derive(Args, Debug)]
pub struct DiscoverArgs {
    /// Root directory to search for workspaces
    #[arg(default_value = ".")]
    root: PathBuf,

    /// Maximum depth to search
    #[arg(long, short = 'd', default_value = "3")]
    depth: usize,

    /// Prefix for discovered workspace names
    #[arg(long)]
    prefix: Option<String>,

    /// Actually register discovered workspaces (dry-run by default)
    #[arg(long)]
    register: bool,
}

/// Execute workspace commands
pub async fn execute(cmd: WorkspaceCommand, global: GlobalOptions) -> Result<()> {
    let registry = WorkspaceRegistry::new()
        .await
        .context("Failed to load workspace registry")?;

    match cmd {
        WorkspaceCommand::List(args) => execute_list(&registry, args, global).await,
        WorkspaceCommand::Add(args) => execute_add(&registry, args, global).await,
        WorkspaceCommand::Remove(args) => execute_remove(&registry, args, global).await,
        WorkspaceCommand::Use(args) => execute_use(&registry, args, global).await,
        WorkspaceCommand::Discover(args) => execute_discover(&registry, args, global).await,
    }
}

async fn execute_list(
    registry: &WorkspaceRegistry,
    args: ListArgs,
    global: GlobalOptions,
) -> Result<()> {
    let workspaces = registry.list().await;

    if workspaces.is_empty() {
        if !global.quiet {
            println!("No workspaces registered.");
            println!("\nTo register a workspace:");
            println!("  codeprysm workspace add <name> <path>");
            println!("\nTo discover workspaces:");
            println!("  codeprysm workspace discover --register");
        }
        return Ok(());
    }

    if args.json {
        let json: Vec<_> = workspaces
            .iter()
            .map(|w| {
                serde_json::json!({
                    "name": w.name,
                    "path": w.path,
                    "has_graph": w.has_graph,
                    "has_index": w.has_index,
                    "is_active": w.is_active,
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json)?);
    } else {
        if !global.quiet {
            println!("Registered workspaces:\n");
        }

        for ws in &workspaces {
            let active = if ws.is_active { " *" } else { "" };
            let status = if ws.has_graph {
                if ws.has_index {
                    "[indexed]"
                } else {
                    "[graph only]"
                }
            } else {
                "[no graph]"
            };

            println!("  {}{} {} - {}", ws.name, active, status, ws.path.display());
        }

        if !global.quiet {
            let active = registry.active().await;
            if let Some(name) = active {
                println!("\n  * = active workspace ({})", name);
            } else {
                println!("\n  No active workspace set. Use 'codeprysm workspace use <name>'");
            }
        }
    }

    Ok(())
}

async fn execute_add(
    registry: &WorkspaceRegistry,
    args: AddArgs,
    global: GlobalOptions,
) -> Result<()> {
    let path = registry
        .register(&args.name, &args.path)
        .await
        .context("Failed to register workspace")?;

    if !global.quiet {
        println!("Registered workspace '{}' at {}", args.name, path.display());
        println!("\nTo initialize the graph:");
        println!("  prism -w {} init", args.name);
    }

    Ok(())
}

async fn execute_remove(
    registry: &WorkspaceRegistry,
    args: RemoveArgs,
    global: GlobalOptions,
) -> Result<()> {
    let removed = registry
        .unregister(&args.name)
        .await
        .context("Failed to unregister workspace")?;

    if removed {
        if !global.quiet {
            println!("Removed workspace '{}'", args.name);
        }
    } else {
        anyhow::bail!("Workspace '{}' not found", args.name);
    }

    Ok(())
}

async fn execute_use(
    registry: &WorkspaceRegistry,
    args: UseArgs,
    global: GlobalOptions,
) -> Result<()> {
    registry
        .set_active(&args.name)
        .await
        .context("Failed to set active workspace")?;

    if !global.quiet {
        println!("Active workspace set to '{}'", args.name);
    }

    Ok(())
}

async fn execute_discover(
    registry: &WorkspaceRegistry,
    args: DiscoverArgs,
    global: GlobalOptions,
) -> Result<()> {
    let root = if args.root.is_absolute() {
        args.root.clone()
    } else {
        std::env::current_dir()?.join(&args.root)
    };

    let root = root.canonicalize().context("Failed to resolve root path")?;

    if !global.quiet {
        println!("Discovering workspaces under {}...\n", root.display());
    }

    let discovered = registry
        .discover(&root, args.depth)
        .await
        .context("Failed to discover workspaces")?;

    if discovered.is_empty() {
        if !global.quiet {
            println!("No workspaces found.");
            println!("\nWorkspaces are detected by the presence of .codeprysm/manifest.json");
            println!("Initialize a workspace with: codeprysm init <path>");
        }
        return Ok(());
    }

    if !global.quiet {
        println!("Found {} workspaces:\n", discovered.len());
    }

    for path in &discovered {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");

        let full_name = if let Some(ref prefix) = args.prefix {
            format!("{}/{}", prefix, name)
        } else {
            name.to_string()
        };

        println!("  {} - {}", full_name, path.display());
    }

    if args.register {
        if !global.quiet {
            println!("\nRegistering discovered workspaces...");
        }

        let count = registry
            .register_discovered(&discovered, args.prefix.as_deref())
            .await
            .context("Failed to register workspaces")?;

        if !global.quiet {
            println!("Registered {} workspaces", count);
        }
    } else if !global.quiet {
        println!("\nTo register these workspaces, run:");
        println!("  codeprysm workspace discover --register");
    }

    Ok(())
}
