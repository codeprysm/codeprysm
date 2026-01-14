//! Graph context abstraction for memory-efficient indexing.
//!
//! This module provides the `GraphContext` trait that abstracts graph access
//! for the `SemanticTextBuilder`. This enables both in-memory (`PetCodeGraph`)
//! and streaming (`LazyGraphManager`) indexing strategies.
//!
//! # Memory-Efficient Indexing
//!
//! The trait is key to the streaming indexing strategy:
//!
//! - **In-memory mode** (`PetCodeGraph`): Load entire graph, process all nodes.
//!   Simple but uses O(n) memory where n = total nodes.
//!
//! - **Streaming mode** (`LazyGraphManager`): Process partition-by-partition.
//!   Memory bounded by max(partition_size) regardless of total graph size.
//!
//! Use streaming mode for repositories with >10,000 nodes.
//!
//! # Provided Implementations
//!
//! - `PetCodeGraph` / `&PetCodeGraph` - In-memory graph (clones values)
//! - `LazyGraphManager` / `&LazyGraphManager` - Lazy-loading graph (loads on demand)
//!
//! # Design Rationale
//!
//! The trait methods return owned values (`Node`, `Vec<Node>`) rather than
//! references because:
//! - `PetCodeGraph` returns references that would require cloning anyway
//! - `LazyGraphManager` returns owned values from lazy loading
//! - Owned values are the common denominator for both implementations
//!
//! # Example
//!
//! ```rust,ignore
//! use codeprysm_search::graph_context::GraphContext;
//!
//! fn build_context<G: GraphContext>(graph: &G, node_id: &str) -> String {
//!     if let Some(parent) = graph.get_parent(node_id) {
//!         format!("in {} {}", parent.kind.unwrap_or_default(), parent.name)
//!     } else {
//!         String::new()
//!     }
//! }
//! ```
//!
//! # Implementing for Custom Graph Types
//!
//! To implement `GraphContext` for your own graph type:
//!
//! ```rust,ignore
//! use codeprysm_search::GraphContext;
//! use codeprysm_core::{Node, EdgeData};
//!
//! struct MyGraph { /* ... */ }
//!
//! impl GraphContext for MyGraph {
//!     fn get_node(&self, id: &str) -> Option<Node> {
//!         // Look up node by ID
//!     }
//!
//!     fn get_parent(&self, id: &str) -> Option<Node> {
//!         // Find parent via incoming CONTAINS edge
//!     }
//!
//!     fn get_children(&self, id: &str) -> Vec<Node> {
//!         // Find children via outgoing CONTAINS edges
//!     }
//!
//!     fn get_outgoing_edges(&self, id: &str) -> Vec<(Node, EdgeData)> {
//!         // Return all outgoing edges with their targets
//!     }
//! }
//! ```

use codeprysm_core::lazy::manager::LazyGraphManager;
use codeprysm_core::{EdgeData, EdgeType, Node, PetCodeGraph};

/// Trait for accessing graph nodes and edges.
///
/// This abstraction allows `SemanticTextBuilder` to work with both:
/// - `PetCodeGraph`: In-memory graph (fast, but loads everything)
/// - `LazyGraphManager`: Streaming graph (memory-bounded, loads on-demand)
///
/// All methods return owned values for consistency across implementations.
pub trait GraphContext {
    /// Get a node by ID.
    ///
    /// Returns `Some(Node)` if the node exists, `None` otherwise.
    fn get_node(&self, id: &str) -> Option<Node>;

    /// Get the parent node (via incoming CONTAINS edge).
    ///
    /// Most nodes have at most one parent in the containment hierarchy.
    /// Returns `None` for top-level nodes (files, root containers).
    fn get_parent(&self, id: &str) -> Option<Node>;

    /// Get all child nodes (via outgoing CONTAINS edges).
    ///
    /// For containers: returns methods, fields, nested types.
    /// For files: returns top-level definitions.
    fn get_children(&self, id: &str) -> Vec<Node>;

    /// Get all outgoing edges with their target nodes.
    ///
    /// Returns tuples of (target_node, edge_data) for all outgoing edges.
    /// Used for finding references, calls, type usages, etc.
    fn get_outgoing_edges(&self, id: &str) -> Vec<(Node, EdgeData)>;
}

// =============================================================================
// Implementation for PetCodeGraph
// =============================================================================

impl GraphContext for PetCodeGraph {
    fn get_node(&self, id: &str) -> Option<Node> {
        self.get_node(id).cloned()
    }

    fn get_parent(&self, id: &str) -> Option<Node> {
        self.parent(id).cloned()
    }

    fn get_children(&self, id: &str) -> Vec<Node> {
        self.children(id).cloned().collect()
    }

    fn get_outgoing_edges(&self, id: &str) -> Vec<(Node, EdgeData)> {
        self.outgoing_edges(id)
            .map(|(node, edge)| (node.clone(), edge.clone()))
            .collect()
    }
}

// =============================================================================
// Implementation for &PetCodeGraph (reference)
// =============================================================================

impl GraphContext for &PetCodeGraph {
    fn get_node(&self, id: &str) -> Option<Node> {
        (*self).get_node(id).cloned()
    }

    fn get_parent(&self, id: &str) -> Option<Node> {
        (*self).parent(id).cloned()
    }

    fn get_children(&self, id: &str) -> Vec<Node> {
        (*self).children(id).cloned().collect()
    }

