//! Shared types for backend operations.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Search result from any backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    /// Entity ID (e.g., "src/lib.rs:MyStruct:new")
    pub entity_id: String,

    /// Entity name
    pub name: String,

    /// Entity type (Container, Callable, Data)
    pub entity_type: String,

    /// Entity kind (e.g., "function", "class", "method")
    pub kind: String,

    /// Entity subtype (e.g., "async_function")
    pub subtype: String,

    /// File path relative to repo root
    pub file_path: String,

    /// Line range (start, end)
    pub line_range: (u32, u32),

    /// Relevance score (0.0 - 1.0+)
    pub score: f32,

    /// Code snippet content
    pub code_snippet: String,

    /// How this result was found (e.g., ["semantic", "code"])
    pub sources: Vec<String>,
}

impl SearchResult {
    /// Create a new search result.
    pub fn new(entity_id: impl Into<String>, name: impl Into<String>, score: f32) -> Self {
        Self {
            entity_id: entity_id.into(),
            name: name.into(),
            entity_type: String::new(),
            kind: String::new(),
            subtype: String::new(),
            file_path: String::new(),
            line_range: (0, 0),
            score,
            code_snippet: String::new(),
            sources: Vec::new(),
        }
    }
}

/// Convert from codeprysm_search HybridSearchHit.
impl From<codeprysm_search::HybridSearchHit> for SearchResult {
    fn from(hit: codeprysm_search::HybridSearchHit) -> Self {
        Self {
            entity_id: hit.entity_id,
            name: hit.name,
            entity_type: hit.entity_type,
            kind: hit.kind,
            subtype: hit.subtype,
            file_path: hit.file_path,
            line_range: hit.line_range,
            score: hit.combined_score,
            code_snippet: hit.code_snippet,
            sources: hit.found_via,
        }
    }
}

/// Node information from the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    /// Node ID
    pub id: String,

    /// Node name
    pub name: String,

    /// Node type (Container, Callable, Data)
    pub node_type: String,

    /// Node kind (e.g., "file", "function", "class")
    pub kind: Option<String>,

    /// File path
    pub file_path: Option<String>,

    /// Start line number
    pub start_line: Option<u32>,

    /// End line number
    pub end_line: Option<u32>,

    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

impl NodeInfo {
    /// Create node info from a codeprysm_core Node.
    pub fn from_node(node: &codeprysm_core::Node) -> Self {
        let mut metadata = HashMap::new();
        if let Some(ref m) = node.metadata.manifest_path {
            metadata.insert("manifest_path".to_string(), m.clone());
        }
        if node.metadata.is_workspace_root == Some(true) {
            metadata.insert("is_workspace_root".to_string(), "true".to_string());
        }
        if node.metadata.is_publishable == Some(true) {
            metadata.insert("is_publishable".to_string(), "true".to_string());
        }

        Self {
            id: node.id.clone(),
            name: node.name.clone(),
            node_type: format!("{:?}", node.node_type),
            kind: node.kind.clone(),
            file_path: Some(node.file.clone()),
            start_line: Some(node.line as u32),
            end_line: Some(node.end_line as u32),
            metadata,
        }
    }
}

/// Edge information from the graph.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeInfo {
    /// Source node ID
    pub from_id: String,

    /// Target node ID
    pub to_id: String,

    /// Edge type (Contains, Uses, Defines, DependsOn)
    pub edge_type: String,

    /// Edge metadata (e.g., version_spec for DependsOn)
    pub metadata: HashMap<String, String>,
}

/// Index status information.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexStatus {
    /// Whether the index exists
    pub exists: bool,

    /// Number of indexed entities
    pub entity_count: u64,

    /// Number of entities in semantic collection
    pub semantic_count: u64,

    /// Number of entities in code collection
    pub code_count: u64,

    /// Index version
    pub version: Option<String>,

    /// Last indexed timestamp (Unix epoch)
    pub last_indexed: Option<u64>,
}

impl IndexStatus {
    /// Create a new empty index status.
    pub fn empty() -> Self {
        Self {
            exists: false,
            entity_count: 0,
            semantic_count: 0,
            code_count: 0,
            version: None,
            last_indexed: None,
        }
    }

