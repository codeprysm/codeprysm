//! Backend command - Manage Qdrant backend infrastructure
//!
//! Provides commands for starting, stopping, and checking the status
//! of the Qdrant vector database backend via Docker.

use std::process::Command as ProcessCommand;

use anyhow::{Context, Result};
use clap::Subcommand;
use codeprysm_search::{QdrantConfig, QdrantStore};
use serde::Serialize;

use crate::progress::{finish_spinner, finish_spinner_warn, spinner};
use crate::GlobalOptions;

/// Docker container name for Qdrant
const QDRANT_CONTAINER_NAME: &str = "codeprysm-qdrant";

/// Default Qdrant Docker image
const QDRANT_IMAGE: &str = "qdrant/qdrant:latest";

/// Backend management commands
#[derive(Subcommand, Debug)]
pub enum BackendCommand {
    /// Start the Qdrant backend container
    Start(StartArgs),

    /// Stop the Qdrant backend container
    Stop(StopArgs),

    /// Check the backend status
    Status(StatusArgs),
}

/// Arguments for the start command
#[derive(clap::Args, Debug)]
pub struct StartArgs {
    /// Docker image to use
    #[arg(long, default_value = QDRANT_IMAGE)]
    image: String,

    /// Port for gRPC API (default: 6334)
    #[arg(long, default_value = "6334")]
    grpc_port: u16,

    /// Port for REST API (default: 6333)
    #[arg(long, default_value = "6333")]
    rest_port: u16,

    /// Persist data to a local directory
    #[arg(long)]
    storage: Option<String>,

    /// Force restart if container already exists
    #[arg(long, short = 'f')]
    force: bool,
}

/// Arguments for the stop command
#[derive(clap::Args, Debug)]
pub struct StopArgs {
    /// Remove the container after stopping
    #[arg(long)]
    remove: bool,
}

/// Arguments for the status command
#[derive(clap::Args, Debug)]
pub struct StatusArgs {
    /// Output as JSON
    #[arg(long)]
    json: bool,
}

/// Backend status information
#[derive(Debug, Clone, Serialize)]
pub struct BackendStatus {
    /// Whether Docker is available
    pub docker_available: bool,
    /// Container status (running, stopped, not_found)
    pub container_status: String,
    /// Container ID if exists
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container_id: Option<String>,
    /// Whether Qdrant is reachable
    pub qdrant_reachable: bool,
    /// Qdrant URL
    pub qdrant_url: String,
    /// Error message if any
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Execute the backend command
pub async fn execute(cmd: BackendCommand, global: GlobalOptions) -> Result<()> {
    match cmd {
        BackendCommand::Start(args) => execute_start(args, global).await,
        BackendCommand::Stop(args) => execute_stop(args, global).await,
        BackendCommand::Status(args) => execute_status(args, global).await,
    }
}

async fn execute_start(args: StartArgs, global: GlobalOptions) -> Result<()> {
    // Check if Docker is available
    if !is_docker_available() {
        anyhow::bail!(
            "Docker is not available. Please install Docker and ensure it is running.\n\
             See: https://docs.docker.com/get-docker/"
        );
    }

    // Check if container already exists
    let container_status = get_container_status();

    if container_status == "running" {
        if args.force {
            // Stop and remove existing container
            let pb = spinner("Stopping existing container...", global.quiet);
            stop_container(true)?;
            finish_spinner(pb, "Stopped existing container");
        } else {
            if !global.quiet {
                println!("Qdrant container is already running.");
                println!("Use --force to restart it.");
            }
            return Ok(());
        }
    } else if container_status == "exited" || container_status == "created" {
        if args.force {
            // Remove existing container
            remove_container()?;
        } else {
            // Just start the existing container
            let pb = spinner("Starting existing container...", global.quiet);
            start_existing_container()?;
            finish_spinner(pb, "Started Qdrant container");
            print_connection_info(&global, args.grpc_port, args.rest_port);
            return Ok(());
        }
    }

    // Start a new container
    let pb = spinner("Starting Qdrant container...", global.quiet);

    let mut docker_args = vec![
        "run".to_string(),
        "-d".to_string(),
        "--name".to_string(),
        QDRANT_CONTAINER_NAME.to_string(),
        "-p".to_string(),
        format!("{}:6333", args.rest_port),
        "-p".to_string(),
        format!("{}:6334", args.grpc_port),
    ];

    // Add storage volume if specified
    if let Some(storage) = &args.storage {
        docker_args.push("-v".to_string());
        docker_args.push(format!("{}:/qdrant/storage", storage));
    }

    docker_args.push(args.image.clone());

    let output = ProcessCommand::new("docker")
        .args(&docker_args)
        .output()
        .context("Failed to execute docker command")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        finish_spinner_warn(pb, "Failed to start container");
        anyhow::bail!("Docker run failed: {}", stderr);
    }

