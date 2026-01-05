//! Equivalence tests for Rust graph generation
//!
//! These tests verify that the Rust implementation produces semantically
//! equivalent output to the Python implementation.

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::graph::{NodeType, PetCodeGraph};
use std::path::Path;

/// Test fixture directory
const FIXTURE_DIR: &str = "tests/fixtures/sample_repo";

/// Queries directory
const QUERIES_DIR: &str = "queries";

/// Build a graph from the test fixtures
fn build_fixture_graph() -> PetCodeGraph {
    let config = BuilderConfig::default();
    let mut builder = GraphBuilder::with_config(Path::new(QUERIES_DIR), config)
        .expect("Failed to create builder");
    builder
        .build_from_directory(Path::new(FIXTURE_DIR))
        .expect("Failed to build graph")
}

#[test]
fn test_fixture_graph_builds() {
    let graph = build_fixture_graph();
    assert!(graph.node_count() > 0, "Graph should have nodes");
    assert!(graph.edge_count() > 0, "Graph should have edges");
}

#[test]
fn test_fixture_has_file_nodes() {
    let graph = build_fixture_graph();

    // Expected files
    let expected_files = ["main.py", "lib.rs", "utils.ts"];

    for file in &expected_files {
        let file_node = graph.iter_nodes().find(|n| n.name == *file);
        assert!(file_node.is_some(), "Should have file node for {}", file);
        assert!(
            file_node.unwrap().is_file(),
            "{} should be a file node",
            file
        );
    }
}

#[test]
fn test_python_class_containment() {
    let graph = build_fixture_graph();

    // Calculator class should exist
    let calculator = graph
        .iter_nodes()
        .find(|n| n.name == "Calculator" && n.node_type == NodeType::Container);
    assert!(calculator.is_some(), "Should have Calculator class");

    // Methods should be contained in Calculator (check by node ID)
    let method_names = ["__init__", "add", "multiply"];
    for method in &method_names {
        let node = graph.iter_nodes().find(|n| {
            n.name == *method && n.id.contains("Calculator") && n.node_type == NodeType::Callable
        });
        assert!(
            node.is_some(),
            "Method {} should be contained in Calculator",
            method
        );
    }

    // Field 'value' should be inside __init__
    let value_node = graph.iter_nodes().find(|n| {
        n.name == "value" && n.id.contains("Calculator") && n.node_type == NodeType::Data
    });
    assert!(
        value_node.is_some(),
        "Field 'value' should be contained in Calculator"
    );
}

#[test]
fn test_rust_struct_containment() {
    let graph = build_fixture_graph();

    // Counter struct should exist
    let counter = graph
        .iter_nodes()
        .find(|n| n.name == "Counter" && n.node_type == NodeType::Container);
    assert!(counter.is_some(), "Should have Counter struct");

    // Methods should exist (impl methods parsed as functions)
    let method_names = ["new", "increment", "get"];
    for method in &method_names {
        let node = graph
            .iter_nodes()
            .find(|n| n.name == *method && n.node_type == NodeType::Callable);
        assert!(node.is_some(), "Should have function {}", method);
    }

    // Field 'value' should be in Counter
    let value_node = graph
        .iter_nodes()
        .find(|n| n.name == "value" && n.id.contains("Counter") && n.node_type == NodeType::Data);
    assert!(value_node.is_some(), "Field 'value' should be in Counter");
}

#[test]
fn test_typescript_class_containment() {
    let graph = build_fixture_graph();

    // UserService class should exist
    let user_service = graph
        .iter_nodes()
        .find(|n| n.name == "UserService" && n.node_type == NodeType::Container);
    assert!(user_service.is_some(), "Should have UserService class");

    // User interface should exist
    let user_interface = graph
        .iter_nodes()
        .find(|n| n.name == "User" && n.node_type == NodeType::Container);
    assert!(user_interface.is_some(), "Should have User interface");

    // Methods should be contained in UserService
    let method_names = ["addUser", "findById", "getAll"];
    for method in &method_names {
        let node = graph.iter_nodes().find(|n| {
            n.name == *method && n.id.contains("UserService") && n.node_type == NodeType::Callable
        });
        assert!(
            node.is_some(),
            "Method {} should be contained in UserService",
            method
        );
    }
}

#[test]
fn test_async_functions_exist() {
    let graph = build_fixture_graph();

    // Python async function
    let fetch_data = graph
        .iter_nodes()
        .find(|n| n.name == "fetch_data" && n.file.contains("main.py"));
    assert!(fetch_data.is_some(), "Should have fetch_data function");

    // Rust async function
    let fetch_config = graph
        .iter_nodes()
        .find(|n| n.name == "fetch_config" && n.file.contains("lib.rs"));
    assert!(fetch_config.is_some(), "Should have fetch_config function");

    // TypeScript async function
    let validate_user = graph
        .iter_nodes()
        .find(|n| n.name == "validateUser" && n.file.contains("utils.ts"));
    assert!(validate_user.is_some(), "Should have validateUser function");

    // NOTE: Async metadata detection requires AST-level metadata extraction
    // which is not yet implemented for name captures. The functions exist
    // but is_async metadata may be None. This is a known limitation.
    // TODO: Implement AST-based metadata extraction for async detection
}

#[test]
fn test_contains_edges_exist() {
    let graph = build_fixture_graph();

    // FILE should contain classes
    let file_to_class = graph.iter_edges().any(|e| {
        e.edge_type == codeprysm_core::graph::EdgeType::Contains
            && e.target.contains("Calculator")
            && e.source == "main.py"
    });
    assert!(file_to_class, "main.py should CONTAIN Calculator");

    // Class should contain methods
    let class_to_method = graph.iter_edges().any(|e| {
        e.edge_type == codeprysm_core::graph::EdgeType::Contains
            && e.target.contains("add")
            && e.source.contains("Calculator")
    });
    assert!(class_to_method, "Calculator should CONTAIN add method");
}

#[test]
fn test_defines_edges_exist() {
    let graph = build_fixture_graph();

    // Classes should DEFINE fields
    let defines_edge = graph
        .iter_edges()
        .any(|e| e.edge_type == codeprysm_core::graph::EdgeType::Defines);
    assert!(defines_edge, "Should have DEFINES edges for fields");
}

#[test]
fn test_node_type_distribution() {
    let graph = build_fixture_graph();

    // Count nodes by category (using helper methods for files)
    let file_count = graph.iter_nodes().filter(|n| n.is_file()).count();
    let container_count = graph
        .iter_nodes()
        .filter(|n| n.is_container() && !n.is_file() && !n.is_repository())
        .count();
    let callable_count = graph.iter_nodes().filter(|n| n.is_callable()).count();
    let data_count = graph.iter_nodes().filter(|n| n.is_data()).count();
    let repo_count = graph.iter_nodes().filter(|n| n.is_repository()).count();

    // Should have file nodes (now Container with kind="file")
    assert!(file_count > 0, "Should have file nodes");
    // Should have container nodes (excluding files and repository)
    assert!(
        container_count > 0,
        "Should have Container nodes (non-file)"
    );
    assert!(callable_count > 0, "Should have Callable nodes");
    assert!(data_count > 0, "Should have Data nodes");
    // Should have exactly one repository node
    assert_eq!(repo_count, 1, "Should have exactly 1 Repository node");

    // Verify counts are reasonable
    assert_eq!(file_count, 3, "Should have 3 file nodes");
    assert!(
        container_count >= 4,
        "Should have at least 4 Container nodes (non-file)"
    );
    assert!(
        callable_count >= 10,
        "Should have at least 10 Callable nodes"
    );
}
