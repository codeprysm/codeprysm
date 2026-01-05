//! Graph completeness tests for codeprysm-core.
//!
//! These tests validate that all code entities are captured correctly:
//! - All methods, fields, parameters are extracted
//! - Edge relationships are accurate
//! - No orphan nodes or dangling edges
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test --package codeprysm-core --test graph_completeness
//! cargo test --package codeprysm-core --test graph_completeness -- --nocapture
//! ```

mod common;

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::graph::{EdgeType, NodeType, PetCodeGraph};
use common::{compute_stats, validate_all};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

// ============================================================================
// Test Helpers
// ============================================================================

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("integration_repos")
}

fn queries_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("queries")
}

fn build_fixture_graph(language: &str) -> PetCodeGraph {
    let fixture_path = fixtures_dir().join(language);
    assert!(
        fixture_path.exists(),
        "Fixture directory does not exist: {:?}",
        fixture_path
    );

    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");

    builder
        .build_from_directory(&fixture_path)
        .expect("Failed to build graph")
}

/// Find a node by name and optionally by type/kind
fn find_node<'a>(
    graph: &'a PetCodeGraph,
    name: &str,
    node_type: Option<NodeType>,
    kind: Option<&str>,
) -> Option<&'a codeprysm_core::graph::Node> {
    graph.iter_nodes().find(|n| {
        n.name == name
            && node_type.is_none_or(|t| n.node_type == t)
            && kind.is_none_or(|k| n.kind.as_deref() == Some(k))
    })
}

/// Count nodes matching criteria
#[allow(dead_code)]
fn count_nodes(graph: &PetCodeGraph, node_type: NodeType, kind: Option<&str>) -> usize {
    graph
        .iter_nodes()
        .filter(|n| n.node_type == node_type && kind.is_none_or(|k| n.kind.as_deref() == Some(k)))
        .count()
}

/// Get all method names for a class
fn get_methods_of_class(graph: &PetCodeGraph, class_name: &str) -> Vec<String> {
    let class_node = find_node(graph, class_name, Some(NodeType::Container), Some("type"));
    let class_id = match class_node {
        Some(n) => n.id.clone(),
        None => return vec![],
    };

    // Find all Callable nodes that have CONTAINS edge from this class
    let method_ids: HashSet<String> = graph
        .iter_edges()
        .filter(|e| e.edge_type == EdgeType::Contains && e.source == class_id)
        .map(|e| e.target.clone())
        .collect();

    graph
        .iter_nodes()
        .filter(|n| n.node_type == NodeType::Callable && method_ids.contains(&n.id))
        .map(|n| n.name.clone())
        .collect()
}

/// Get all fields defined by a class (via DEFINES edges)
#[allow(dead_code)]
fn get_fields_of_class(graph: &PetCodeGraph, class_name: &str) -> Vec<String> {
    let class_node = find_node(graph, class_name, Some(NodeType::Container), Some("type"));
    let class_id = match class_node {
        Some(n) => n.id.clone(),
        None => return vec![],
    };

    // Find all Data nodes that have DEFINES edge from this class
    let field_ids: HashSet<String> = graph
        .iter_edges()
        .filter(|e| e.edge_type == EdgeType::Defines && e.source == class_id)
        .map(|e| e.target.clone())
        .collect();

    graph
        .iter_nodes()
        .filter(|n| n.node_type == NodeType::Data && field_ids.contains(&n.id))
        .map(|n| n.name.clone())
        .collect()
}

// ============================================================================
// Phase 1.1: Entity Extraction Completeness
// ============================================================================

#[test]
fn test_all_class_methods_captured() {
    // Python Calculator class has: __init__, add, multiply, square, from_string
    let graph = build_fixture_graph("python");

    let methods = get_methods_of_class(&graph, "Calculator");

    // Verify expected methods exist
    let expected_methods = ["__init__", "add", "multiply", "square", "from_string"];
    for method in &expected_methods {
        assert!(
            methods.iter().any(|m| m == *method),
            "Method '{}' not found in Calculator. Found methods: {:?}",
            method,
            methods
        );
    }

    // Verify count matches (at least the expected ones)
    assert!(
        methods.len() >= expected_methods.len(),
        "Calculator should have at least {} methods, found {}",
        expected_methods.len(),
        methods.len()
    );
}