    finish_spinner(pb, "Started Qdrant container");
    print_connection_info(&global, args.grpc_port, args.rest_port);

    Ok(())
}

async fn execute_stop(args: StopArgs, global: GlobalOptions) -> Result<()> {
    if !is_docker_available() {
        anyhow::bail!("Docker is not available");
    }

    let status = get_container_status();
    if status == "not_found" {
        if !global.quiet {
            println!("No Qdrant container found.");
        }
        return Ok(());
    }

    let pb = spinner("Stopping Qdrant container...", global.quiet);

    if status == "running" {
        stop_container(false)?;
    }

    if args.remove {
        remove_container()?;
        finish_spinner(pb, "Stopped and removed Qdrant container");
    } else {
        finish_spinner(pb, "Stopped Qdrant container");
    }

    Ok(())
}

async fn execute_status(args: StatusArgs, global: GlobalOptions) -> Result<()> {
    let status = get_backend_status(&global.qdrant_url).await;

    if args.json {
        println!("{}", serde_json::to_string_pretty(&status)?);
    } else {
        print_status(&status, global.verbose);
    }

    Ok(())
}

fn is_docker_available() -> bool {
    ProcessCommand::new("docker")
        .args(["version", "--format", "{{.Server.Version}}"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn get_container_status() -> String {
    let output = ProcessCommand::new("docker")
        .args([
            "inspect",
            "--format",
            "{{.State.Status}}",
            QDRANT_CONTAINER_NAME,
        ])
        .output();

    match output {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "not_found".to_string(),
    }
}

fn get_container_id() -> Option<String> {
    let output = ProcessCommand::new("docker")
        .args(["inspect", "--format", "{{.Id}}", QDRANT_CONTAINER_NAME])
        .output()
        .ok()?;

    if output.status.success() {
        let id = String::from_utf8_lossy(&output.stdout).trim().to_string();
        Some(id[..12.min(id.len())].to_string()) // Short ID
    } else {
        None
    }
}

fn stop_container(remove: bool) -> Result<()> {
    let output = ProcessCommand::new("docker")
        .args(["stop", QDRANT_CONTAINER_NAME])
        .output()
        .context("Failed to stop container")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to stop container: {}", stderr);
    }

    if remove {
        remove_container()?;
    }

    Ok(())
}

fn start_existing_container() -> Result<()> {
    let output = ProcessCommand::new("docker")
        .args(["start", QDRANT_CONTAINER_NAME])
        .output()
        .context("Failed to start container")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to start container: {}", stderr);
    }

    Ok(())
}

fn remove_container() -> Result<()> {
    let output = ProcessCommand::new("docker")
        .args(["rm", "-f", QDRANT_CONTAINER_NAME])
        .output()
        .context("Failed to remove container")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        anyhow::bail!("Failed to remove container: {}", stderr);
    }

    Ok(())
}

fn print_connection_info(global: &GlobalOptions, grpc_port: u16, rest_port: u16) {
    if !global.quiet {
        println!("\nQdrant is running:");
        println!("  REST API: http://localhost:{}", rest_port);
        println!("  gRPC API: http://localhost:{}", grpc_port);
        println!("\nTo use with Prism:");
        println!(
            "  prism --qdrant-url http://localhost:{} search \"your query\"",
            grpc_port
        );
    }
}

