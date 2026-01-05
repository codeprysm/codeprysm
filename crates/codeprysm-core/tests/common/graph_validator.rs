//! Graph validation utilities for integration tests.
//!
//! This module provides comprehensive validation of code graphs against
//! the v2.0 schema, including structural, semantic, relational, and
//! completeness validation.

#![allow(dead_code)]

use codeprysm_core::graph::{EdgeType, NodeType, PetCodeGraph};
use std::collections::{HashMap, HashSet};

// ============================================================================
// Validation Result Types
// ============================================================================

/// Result of graph validation
#[derive(Debug, Default)]
pub struct GraphValidationResult {
    /// Structural validation errors (missing required fields)
    pub structural_errors: Vec<String>,
    /// Semantic validation errors (invalid types/kinds)
    pub semantic_errors: Vec<String>,
    /// Relational validation errors (broken edges, cycles)
    pub relational_errors: Vec<String>,
    /// Completeness validation errors (missing expected entities)
    pub completeness_errors: Vec<String>,
    /// Graph statistics
    pub stats: GraphStats,
}

impl GraphValidationResult {
    /// Check if all validations passed
    pub fn is_valid(&self) -> bool {
        self.structural_errors.is_empty()
            && self.semantic_errors.is_empty()
            && self.relational_errors.is_empty()
            && self.completeness_errors.is_empty()
    }

    /// Get all errors as a single vector
    pub fn all_errors(&self) -> Vec<String> {
        let mut all = Vec::new();
        all.extend(
            self.structural_errors
                .iter()
                .map(|e| format!("[structural] {}", e)),
        );
        all.extend(
            self.semantic_errors
                .iter()
                .map(|e| format!("[semantic] {}", e)),
        );
        all.extend(
            self.relational_errors
                .iter()
                .map(|e| format!("[relational] {}", e)),
        );
        all.extend(
            self.completeness_errors
                .iter()
                .map(|e| format!("[completeness] {}", e)),
        );
        all
    }
}

/// Graph statistics
#[derive(Debug, Default)]
pub struct GraphStats {
    pub file_count: usize,
    pub container_count: usize,
    pub callable_count: usize,
    pub data_count: usize,
    pub contains_edges: usize,
    pub uses_edges: usize,
    pub defines_edges: usize,
    pub depends_on_edges: usize,
}

impl GraphStats {
    /// Total node count
    pub fn total_nodes(&self) -> usize {
        self.file_count + self.container_count + self.callable_count + self.data_count
    }

    /// Total edge count
    pub fn total_edges(&self) -> usize {
        self.contains_edges + self.uses_edges + self.defines_edges
    }
}

// ============================================================================
// Validation Functions
// ============================================================================

/// Validate graph structure (required fields exist)
pub fn validate_structural(graph: &PetCodeGraph) -> Vec<String> {
    let mut errors = Vec::new();

    for node in graph.iter_nodes() {
        // Check required fields are non-empty
        if node.id.is_empty() {
            errors.push("Node has empty id".to_string());
        }
        if node.name.is_empty() {
            errors.push(format!("Node {} has empty name", node.id));
        }

        // Repository nodes are virtual roots - they don't have a file path
        let is_repository = node.is_repository();
        if node.file.is_empty() && !is_repository {
            errors.push(format!("Node {} has empty file", node.id));
        }

        // Check line numbers are valid (1-indexed), except for repository nodes
        if node.line == 0 && !is_repository {
            errors.push(format!("Node {} has invalid line number 0", node.id));
        }
        if node.end_line < node.line {
            errors.push(format!(
                "Node {} has end_line ({}) < line ({})",
                node.id, node.end_line, node.line
            ));
        }

        // All nodes must have a kind
        if node.kind.is_none() {
            errors.push(format!(
                "Node {} of type {:?} missing required 'kind' field",
                node.id, node.node_type
            ));
        }
    }

    for edge in graph.iter_edges() {
        // Check edge endpoints are non-empty
        if edge.source.is_empty() {
            errors.push("Edge has empty source".to_string());
        }
        if edge.target.is_empty() {
            errors.push("Edge has empty target".to_string());
        }
    }

    errors
}