    fn get_outgoing_edges(&self, id: &str) -> Vec<(Node, EdgeData)> {
        (*self)
            .outgoing_edges(id)
            .map(|(node, edge)| (node.clone(), edge.clone()))
            .collect()
    }
}

// =============================================================================
// Implementation for LazyGraphManager
// =============================================================================

impl GraphContext for LazyGraphManager {
    fn get_node(&self, id: &str) -> Option<Node> {
        // Use the lazy-loading get_node method
        // Unwrap Result to Option, logging errors at debug level
        // since context is best-effort enhancement
        match self.get_node(id) {
            Ok(node) => node,
            Err(e) => {
                tracing::debug!("GraphContext::get_node failed for {}: {}", id, e);
                None
            }
        }
    }

    fn get_parent(&self, id: &str) -> Option<Node> {
        // Get incoming edges and find the CONTAINS edge (parent relationship)
        match self.get_incoming_edges(id) {
            Ok(edges) => edges
                .into_iter()
                .find(|(_, edge)| edge.edge_type == EdgeType::Contains)
                .map(|(node, _)| node),
            Err(e) => {
                tracing::debug!("GraphContext::get_parent failed for {}: {}", id, e);
                None
            }
        }
    }

    fn get_children(&self, id: &str) -> Vec<Node> {
        // Get outgoing edges and filter for CONTAINS edges (children)
        match self.get_outgoing_edges(id) {
            Ok(edges) => edges
                .into_iter()
                .filter(|(_, edge)| edge.edge_type == EdgeType::Contains)
                .map(|(node, _)| node)
                .collect(),
            Err(e) => {
                tracing::debug!("GraphContext::get_children failed for {}: {}", id, e);
                Vec::new()
            }
        }
    }

    fn get_outgoing_edges(&self, id: &str) -> Vec<(Node, EdgeData)> {
        // Use the lazy-loading get_outgoing_edges method
        match LazyGraphManager::get_outgoing_edges(self, id) {
            Ok(edges) => edges,
            Err(e) => {
                tracing::debug!("GraphContext::get_outgoing_edges failed for {}: {}", id, e);
                Vec::new()
            }
        }
    }
}

impl GraphContext for &LazyGraphManager {
    fn get_node(&self, id: &str) -> Option<Node> {
        GraphContext::get_node(*self, id)
    }

    fn get_parent(&self, id: &str) -> Option<Node> {
        GraphContext::get_parent(*self, id)
    }

    fn get_children(&self, id: &str) -> Vec<Node> {
        GraphContext::get_children(*self, id)
    }

    fn get_outgoing_edges(&self, id: &str) -> Vec<(Node, EdgeData)> {
        GraphContext::get_outgoing_edges(*self, id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeprysm_core::{EdgeData, NodeMetadata, NodeType};

    fn create_test_graph() -> PetCodeGraph {
        let mut graph = PetCodeGraph::new();

        // Create a class node
        let class_node = Node {
            id: "test.py:MyClass".to_string(),
            name: "MyClass".to_string(),
            node_type: NodeType::Container,
            kind: Some("type".to_string()),
            subtype: Some("class".to_string()),
            file: "test.py".to_string(),
            line: 1,
            end_line: 50,
            text: None,
            metadata: NodeMetadata::default(),
            hash: None,
        };
        graph.add_node(class_node);

        // Create a method node
        let method_node = Node {
            id: "test.py:MyClass:process".to_string(),
            name: "process".to_string(),
            node_type: NodeType::Callable,
            kind: Some("method".to_string()),
            subtype: None,
            file: "test.py".to_string(),
            line: 10,
            end_line: 20,
            text: None,
            metadata: NodeMetadata::default(),
            hash: None,
        };
        graph.add_node(method_node);

        // Add CONTAINS edge (class -> method)
        graph.add_edge(
            "test.py:MyClass",
            "test.py:MyClass:process",
            EdgeData::contains(),
        );

        graph
    }

    #[test]
    fn test_get_node() {
        let graph = create_test_graph();

        let node = graph.get_node("test.py:MyClass");
        assert!(node.is_some());
        assert_eq!(node.unwrap().name, "MyClass");

        let missing = graph.get_node("nonexistent");
        assert!(missing.is_none());
    }

    #[test]
    fn test_get_parent() {
        let graph = create_test_graph();

        let parent = graph.get_parent("test.py:MyClass:process");
        assert!(parent.is_some());
        assert_eq!(parent.unwrap().name, "MyClass");

        // Class has no parent (top-level)
        let no_parent = graph.get_parent("test.py:MyClass");
        assert!(no_parent.is_none());
    }

    #[test]
    fn test_get_children() {
        let graph = create_test_graph();

        let children = graph.get_children("test.py:MyClass");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].name, "process");

        // Method has no children
        let no_children = graph.get_children("test.py:MyClass:process");
        assert!(no_children.is_empty());
    }

    #[test]
    fn test_get_outgoing_edges() {
        let graph = create_test_graph();

        let edges = graph.get_outgoing_edges("test.py:MyClass");
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].0.name, "process");
        assert_eq!(edges[0].1.edge_type, codeprysm_core::EdgeType::Contains);
    }

    #[test]
    fn test_reference_impl() {
        let graph = create_test_graph();
        let graph_ref: &PetCodeGraph = &graph;

        // Test that &PetCodeGraph also implements GraphContext
        let node = GraphContext::get_node(&graph_ref, "test.py:MyClass");
        assert!(node.is_some());
        assert_eq!(node.unwrap().name, "MyClass");
    }
}