async fn get_backend_status(qdrant_url: &str) -> BackendStatus {
    let docker_available = is_docker_available();
    let container_status = if docker_available {
        get_container_status()
    } else {
        "unknown".to_string()
    };
    let container_id = if docker_available {
        get_container_id()
    } else {
        None
    };

    // Check if Qdrant is reachable
    let qdrant_reachable = check_qdrant_reachable(qdrant_url).await;

    BackendStatus {
        docker_available,
        container_status,
        container_id,
        qdrant_reachable,
        qdrant_url: qdrant_url.to_string(),
        error: None,
    }
}

async fn check_qdrant_reachable(qdrant_url: &str) -> bool {
    // Try to connect to Qdrant via QdrantStore
    let config = QdrantConfig::with_url(qdrant_url);
    QdrantStore::connect(config, "health-check").await.is_ok()
}

fn print_status(status: &BackendStatus, verbose: bool) {
    println!("CodePrysm Backend Status");
    println!("====================\n");

    // Docker status
    let docker_icon = if status.docker_available {
        "\x1b[32m✓\x1b[0m"
    } else {
        "\x1b[31m✗\x1b[0m"
    };
    println!(
        "{} Docker: {}",
        docker_icon,
        if status.docker_available {
            "available"
        } else {
            "not available"
        }
    );

    // Container status
    let container_icon = match status.container_status.as_str() {
        "running" => "\x1b[32m✓\x1b[0m",
        "exited" | "created" => "\x1b[33m!\x1b[0m",
        _ => "\x1b[90m-\x1b[0m",
    };

    let container_msg = match status.container_status.as_str() {
        "running" => format!(
            "running ({})",
            status.container_id.as_deref().unwrap_or("?")
        ),
        "exited" => "stopped".to_string(),
        "created" => "created but not started".to_string(),
        "not_found" => "not found".to_string(),
        other => other.to_string(),
    };
    println!("{} Container: {}", container_icon, container_msg);

    // Qdrant connectivity
    let qdrant_icon = if status.qdrant_reachable {
        "\x1b[32m✓\x1b[0m"
    } else {
        "\x1b[31m✗\x1b[0m"
    };
    println!(
        "{} Qdrant: {}",
        qdrant_icon,
        if status.qdrant_reachable {
            "reachable"
        } else {
            "not reachable"
        }
    );

    if verbose {
        println!("\nDetails:");
        println!("  URL: {}", status.qdrant_url);
    }

    // Recommendations
    if !status.docker_available {
        println!("\nRecommendation: Install Docker to manage the Qdrant backend.");
    } else if status.container_status == "not_found" {
        println!("\nTo start Qdrant: prism backend start");
    } else if status.container_status == "exited" {
        println!("\nTo restart Qdrant: prism backend start");
    } else if !status.qdrant_reachable && status.container_status == "running" {
        println!("\nQdrant container is running but not reachable.");
        println!(
            "Check the container logs: docker logs {}",
            QDRANT_CONTAINER_NAME
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_container_name() {
        assert_eq!(QDRANT_CONTAINER_NAME, "codeprysm-qdrant");
    }

    #[test]
    fn test_backend_status_serialization() {
        let status = BackendStatus {
            docker_available: true,
            container_status: "running".to_string(),
            container_id: Some("abc123".to_string()),
            qdrant_reachable: true,
            qdrant_url: "http://localhost:6334".to_string(),
            error: None,
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("\"docker_available\":true"));
        assert!(json.contains("\"container_status\":\"running\""));
        assert!(json.contains("\"qdrant_reachable\":true"));
    }

    #[test]
    fn test_backend_status_skips_none() {
        let status = BackendStatus {
            docker_available: false,
            container_status: "not_found".to_string(),
            container_id: None,
            qdrant_reachable: false,
            qdrant_url: "http://localhost:6334".to_string(),
            error: None,
        };

        let json = serde_json::to_string(&status).unwrap();
        assert!(!json.contains("container_id"));
        assert!(!json.contains("error"));
    }
}