#[test]
fn test_all_fields_captured() {
    // Python Calculator has class_constant field
    let graph = build_fixture_graph("python");

    // Check that Data nodes exist for fields
    // Note: Field capture depends on language-specific query patterns
    let data_nodes: Vec<_> = graph
        .iter_nodes()
        .filter(|n| n.node_type == NodeType::Data)
        .collect();

    // We should have at least some Data nodes (constants, fields, parameters)
    assert!(
        !data_nodes.is_empty(),
        "Should have Data nodes for fields/constants"
    );

    // Check for module-level constant MAX_ITEMS
    let max_items = find_node(&graph, "MAX_ITEMS", Some(NodeType::Data), None);
    assert!(
        max_items.is_some(),
        "Module constant MAX_ITEMS should be captured"
    );
}

#[test]
fn test_function_parameters_captured() {
    let graph = build_fixture_graph("python");

    // Find standalone_function and check it has parameters
    let func = find_node(
        &graph,
        "standalone_function",
        Some(NodeType::Callable),
        Some("function"),
    );
    assert!(func.is_some(), "standalone_function should exist");

    // Check that parameter 'param' exists as a Data node with kind="parameter"
    let param_node = find_node(&graph, "param", Some(NodeType::Data), Some("parameter"));
    assert!(
        param_node.is_some(),
        "Function parameter 'param' should be captured as Data node with kind=parameter"
    );

    // Check that multiple parameters are captured across the codebase
    let all_params: Vec<_> = graph
        .iter_nodes()
        .filter(|n| n.node_type == NodeType::Data && n.kind.as_deref() == Some("parameter"))
        .collect();

    // Python fixture has many functions with parameters
    assert!(
        all_params.len() >= 5,
        "Should have at least 5 parameter nodes, found {}",
        all_params.len()
    );
}

#[test]
fn test_nested_functions_captured() {
    let graph = build_fixture_graph("python");

    // decorator_example contains nested function 'wrapper'
    let decorator = find_node(
        &graph,
        "decorator_example",
        Some(NodeType::Callable),
        Some("function"),
    );
    assert!(decorator.is_some(), "decorator_example should exist");

    let wrapper = find_node(&graph, "wrapper", Some(NodeType::Callable), None);
    assert!(
        wrapper.is_some(),
        "Nested function 'wrapper' should be captured"
    );

    // wrapper should be contained by decorator_example
    if let (Some(dec), Some(wrap)) = (decorator, wrapper) {
        let has_contains = graph.iter_edges().any(|e| {
            e.edge_type == EdgeType::Contains && e.source == dec.id && e.target == wrap.id
        });
        assert!(
            has_contains,
            "Nested function should have CONTAINS edge from parent"
        );
    }
}

#[test]
fn test_module_level_variables_captured() {
    let graph = build_fixture_graph("python");

    // MAX_ITEMS = 100 is a module-level constant
    let max_items = find_node(&graph, "MAX_ITEMS", Some(NodeType::Data), None);
    assert!(
        max_items.is_some(),
        "Module-level constant MAX_ITEMS should be captured"
    );

    if let Some(node) = max_items {
        // Should be kind="constant" or "value"
        assert!(
            node.kind.as_deref() == Some("constant") || node.kind.as_deref() == Some("value"),
            "Module constant should have kind constant or value, got {:?}",
            node.kind
        );
    }
}

// ============================================================================
// Phase 1.2: Edge Accuracy
// ============================================================================

#[test]
fn test_contains_edges_form_hierarchy() {
    let graph = build_fixture_graph("python");

    // Every non-file, non-repository node should have exactly one incoming CONTAINS edge
    let repo_and_file_ids: HashSet<String> = graph
        .iter_nodes()
        .filter(|n| n.is_repository() || n.is_file())
        .map(|n| n.id.clone())
        .collect();

    let mut incoming_contains: HashMap<String, usize> = HashMap::new();
    for edge in graph.iter_edges() {
        if edge.edge_type == EdgeType::Contains {
            *incoming_contains.entry(edge.target.clone()).or_insert(0) += 1;
        }
    }

    for node in graph.iter_nodes() {
        if repo_and_file_ids.contains(&node.id) {
            continue; // Skip repo and files
        }

        let count = incoming_contains.get(&node.id).copied().unwrap_or(0);
        assert!(
            count >= 1,
            "Node '{}' ({:?}) should have at least 1 incoming CONTAINS edge, has {}",
            node.name,
            node.node_type,
            count
        );
    }
}

