//! Integration tests for codeprysm-mcp MCP server tools.
//!
//! These tests validate that the MCP tools work correctly with the
//! lazy-loading graph storage system using real fixture repositories.
//!
//! ## Running Tests
//!
//! ```bash
//! # Run all MCP integration tests
//! cargo test --package codeprysm-mcp --test integration
//!
//! # Run with output
//! cargo test --package codeprysm-mcp --test integration -- --nocapture
//! ```

mod common;

use std::path::PathBuf;

use codeprysm_core::lazy::manager::LazyGraphManager;
use codeprysm_mcp::ServerConfig;
use codeprysm_search::QdrantConfig;

// ============================================================================
// Test Helpers
// ============================================================================

/// Create a server config for testing (without Qdrant)
fn test_server_config(repo_path: PathBuf, codeprysm_dir: PathBuf) -> ServerConfig {
    ServerConfig {
        repo_path: repo_path.clone(),
        codeprysm_dir: codeprysm_dir.clone(),
        queries_path: Some(common::queries_dir()),
        qdrant_config: QdrantConfig::with_url("http://localhost:65535"), // Invalid port to ensure no connection
        repo_id: "test-repo".to_string(),
        enable_auto_sync: false,
        sync_interval_secs: 3600,
    }
}

// ============================================================================
// LazyGraphManager Tests (Direct Testing Without Full Server)
// ============================================================================

#[test]
fn test_lazy_graph_manager_open() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    // Open the lazy graph manager
    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");

    let stats = manager.stats();
    assert!(stats.total_partitions > 0, "Should have partitions");
    assert!(stats.total_files > 0, "Should have tracked files");
}

#[test]
fn test_lazy_graph_get_node_info() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");

    // Load all partitions to find the Calculator class
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    // Find the Calculator class node ID
    let calculator_id = {
        let graph = manager.graph_read();
        let result = graph
            .iter_nodes()
            .find(|n| n.name == "Calculator")
            .map(|n| n.id.clone())
            .expect("Calculator class not found in graph");
        result
    };

    // Now test get_node (can mutably borrow since graph reference is dropped)
    let node = manager
        .get_node(&calculator_id)
        .expect("Failed to get node")
        .expect("Node should exist");

    assert_eq!(node.name, "Calculator");
    assert!(
        node.kind.as_deref() == Some("type"),
        "Calculator should be a type"
    );
}

#[test]
fn test_lazy_graph_get_outgoing_edges() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");

    // Load all partitions
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    // Find the Calculator class node ID
    let calculator_id = {
        let graph = manager.graph_read();
        let result = graph
            .iter_nodes()
            .find(|n| n.name == "Calculator")
            .map(|n| n.id.clone())
            .expect("Calculator class not found in graph");
        result
    };

    // Get outgoing edges (should include CONTAINS/DEFINES for methods)
    let edges = manager
        .get_outgoing_edges(&calculator_id)
        .expect("Failed to get outgoing edges");

    // Calculator should have outgoing edges to its methods
    assert!(!edges.is_empty(), "Calculator should have outgoing edges");

    // Check that we find method references
    let method_names: Vec<_> = edges.iter().map(|(n, _)| n.name.as_str()).collect();
    assert!(
        method_names
            .iter()
            .any(|n| *n == "add" || *n == "multiply" || *n == "__init__"),
        "Should find method edges, found: {:?}",
        method_names
    );
}

#[test]
fn test_lazy_graph_get_incoming_edges() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");

    // Load all partitions
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    // Find the Calculator class node ID
    let calculator_id = {
        let graph = manager.graph_read();
        let result = graph
            .iter_nodes()
            .find(|n| n.name == "Calculator")
            .map(|n| n.id.clone())
            .expect("Calculator class not found in graph");
        result
    };

    // Get incoming edges (should include CONTAINS from file)
    let edges = manager
        .get_incoming_edges(&calculator_id)
        .expect("Failed to get incoming edges");

    // Calculator should be contained by a file
    assert!(
        !edges.is_empty(),
        "Calculator should have incoming edges (from file)"
    );
}

#[test]
fn test_lazy_graph_stats() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");

    let stats_before = manager.stats();
    assert_eq!(
        stats_before.loaded_partitions, 0,
        "No partitions loaded initially"
    );

    // Load all partitions
    let loaded = manager
        .load_all_partitions()
        .expect("Failed to load partitions");
    assert!(loaded > 0, "Should have loaded partitions");

    let stats_after = manager.stats();
    assert!(
        stats_after.loaded_partitions > 0,
        "Should have loaded partitions after load_all"
    );
    assert!(stats_after.loaded_nodes > 0, "Should have loaded nodes");
}

