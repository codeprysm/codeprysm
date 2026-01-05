//! MCP Tool parameter definitions
//!
//! These structs define the JSON Schema for tool parameters using schemars.

use rmcp::schemars::{self, JsonSchema};
use serde::{Deserialize, Serialize};

/// Parameters for search_graph_nodes tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// Search query (natural language or code identifier)
    #[schemars(
        description = "Search query string (can be exact name, partial match, or semantic description)"
    )]
    pub query: String,

    /// Search mode to control which embedding model is used
    #[schemars(
        description = "Search mode: 'code' for code patterns/identifiers (uses jina-base-code), 'info' for semantic/conceptual queries (uses jina-base-en). Defaults to hybrid (both, fused)."
    )]
    pub mode: Option<String>,

    /// Filter by entity types
    #[schemars(
        description = "Filter by entity types. Valid types: \"Container\", \"Callable\", \"Data\". Supports kind filtering with colon syntax: \"Container:type\", \"Container:file\", \"Callable:method\", \"Data:field\". Files are Container nodes with kind=\"file\"."
    )]
    pub node_types: Option<Vec<String>>,

    /// Maximum number of results
    #[schemars(description = "Maximum results to return (default 20)")]
    pub max_results: Option<usize>,
}

/// Parameters for get_node_info tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NodeInfoParams {
    /// Node ID to look up
    #[schemars(description = "The ID of the node (e.g., \"app/main.py:MainApp\" for a class)")]
    pub node_id: String,
}

/// Parameters for read_code tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ReadCodeParams {
    /// Node ID to read (optional)
    #[schemars(description = "Read content of a specific node (e.g., a function or class)")]
    pub node_id: Option<String>,

    /// File path to read (optional)
    #[schemars(description = "Read from a specific file (used when node_id is not provided)")]
    pub file_path: Option<String>,

    /// Starting line number
    #[schemars(description = "Starting line number (optional, defaults to node start or 1)")]
    pub line_start: Option<usize>,

    /// Ending line number
    #[schemars(
        description = "Ending line number (optional, defaults to node end or line_start + max_lines)"
    )]
    pub line_end: Option<usize>,

    /// Maximum lines to read
    #[schemars(description = "Maximum lines to read to prevent token overflow (default 100)")]
    pub max_lines: Option<usize>,

    /// Context lines before/after
    #[schemars(description = "Additional context lines before/after the range (default 0)")]
    pub context_lines: Option<usize>,
}

/// Parameters for find_references tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindReferencesParams {
    /// Node ID to find references for
    #[schemars(description = "The node to find references for")]
    pub node_id: String,

    /// Filter by edge types
    #[schemars(
        description = "Filter by relationship types (e.g., [\"USES\", \"CONTAINS\", \"DEFINES\"]). If not specified, returns all edge types"
    )]
    pub edge_types: Option<Vec<String>>,

    /// Include line numbers
    #[schemars(description = "Include line number information (default True)")]
    pub include_line_info: Option<bool>,

    /// Maximum results
    #[schemars(description = "Maximum results to return (default 50)")]
    pub max_results: Option<usize>,
}

/// Parameters for find_outgoing_references tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindOutgoingReferencesParams {
    /// Node ID to analyze
    #[schemars(description = "The node to analyze")]
    pub node_id: String,

    /// Filter by edge types
    #[schemars(
        description = "Filter by relationship types (e.g., [\"USES\", \"CONTAINS\", \"DEFINES\"]). If not specified, returns all edge types"
    )]
    pub edge_types: Option<Vec<String>>,

    /// Include line numbers
    #[schemars(description = "Include line number information (default True)")]
    pub include_line_info: Option<bool>,

    /// Maximum results
    #[schemars(description = "Maximum results to return (default 50)")]
    pub max_results: Option<usize>,
}

/// Parameters for find_definitions tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindDefinitionsParams {
    /// Node ID to find definitions for
    #[schemars(description = "The node to find definitions for")]
    pub node_id: String,

    /// Include line numbers
    #[schemars(description = "Include line number information (default True)")]
    pub include_line_info: Option<bool>,

    /// Maximum results
    #[schemars(description = "Maximum results to return (default 50)")]
    pub max_results: Option<usize>,
}

/// Parameters for find_call_chain tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindCallChainParams {
    /// Starting node ID
    #[schemars(description = "Starting point for the trace")]
    pub node_id: String,

    /// Direction to trace
    #[schemars(
        description = "\"upstream\" (who calls this), \"downstream\" (what this calls), or \"both\""
    )]
    pub direction: Option<String>,

    /// Maximum traversal depth
    #[schemars(description = "Maximum depth to traverse (default 3)")]
    pub max_depth: Option<usize>,

    /// Maximum chains to return
    #[schemars(description = "Maximum number of chains to return (default 5)")]
    pub max_chains: Option<usize>,

    /// Edge types to follow
    #[schemars(description = "Edge types to follow (defaults to [\"USES\"])")]
    pub edge_types: Option<Vec<String>>,
}

/// Parameters for find_module_structure tool
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FindModuleStructureParams {
    /// Base directory to analyze
    #[schemars(description = "Base directory to analyze (e.g., \"app/modules\")")]
    pub base_path: String,

    /// Maximum directory depth
    #[schemars(description = "Maximum directory depth to traverse (1-3, default 2)")]
    pub max_depth: Option<usize>,

    /// Filter by node types
    #[schemars(
        description = "Filter by entity types. Valid types: \"Container\", \"Callable\", \"Data\". Files are Container nodes with kind=\"file\"."
    )]
    pub node_types: Option<Vec<String>>,

    /// Include empty directories
    #[schemars(description = "Include directories with no matching nodes (default False)")]
    pub include_empty: Option<bool>,
}

/// Parameters for sync_repository tool (no params needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SyncRepositoryParams {}

/// Parameters for get_index_status tool (no params needed)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GetIndexStatusParams {}