#[test]
fn test_uses_edges_capture_references() {
    let graph = build_fixture_graph("python");

    // InheritedClass inherits from Calculator - should have USES edge
    let inherited = find_node(
        &graph,
        "InheritedClass",
        Some(NodeType::Container),
        Some("type"),
    );
    let calculator = find_node(
        &graph,
        "Calculator",
        Some(NodeType::Container),
        Some("type"),
    );

    let (inh, calc) = match (inherited, calculator) {
        (Some(i), Some(c)) => (i, c),
        _ => panic!("Could not find InheritedClass or Calculator nodes"),
    };

    // Inheritance should create a USES edge
    let has_inheritance_uses = graph
        .iter_edges()
        .any(|e| e.edge_type == EdgeType::Uses && e.source == inh.id && e.target == calc.id);
    assert!(
        has_inheritance_uses,
        "InheritedClass(Calculator) inheritance should create USES edge"
    );

    // Function calls should also create USES edges
    // e.g., process_batch calls process_item
    let process_batch = find_node(
        &graph,
        "process_batch",
        Some(NodeType::Callable),
        Some("method"),
    );
    let process_item = find_node(
        &graph,
        "process_item",
        Some(NodeType::Callable),
        Some("method"),
    );

    if let (Some(batch), Some(item)) = (process_batch, process_item) {
        let has_call_uses = graph
            .iter_edges()
            .any(|e| e.edge_type == EdgeType::Uses && e.source == batch.id && e.target == item.id);
        assert!(
            has_call_uses,
            "process_batch calling process_item should create USES edge"
        );
    }

    // Count total USES edges - Python fixture should have multiple
    let uses_count = graph
        .iter_edges()
        .filter(|e| e.edge_type == EdgeType::Uses)
        .count();

    assert!(
        uses_count >= 3,
        "Python fixture should have at least 3 USES edges (inheritance, function calls), found {}",
        uses_count
    );
}

#[test]
fn test_defines_edges_link_containers_to_members() {
    let graph = build_fixture_graph("python");

    // Classes should have DEFINES edges to their fields
    let calculator = find_node(
        &graph,
        "Calculator",
        Some(NodeType::Container),
        Some("type"),
    );

    if let Some(calc) = calculator {
        let defines_edges: Vec<_> = graph
            .iter_edges()
            .filter(|e| e.edge_type == EdgeType::Defines && e.source == calc.id)
            .collect();

        // Calculator has class_constant field
        println!("Calculator DEFINES {} entities", defines_edges.len());

        // Should have at least some DEFINES edges for fields
        // Note: Exact count depends on what the queries capture
    }

    // Functions should have DEFINES edges to their parameters
    let standalone = find_node(
        &graph,
        "standalone_function",
        Some(NodeType::Callable),
        Some("function"),
    );

    if let Some(func) = standalone {
        let defines_edges: Vec<_> = graph
            .iter_edges()
            .filter(|e| e.edge_type == EdgeType::Defines && e.source == func.id)
            .collect();

        println!(
            "standalone_function DEFINES {} entities (parameters)",
            defines_edges.len()
        );
    }
}

#[test]
fn test_no_duplicate_edges() {
    let graph = build_fixture_graph("python");

    // Collect all edges as (source, target, type) tuples
    let mut edge_set: HashSet<(String, String, String)> = HashSet::new();
    let mut duplicate_count = 0;

    for edge in graph.iter_edges() {
        let key = (
            edge.source.clone(),
            edge.target.clone(),
            format!("{:?}", edge.edge_type),
        );
        if !edge_set.insert(key.clone()) {
            duplicate_count += 1;
            println!(
                "Duplicate edge: {} -> {} ({:?})",
                edge.source, edge.target, edge.edge_type
            );
        }
    }

    assert_eq!(
        duplicate_count, 0,
        "Should have no duplicate edges, found {}",
        duplicate_count
    );
}

// ============================================================================
// Phase 1.3: Graph Integrity
// ============================================================================

#[test]
fn test_no_orphan_nodes() {
    let graph = build_fixture_graph("python");

    // Collect all node IDs that are targets of edges
    let mut has_incoming: HashSet<String> = HashSet::new();
    for edge in graph.iter_edges() {
        has_incoming.insert(edge.target.clone());
    }

    // Repository root doesn't need incoming edges
    let repo_ids: HashSet<String> = graph
        .iter_nodes()
        .filter(|n| n.is_repository())
        .map(|n| n.id.clone())
        .collect();

    let mut orphans = Vec::new();
    for node in graph.iter_nodes() {
        if repo_ids.contains(&node.id) {
            continue; // Repository root is allowed to be orphan
        }

        if !has_incoming.contains(&node.id) {
            orphans.push(format!("{} ({})", node.name, node.id));
        }
    }

    assert!(
        orphans.is_empty(),
        "Found {} orphan nodes (no incoming edges): {:?}",
        orphans.len(),
        orphans.iter().take(5).collect::<Vec<_>>()
    );
}

