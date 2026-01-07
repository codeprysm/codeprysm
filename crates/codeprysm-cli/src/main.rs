//! CodePrism CLI - Code graph generation and semantic search
//!
//! A command-line interface for managing code graphs, performing semantic search,
//! and analyzing component dependencies across workspaces.
//!
//! # Usage
//!
//! ```bash
//! # Initialize a new workspace
//! codeprysm init
//!
//! # Search for code patterns
//! codeprysm search "authentication logic"
//!
//! # List components
//! codeprysm components list
//!
//! # Show component dependencies
//! codeprysm components deps my-crate
//! ```

use std::path::PathBuf;

use anyhow::Result;
use clap::{Args, Parser, Subcommand};
use tracing::Level;
use tracing_subscriber::FmtSubscriber;

mod commands;
mod progress;

/// CodePrism - Semantic code search and graph analysis
#[derive(Parser, Debug)]
#[command(name = "codeprysm")]
#[command(author, version, about, long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    #[command(flatten)]
    global: GlobalOptions,
}

/// Global options available to all commands
#[derive(Args, Debug, Clone)]
struct GlobalOptions {
    /// Workspace to operate on (name or path)
    #[arg(long, short = 'w', global = true, env = "CODEPRYSM_WORKSPACE")]
    workspace: Option<String>,

    /// Path to configuration file
    #[arg(long, short = 'c', global = true, env = "CODEPRYSM_CONFIG")]
    config: Option<PathBuf>,

    /// Enable verbose output
    #[arg(long, short = 'v', global = true)]
    verbose: bool,

    /// Suppress non-essential output
    #[arg(long, short = 'q', global = true)]
    quiet: bool,

    /// Qdrant server URL
    #[arg(
        long,
        global = true,
        env = "CODEPRYSM_QDRANT_URL",
        default_value = "http://localhost:6334"
    )]
    qdrant_url: String,

    /// Embedding provider type (local, azure-ml, openai)
    #[arg(long, global = true, env = "CODEPRYSM_EMBEDDING_PROVIDER", value_parser = parse_embedding_provider)]
    embedding_provider: Option<codeprysm_config::EmbeddingProviderType>,
}

/// Parse embedding provider from string
fn parse_embedding_provider(s: &str) -> Result<codeprysm_config::EmbeddingProviderType, String> {
    s.parse()
        .map_err(|e: codeprysm_config::ConfigError| e.to_string())
}

impl GlobalOptions {
    /// Convert global options to config overrides
    pub fn to_config_overrides(&self) -> codeprysm_config::ConfigOverrides {
        codeprysm_config::ConfigOverrides {
            qdrant_url: Some(self.qdrant_url.clone()),
            embedding_provider: self.embedding_provider,
            ..Default::default()
        }
    }
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Initialize a new CodePrysm workspace
    Init(commands::init::InitArgs),

    /// Update the code graph incrementally
    Update(commands::update::UpdateArgs),

    /// Search the codebase semantically or by pattern
    Search(commands::search::SearchArgs),

    /// Graph query and navigation commands
    Graph(commands::graph::GraphArgs),

    /// Component management and analysis
    #[command(subcommand)]
    Components(commands::components::ComponentsCommand),

    /// Workspace management commands
    #[command(subcommand)]
    Workspace(commands::workspace::WorkspaceCommand),

    /// Show configuration and status
    Status(commands::status::StatusArgs),

    /// Comprehensive health check with recommendations
    Doctor(commands::doctor::DoctorArgs),

    /// Remove CodePrysm data (local and/or backend)
    Clean(commands::clean::CleanArgs),

    /// Manage the Qdrant backend (start/stop/status)
    #[command(subcommand)]
    Backend(commands::backend::BackendCommand),

    /// View and manage configuration
    #[command(subcommand)]
    Config(commands::config::ConfigCommand),

    /// Start the MCP server for AI assistant integration
    Mcp(commands::mcp::McpArgs),
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Set up logging based on verbosity
    let log_level = if cli.global.quiet {
        Level::ERROR
    } else if cli.global.verbose {
        Level::DEBUG
    } else {
        Level::INFO
    };

    // MCP command handles its own tracing setup (needs ansi=false for JSON-RPC protocol,
    // and must gracefully handle pre-existing subscribers when launched by Claude Code)
    if !matches!(cli.command, Commands::Mcp(_)) {
        let subscriber = FmtSubscriber::builder()
            .with_max_level(log_level)
            .with_writer(std::io::stderr)
            .with_ansi(true)
            .finish();
        tracing::subscriber::set_global_default(subscriber)?;
    }

    // Execute the command
    match cli.command {
        Commands::Init(args) => commands::init::execute(args, cli.global).await,
        Commands::Update(args) => commands::update::execute(args, cli.global).await,
        Commands::Search(args) => commands::search::execute(args, cli.global).await,
        Commands::Graph(args) => commands::graph::execute(args, cli.global).await,
        Commands::Components(cmd) => commands::components::execute(cmd, cli.global).await,
        Commands::Workspace(cmd) => commands::workspace::execute(cmd, cli.global).await,
        Commands::Status(args) => commands::status::execute(args, cli.global).await,
        Commands::Doctor(args) => commands::doctor::execute(args, cli.global).await,
        Commands::Clean(args) => commands::clean::execute(args, cli.global).await,
        Commands::Backend(cmd) => commands::backend::execute(cmd, cli.global).await,
        Commands::Config(cmd) => commands::config::execute(cmd, cli.global).await,
        Commands::Mcp(args) => commands::mcp::execute(args, cli.global).await,
    }
}