    /// Create an index status for an existing index.
    pub fn existing(semantic_count: u64, code_count: u64) -> Self {
        Self {
            exists: true,
            entity_count: semantic_count.max(code_count),
            semantic_count,
            code_count,
            version: Some(env!("CARGO_PKG_VERSION").to_string()),
            last_indexed: None,
        }
    }
}

/// Graph statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    /// Total number of nodes
    pub node_count: usize,

    /// Number of nodes by type
    pub nodes_by_type: HashMap<String, usize>,

    /// Total number of edges
    pub edge_count: usize,

    /// Number of edges by type
    pub edges_by_type: HashMap<String, usize>,

    /// Number of files indexed
    pub file_count: usize,

    /// Number of components detected
    pub component_count: usize,
}

impl GraphStats {
    /// Create stats from a PetCodeGraph.
    pub fn from_graph(graph: &codeprysm_core::PetCodeGraph) -> Self {
        use codeprysm_core::{EdgeType, NodeType};

        let mut nodes_by_type = HashMap::new();
        let mut edges_by_type = HashMap::new();
        let mut file_count = 0;
        let mut component_count = 0;

        for node in graph.iter_nodes() {
            let type_name = format!("{:?}", node.node_type);
            *nodes_by_type.entry(type_name).or_insert(0) += 1;

            if node.node_type == NodeType::Container {
                if node.kind.as_deref() == Some("file") {
                    file_count += 1;
                } else if node.kind.as_deref() == Some("component") {
                    component_count += 1;
                }
            }
        }

        for edge_type in [
            EdgeType::Contains,
            EdgeType::Uses,
            EdgeType::Defines,
            EdgeType::DependsOn,
        ] {
            let count = graph.edges_by_type(edge_type).count();
            if count > 0 {
                edges_by_type.insert(format!("{:?}", edge_type), count);
            }
        }

        Self {
            node_count: graph.node_count(),
            nodes_by_type,
            edge_count: graph.edge_count(),
            edges_by_type,
            file_count,
            component_count,
        }
    }
}

/// Search options for filtering and customization.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchOptions {
    /// Filter by node types (e.g., ["Callable", "Container"])
    pub node_types: Vec<String>,

    /// Filter by file path patterns (glob)
    pub file_patterns: Vec<String>,

    /// Search mode: "code", "info", or None for hybrid
    pub mode: Option<String>,

    /// Include code snippets in results
    pub include_snippets: bool,

    /// Minimum score threshold (0.0 - 1.0)
    pub min_score: Option<f32>,
}

impl SearchOptions {
    /// Create options for code-focused search.
    pub fn code_only() -> Self {
        Self {
            mode: Some("code".to_string()),
            include_snippets: true,
            ..Default::default()
        }
    }

    /// Create options for semantic/info search.
    pub fn semantic_only() -> Self {
        Self {
            mode: Some("info".to_string()),
            include_snippets: true,
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_result_new() {
        let result = SearchResult::new("src/lib.rs:foo", "foo", 0.95);
        assert_eq!(result.entity_id, "src/lib.rs:foo");
        assert_eq!(result.name, "foo");
        assert_eq!(result.score, 0.95);
    }

    #[test]
    fn test_index_status_empty() {
        let status = IndexStatus::empty();
        assert!(!status.exists);
        assert_eq!(status.entity_count, 0);
    }

    #[test]
    fn test_index_status_existing() {
        let status = IndexStatus::existing(100, 150);
        assert!(status.exists);
        assert_eq!(status.semantic_count, 100);
        assert_eq!(status.code_count, 150);
        assert_eq!(status.entity_count, 150); // max of the two
    }

    #[test]
    fn test_search_options_defaults() {
        let opts = SearchOptions::default();
        assert!(opts.node_types.is_empty());
        assert!(opts.mode.is_none());
    }

    #[test]
    fn test_search_options_code_only() {
        let opts = SearchOptions::code_only();
        assert_eq!(opts.mode, Some("code".to_string()));
        assert!(opts.include_snippets);
    }
}