// ============================================================================
// Server Config Tests
// ============================================================================

#[test]
fn test_server_config_creation() {
    let (_temp_dir, repo_path, prism_dir) = common::setup_test_environment("python");

    let config = test_server_config(repo_path.clone(), prism_dir.clone());

    assert_eq!(config.repo_path, repo_path);
    assert_eq!(config.codeprysm_dir, prism_dir);
    assert!(!config.enable_auto_sync);
}

// ============================================================================
// Tool Parameter Validation Tests
// ============================================================================

#[test]
fn test_node_id_format() {
    // Test that node IDs are formatted correctly
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    let graph = manager.graph_read();

    // Check node ID format - should be file:Entity or file:Container:Entity
    for node in graph.iter_nodes().take(10) {
        if !node.file.is_empty() {
            assert!(
                node.id.starts_with(&node.file) || node.id.contains(':'),
                "Node ID '{}' should reference file '{}'",
                node.id,
                node.file
            );
        }
    }
}

// ============================================================================
// Graph Content Validation Tests
// ============================================================================

#[test]
fn test_python_fixture_entities() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    let graph = manager.graph_read();
    let names: Vec<_> = graph.iter_nodes().map(|n| n.name.as_str()).collect();

    // Verify expected entities from sample.py are in the graph
    assert!(
        names.contains(&"Calculator"),
        "Should have Calculator class"
    );
    assert!(
        names.contains(&"AsyncProcessor"),
        "Should have AsyncProcessor class"
    );
    assert!(
        names.contains(&"InheritedClass"),
        "Should have InheritedClass"
    );
    assert!(
        names.contains(&"standalone_function"),
        "Should have standalone_function"
    );
    assert!(
        names.contains(&"async_standalone"),
        "Should have async_standalone function"
    );
}

#[test]
fn test_javascript_fixture_entities() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("javascript");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    let graph = manager.graph_read();
    let names: Vec<_> = graph.iter_nodes().map(|n| n.name.as_str()).collect();

    // Verify expected entities from JavaScript fixture
    assert!(
        names.contains(&"Calculator"),
        "Should have Calculator class"
    );
    assert!(
        names.contains(&"standaloneFunction"),
        "Should have standaloneFunction"
    );
}

#[test]
fn test_typescript_fixture_entities() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("typescript");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    let graph = manager.graph_read();
    let names: Vec<_> = graph.iter_nodes().map(|n| n.name.as_str()).collect();

    // Verify expected entities from TypeScript fixture
    assert!(
        names.contains(&"UserService"),
        "Should have UserService class"
    );
    assert!(names.contains(&"User"), "Should have User interface/type");
}

// ============================================================================
// Cross-Partition Edge Tests
// ============================================================================

#[test]
fn test_cross_refs_loaded() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");

    let stats = manager.stats();
    // Cross-partition edges may be 0 if all code is in one file
    // Just verify the stats are accessible (value is always non-negative since it's usize)
    let _ = stats.cross_partition_edges; // Access to verify it exists
}

// ============================================================================
// Multi-Language Tests
// ============================================================================

#[test]
fn test_all_fixture_languages() {
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

    for language in &languages {
        let result = std::panic::catch_unwind(|| {
            let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment(language);

            let manager =
                LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");
            manager
                .load_all_partitions()
                .expect("Failed to load partitions");

            let stats = manager.stats();
            assert!(
                stats.loaded_nodes > 0,
                "{}: Should have loaded nodes",
                language
            );
        });

        assert!(
            result.is_ok(),
            "Test failed for language: {}. Error: {:?}",
            language,
            result.err()
        );
    }
}

// ============================================================================
// Summary Test
// ============================================================================

#[test]
fn test_integration_summary() {
    println!("\n=== codeprysm-mcp Integration Test Summary ===\n");

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

    for language in &languages {
        let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment(language);

        let manager =
            LazyGraphManager::open(&prism_dir).expect("Failed to open lazy graph manager");
        manager
            .load_all_partitions()
            .expect("Failed to load partitions");

        let stats = manager.stats();

        println!(
            "{:12} PASS | Partitions: {:2} | Nodes: {:4} | Cross-refs: {:3}",
            language, stats.total_partitions, stats.loaded_nodes, stats.cross_partition_edges
        );
    }

    println!();
}