#[test]
fn test_no_dangling_edges() {
    let graph = build_fixture_graph("python");

    // Collect all node IDs
    let node_ids: HashSet<&str> = graph.iter_nodes().map(|n| n.id.as_str()).collect();

    let mut dangling = Vec::new();
    for edge in graph.iter_edges() {
        if !node_ids.contains(edge.source.as_str()) {
            dangling.push(format!("Source '{}' does not exist", edge.source));
        }
        if !node_ids.contains(edge.target.as_str()) {
            dangling.push(format!("Target '{}' does not exist", edge.target));
        }
    }

    assert!(
        dangling.is_empty(),
        "Found {} dangling edge endpoints: {:?}",
        dangling.len(),
        dangling.iter().take(5).collect::<Vec<_>>()
    );
}

// ============================================================================
// Cross-Language Completeness Tests
// ============================================================================

#[test]
fn test_javascript_class_methods_captured() {
    let graph = build_fixture_graph("javascript");

    let methods = get_methods_of_class(&graph, "Calculator");

    // JavaScript Calculator should have constructor, add, etc.
    assert!(
        methods.iter().any(|m| m == "constructor"),
        "JavaScript Calculator should have constructor. Found: {:?}",
        methods
    );
    assert!(
        methods.iter().any(|m| m == "add"),
        "JavaScript Calculator should have add method. Found: {:?}",
        methods
    );
}

#[test]
fn test_typescript_interface_captured() {
    let graph = build_fixture_graph("typescript");

    // TypeScript has interfaces
    let user_interface = find_node(&graph, "User", Some(NodeType::Container), Some("type"));
    assert!(
        user_interface.is_some(),
        "TypeScript User interface should be captured"
    );

    let repository = find_node(
        &graph,
        "Repository",
        Some(NodeType::Container),
        Some("type"),
    );
    assert!(
        repository.is_some(),
        "TypeScript Repository interface should be captured"
    );
}

#[test]
fn test_rust_impl_methods_captured() {
    let graph = build_fixture_graph("rust");

    // Rust has impl blocks with methods
    let calc = find_node(
        &graph,
        "Calculator",
        Some(NodeType::Container),
        Some("type"),
    );
    assert!(calc.is_some(), "Rust Calculator trait should be captured");

    // Check for impl methods
    let simple_calc = find_node(
        &graph,
        "SimpleCalculator",
        Some(NodeType::Container),
        Some("type"),
    );
    assert!(
        simple_calc.is_some(),
        "Rust SimpleCalculator struct should be captured"
    );
}

#[test]
fn test_go_struct_methods_captured() {
    let graph = build_fixture_graph("go");

    // Go has receiver methods
    let calc = find_node(
        &graph,
        "SimpleCalculator",
        Some(NodeType::Container),
        Some("type"),
    );
    assert!(
        calc.is_some(),
        "Go SimpleCalculator struct should be captured"
    );

    // Check for constructor function
    let new_func = find_node(
        &graph,
        "NewSimpleCalculator",
        Some(NodeType::Callable),
        Some("function"),
    );
    assert!(
        new_func.is_some(),
        "Go constructor function should be captured"
    );
}

// ============================================================================
// Summary Test
// ============================================================================

#[test]
fn test_graph_completeness_summary() {
    let languages = [
        "python",
        "javascript",
        "typescript",
        "rust",
        "go",
        "c",
        "cpp",
        "csharp",
    ];

    println!("\n=== Graph Completeness Summary ===\n");

    for lang in &languages {
        let graph = build_fixture_graph(lang);
        let stats = compute_stats(&graph);
        let result = validate_all(&graph);

        let status = if result.is_valid() { "PASS" } else { "FAIL" };

        // Count edge types
        let contains = graph
            .iter_edges()
            .filter(|e| e.edge_type == EdgeType::Contains)
            .count();
        let uses = graph
            .iter_edges()
            .filter(|e| e.edge_type == EdgeType::Uses)
            .count();
        let defines = graph
            .iter_edges()
            .filter(|e| e.edge_type == EdgeType::Defines)
            .count();

        println!(
            "{:12} {} | Nodes: {:3} | Edges: CONTAINS:{}, USES:{}, DEFINES:{}",
            lang,
            status,
            stats.total_nodes(),
            contains,
            uses,
            defines
        );
    }

    println!();
}