/// Validate semantic correctness (types and kinds are valid)
pub fn validate_semantic(graph: &PetCodeGraph) -> Vec<String> {
    let mut errors = Vec::new();

    // Valid kinds per node type
    let container_kinds: HashSet<&str> = [
        "repository",
        "file",
        "namespace",
        "module",
        "package",
        "type",
    ]
    .iter()
    .cloned()
    .collect();
    let callable_kinds: HashSet<&str> = ["function", "method", "constructor", "macro"]
        .iter()
        .cloned()
        .collect();
    let data_kinds: HashSet<&str> = [
        "constant",
        "value",
        "field",
        "property",
        "parameter",
        "local",
    ]
    .iter()
    .cloned()
    .collect();

    for node in graph.iter_nodes() {
        // Validate kind is correct for node type
        if let Some(ref kind) = node.kind {
            let kind_str = kind.as_str();
            match node.node_type {
                NodeType::Container => {
                    if !container_kinds.contains(kind_str) {
                        errors.push(format!(
                            "Container node {} has invalid kind '{}'. Valid: {:?}",
                            node.id, kind, container_kinds
                        ));
                    }
                }
                NodeType::Callable => {
                    if !callable_kinds.contains(kind_str) {
                        errors.push(format!(
                            "Callable node {} has invalid kind '{}'. Valid: {:?}",
                            node.id, kind, callable_kinds
                        ));
                    }
                }
                NodeType::Data => {
                    if !data_kinds.contains(kind_str) {
                        errors.push(format!(
                            "Data node {} has invalid kind '{}'. Valid: {:?}",
                            node.id, kind, data_kinds
                        ));
                    }
                }
            }
        }
    }

    errors
}

/// Validate relational integrity (edges are valid, DAG for CONTAINS)
pub fn validate_relational(graph: &PetCodeGraph) -> Vec<String> {
    let mut errors = Vec::new();

    // Collect all node IDs
    let node_ids: HashSet<&str> = graph.iter_nodes().map(|n| n.id.as_str()).collect();

    // Check all edge endpoints exist
    for edge in graph.iter_edges() {
        if !node_ids.contains(edge.source.as_str()) {
            errors.push(format!(
                "Edge source '{}' does not exist as a node",
                edge.source
            ));
        }
        if !node_ids.contains(edge.target.as_str()) {
            errors.push(format!(
                "Edge target '{}' does not exist as a node",
                edge.target
            ));
        }
    }

    // Check CONTAINS edges form a DAG (no cycles)
    // Note: iter_edges() returns owned Edge values, so we collect first then reference
    let contains_edges: Vec<_> = graph
        .iter_edges()
        .filter(|e| e.edge_type == EdgeType::Contains)
        .collect();
    let edge_refs: Vec<(&str, &str)> = contains_edges
        .iter()
        .map(|e| (e.source.as_str(), e.target.as_str()))
        .collect();

    if has_cycle(&edge_refs) {
        errors.push("CONTAINS edges form a cycle (should be a DAG)".to_string());
    }

    // Check that file nodes only receive CONTAINS edges from repository
    let file_ids: HashSet<&str> = graph
        .iter_nodes()
        .filter(|n| n.is_file())
        .map(|n| n.id.as_str())
        .collect();

    let repo_ids: HashSet<&str> = graph
        .iter_nodes()
        .filter(|n| n.is_repository())
        .map(|n| n.id.as_str())
        .collect();

    for edge in graph.iter_edges() {
        if edge.edge_type == EdgeType::Contains
            && file_ids.contains(edge.target.as_str())
            && !repo_ids.contains(edge.source.as_str())
        {
            errors.push(format!(
                "File '{}' has incoming CONTAINS edge from non-repository '{}'",
                edge.target, edge.source
            ));
        }
    }

    errors
}

/// Check if the given edges form a cycle using DFS
fn has_cycle(edges: &[(&str, &str)]) -> bool {
    // Build adjacency list
    let mut adj: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut all_nodes: HashSet<&str> = HashSet::new();

    for (src, tgt) in edges {
        adj.entry(*src).or_default().push(*tgt);
        all_nodes.insert(*src);
        all_nodes.insert(*tgt);
    }

    // DFS cycle detection
    let mut visited: HashSet<&str> = HashSet::new();
    let mut rec_stack: HashSet<&str> = HashSet::new();

    fn dfs<'a>(
        node: &'a str,
        adj: &HashMap<&'a str, Vec<&'a str>>,
        visited: &mut HashSet<&'a str>,
        rec_stack: &mut HashSet<&'a str>,
    ) -> bool {
        visited.insert(node);
        rec_stack.insert(node);

        if let Some(neighbors) = adj.get(node) {
            for neighbor in neighbors {
                if !visited.contains(neighbor) {
                    if dfs(neighbor, adj, visited, rec_stack) {
                        return true;
                    }
                } else if rec_stack.contains(neighbor) {
                    return true;
                }
            }
        }

        rec_stack.remove(node);
        false
    }

    for node in all_nodes {
        if !visited.contains(node) && dfs(node, &adj, &mut visited, &mut rec_stack) {
            return true;
        }
    }

    false
}

