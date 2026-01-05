//! Search command - Semantic and keyword code search

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use codeprysm_backend::{Backend, SearchOptions};

use super::create_backend;
use crate::GlobalOptions;

/// Search mode
#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum SearchMode {
    /// Hybrid semantic + keyword search (default)
    Hybrid,
    /// Semantic search optimized for natural language queries
    Semantic,
    /// Code pattern search optimized for code identifiers
    Code,
}

impl SearchMode {
    fn to_option_str(self) -> Option<&'static str> {
        match self {
            SearchMode::Hybrid => None,
            SearchMode::Semantic => Some("info"),
            SearchMode::Code => Some("code"),
        }
    }
}

/// Arguments for the search command
#[derive(Args, Debug)]
pub struct SearchArgs {
    /// Search query
    query: String,

    /// Maximum number of results to return
    #[arg(long, short = 'n', default_value = "10")]
    limit: usize,

    /// Search mode: hybrid, semantic, or code
    #[arg(long, short = 'm', value_enum, default_value = "hybrid")]
    mode: SearchMode,

    /// Filter by node types (e.g., Callable, Container)
    #[arg(long, short = 't')]
    types: Vec<String>,

    /// Minimum relevance score (0.0 - 1.0)
    #[arg(long)]
    min_score: Option<f32>,

    /// Output format: text (default), json
    #[arg(long, short = 'o', default_value = "text")]
    output: OutputFormat,

    /// Include code snippets in output
    #[arg(long, short = 's')]
    snippets: bool,

    /// Show file paths only (compact output)
    #[arg(long)]
    files_only: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text output
    Text,
    /// JSON output for scripting
    Json,
}

/// Execute the search command
pub async fn execute(args: SearchArgs, global: GlobalOptions) -> Result<()> {
    let backend = create_backend(&global).await?;

    // Build search options
    let options = SearchOptions {
        node_types: args.types.clone(),
        file_patterns: vec![],
        mode: args.mode.to_option_str().map(String::from),
        include_snippets: args.snippets,
        min_score: args.min_score,
    };

    // Perform search
    let results = backend
        .search(&args.query, args.limit, Some(options))
        .await
        .context("Search failed")?;

    if results.is_empty() {
        if !global.quiet {
            eprintln!("No results found for: {}", args.query);
        }
        return Ok(());
    }

    // Format output
    match args.output {
        OutputFormat::Json => {
            let json =
                serde_json::to_string_pretty(&results).context("Failed to serialize results")?;
            println!("{}", json);
        }
        OutputFormat::Text => {
            if args.files_only {
                // Compact output - just file paths with line numbers
                let mut seen = std::collections::HashSet::new();
                for result in &results {
                    let key = format!("{}:{}", result.file_path, result.line_range.0);
                    if seen.insert(key.clone()) {
                        println!("{}", key);
                    }
                }
            } else {
                // Full output
                if !global.quiet {
                    println!("Found {} results for \"{}\":\n", results.len(), args.query);
                }

                for (i, result) in results.iter().enumerate() {
                    println!("{}. {} ({})", i + 1, result.name, result.kind);
                    println!(
                        "   {}:{}-{}",
                        result.file_path, result.line_range.0, result.line_range.1
                    );
                    println!(
                        "   Score: {:.3}  Sources: {}",
                        result.score,
                        result.sources.join(", ")
                    );

                    if args.snippets && !result.code_snippet.is_empty() {
                        println!("   ---");
                        for line in result.code_snippet.lines().take(5) {
                            println!("   {}", line);
                        }
                        if result.code_snippet.lines().count() > 5 {
                            println!("   ...");
                        }
                    }
                    println!();
                }
            }
        }
    }

    Ok(())
}