/// Compute graph statistics
pub fn compute_stats(graph: &PetCodeGraph) -> GraphStats {
    let mut stats = GraphStats::default();

    for node in graph.iter_nodes() {
        // Count file containers and repository containers separately
        if node.is_file() {
            stats.file_count += 1;
        } else if node.is_repository() {
            // Repository is not counted as a regular container
            // (it's a virtual root node)
        } else {
            match node.node_type {
                NodeType::Container => stats.container_count += 1,
                NodeType::Callable => stats.callable_count += 1,
                NodeType::Data => stats.data_count += 1,
            }
        }
    }

    for edge in graph.iter_edges() {
        match edge.edge_type {
            EdgeType::Contains => stats.contains_edges += 1,
            EdgeType::Uses => stats.uses_edges += 1,
            EdgeType::Defines => stats.defines_edges += 1,
            EdgeType::DependsOn => stats.depends_on_edges += 1,
        }
    }

    stats
}

/// Run all validation levels and return combined result
pub fn validate_all(graph: &PetCodeGraph) -> GraphValidationResult {
    GraphValidationResult {
        structural_errors: validate_structural(graph),
        semantic_errors: validate_semantic(graph),
        relational_errors: validate_relational(graph),
        completeness_errors: Vec::new(), // Populated separately per-test
        stats: compute_stats(graph),
    }
}

// ============================================================================
// Entity Assertion Helpers
// ============================================================================

/// Assert that an entity with the given name and type exists
pub fn assert_entity_exists(
    graph: &PetCodeGraph,
    name: &str,
    node_type: NodeType,
    kind: Option<&str>,
) {
    let found = graph.iter_nodes().find(|n| {
        n.name == name && n.node_type == node_type && (kind.is_none() || n.kind.as_deref() == kind)
    });

    assert!(
        found.is_some(),
        "Expected entity '{}' of type {:?} with kind {:?} not found",
        name,
        node_type,
        kind
    );
}

/// Assert that a CONTAINS edge exists between parent and child
pub fn assert_contains_edge(graph: &PetCodeGraph, parent_name: &str, child_name: &str) {
    // Find parent node
    let parent = graph.iter_nodes().find(|n| n.name == parent_name);
    assert!(parent.is_some(), "Parent '{}' not found", parent_name);

    // Find child node
    let child = graph.iter_nodes().find(|n| n.name == child_name);
    assert!(child.is_some(), "Child '{}' not found", child_name);

    let parent_id = &parent.unwrap().id;
    let child_id = &child.unwrap().id;

    // Find CONTAINS edge
    let edge = graph.iter_edges().find(|e| {
        e.edge_type == EdgeType::Contains && e.source == *parent_id && e.target == *child_id
    });

    assert!(
        edge.is_some(),
        "Expected CONTAINS edge from '{}' to '{}' not found",
        parent_name,
        child_name
    );
}

/// Check minimum entity counts with tolerance
pub fn check_min_counts(
    stats: &GraphStats,
    min_file: usize,
    min_container: usize,
    min_callable: usize,
    min_data: usize,
    tolerance_percent: f64,
) -> Vec<String> {
    let mut errors = Vec::new();
    let tolerance = |expected: usize| -> usize {
        ((expected as f64) * (1.0 - tolerance_percent / 100.0)).floor() as usize
    };

    if stats.file_count < tolerance(min_file) {
        errors.push(format!(
            "FILE count {} below minimum {} (with {}% tolerance)",
            stats.file_count, min_file, tolerance_percent
        ));
    }
    if stats.container_count < tolerance(min_container) {
        errors.push(format!(
            "Container count {} below minimum {} (with {}% tolerance)",
            stats.container_count, min_container, tolerance_percent
        ));
    }
    if stats.callable_count < tolerance(min_callable) {
        errors.push(format!(
            "Callable count {} below minimum {} (with {}% tolerance)",
            stats.callable_count, min_callable, tolerance_percent
        ));
    }
    if stats.data_count < tolerance(min_data) {
        errors.push(format!(
            "Data count {} below minimum {} (with {}% tolerance)",
            stats.data_count, min_data, tolerance_percent
        ));
    }

    errors
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_cycle_simple() {
        // No cycle: A -> B -> C
        let edges = vec![("A", "B"), ("B", "C")];
        assert!(!has_cycle(&edges));

        // Cycle: A -> B -> C -> A
        let edges_cycle = vec![("A", "B"), ("B", "C"), ("C", "A")];
        assert!(has_cycle(&edges_cycle));
    }

    #[test]
    fn test_has_cycle_self_loop() {
        let edges = vec![("A", "A")];
        assert!(has_cycle(&edges));
    }

    #[test]
    fn test_has_cycle_diamond() {
        // Diamond: A -> B, A -> C, B -> D, C -> D (not a cycle)
        let edges = vec![("A", "B"), ("A", "C"), ("B", "D"), ("C", "D")];
        assert!(!has_cycle(&edges));
    }
}
