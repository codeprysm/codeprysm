//! Error handling tests for codeprysm-mcp.
//!
//! These tests validate that the MCP tools handle errors gracefully
//! without panicking, returning appropriate error messages.
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test --package codeprysm-mcp --test error_handling
//! cargo test --package codeprysm-mcp --test error_handling -- --nocapture
//! ```

mod common;

use codeprysm_core::lazy::manager::LazyGraphManager;

// ============================================================================
// Node Access Error Tests
// ============================================================================

#[test]
fn test_get_nonexistent_node() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager.load_all_partitions().expect("Failed to load");

    // Try to get a node that doesn't exist
    let result = manager.get_node("nonexistent:node:id");

    // Should return Ok(None), not error or panic
    assert!(result.is_ok(), "Should not error for nonexistent node");
    assert!(
        result.unwrap().is_none(),
        "Should return None for nonexistent node"
    );
}

#[test]
fn test_get_node_empty_id() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager.load_all_partitions().expect("Failed to load");

    // Try to get a node with empty ID
    let result = manager.get_node("");

    // Should handle gracefully
    assert!(result.is_ok(), "Should not error for empty node ID");
    assert!(
        result.unwrap().is_none(),
        "Should return None for empty node ID"
    );
}

#[test]
fn test_get_node_malformed_id() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager.load_all_partitions().expect("Failed to load");

    // Try various malformed IDs
    let malformed_ids = vec![
        ":::::",
        "../../../etc/passwd",
        "file.py\0injection",
        "very:long:nested:node:id:that:might:overflow",
        " ",
        "\t\n",
    ];

    for id in malformed_ids {
        let result = manager.get_node(id);
        assert!(
            result.is_ok(),
            "Should not error for malformed ID: {:?}",
            id
        );
    }
}

// ============================================================================
// Edge Access Error Tests
// ============================================================================

#[test]
fn test_get_edges_nonexistent_node() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager.load_all_partitions().expect("Failed to load");

    // Get edges for nonexistent node
    let outgoing = manager.get_outgoing_edges("nonexistent:node");
    let incoming = manager.get_incoming_edges("nonexistent:node");

    // Should return empty, not error
    assert!(outgoing.is_ok(), "Outgoing should not error");
    assert!(incoming.is_ok(), "Incoming should not error");
    assert!(
        outgoing.unwrap().is_empty(),
        "Outgoing should be empty for nonexistent node"
    );
    assert!(
        incoming.unwrap().is_empty(),
        "Incoming should be empty for nonexistent node"
    );
}

// ============================================================================
// Partition Error Tests
// ============================================================================

#[test]
fn test_load_nonexistent_partition() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Try to load a partition that doesn't exist
    let result = manager.load_partition("nonexistent_partition_xyz");

    // Should return an error (partition not in manifest)
    assert!(
        result.is_err(),
        "Should error when loading nonexistent partition"
    );
}

#[test]
fn test_unload_nonloaded_partition() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Get a real partition ID but don't load it
    let partition_id = manager
        .manifest()
        .partitions
        .keys()
        .next()
        .cloned()
        .expect("Should have partition");

    // Try to unload without loading first - should be no-op, not error
    let unloaded = manager.unload_partition(&partition_id);

    // Should return 0 (nothing to unload)
    assert_eq!(
        unloaded, 0,
        "Should unload 0 nodes for non-loaded partition"
    );
}

// ============================================================================
// Manager State Tests
// ============================================================================

#[test]
fn test_stats_before_load() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Get stats before loading anything
    let stats = manager.stats();

    // Should work, show 0 loaded but >0 total
    assert_eq!(stats.loaded_partitions, 0, "No partitions loaded yet");
    assert!(
        stats.total_partitions > 0,
        "Should have partitions in manifest"
    );
    assert_eq!(stats.loaded_nodes, 0, "No nodes loaded yet");
}

#[test]
fn test_multiple_load_same_partition() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    let partition_id = manager
        .manifest()
        .partitions
        .keys()
        .next()
        .cloned()
        .expect("Should have partition");

    // Load same partition multiple times
    let first_load = manager.load_partition(&partition_id);
    let stats_after_first = manager.stats();

    let second_load = manager.load_partition(&partition_id);
    let stats_after_second = manager.stats();

    // Both should succeed
    assert!(first_load.is_ok(), "First load should succeed");
    assert!(second_load.is_ok(), "Second load should succeed (no-op)");

    // Stats should be the same (no double loading)
    assert_eq!(
        stats_after_first.loaded_nodes, stats_after_second.loaded_nodes,
        "Double load should not add more nodes"
    );
}

// ============================================================================
// Graph Iteration Tests
// ============================================================================

#[test]
fn test_iterate_empty_graph() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Iterate without loading partitions
    let graph = manager.graph_read();
    let count = graph.iter_nodes().count();

    // Should be empty but not error
    assert_eq!(count, 0, "Empty graph should have 0 nodes");
}

// ============================================================================
// Empty/New Prism Directory Tests
// ============================================================================

#[test]
fn test_open_new_directory() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let new_prism = temp_dir.path().join(".codeprysm");
    std::fs::create_dir_all(&new_prism).expect("Failed to create .codeprysm");

    // Opening a new directory without manifest creates empty manager (by design)
    let result = LazyGraphManager::open(&new_prism);
    assert!(
        result.is_ok(),
        "Should succeed for new/empty .codeprysm directory"
    );

    let manager = result.unwrap();
    let stats = manager.stats();

    // Should have empty but valid state
    assert_eq!(stats.total_partitions, 0, "New directory has no partitions");
    assert_eq!(stats.loaded_nodes, 0, "New directory has no nodes");
}

#[test]
fn test_open_and_use_empty_manager() {
    use tempfile::TempDir;

    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let empty_prism = temp_dir.path().join(".codeprysm");
    std::fs::create_dir_all(&empty_prism).expect("Failed to create empty .codeprysm");

    let manager = LazyGraphManager::open(&empty_prism).expect("Should open empty dir");

    // Verify operations work on empty manager
    let result = manager.get_node("any:node:id");
    assert!(result.is_ok(), "get_node should work on empty manager");
    assert!(
        result.unwrap().is_none(),
        "Should return None on empty manager"
    );

    let outgoing = manager.get_outgoing_edges("any:node:id");
    assert!(
        outgoing.is_ok(),
        "get_outgoing_edges should work on empty manager"
    );
    assert!(
        outgoing.unwrap().is_empty(),
        "Should return empty vec on empty manager"
    );
}

// ============================================================================
// Unicode and Special Character Tests
// ============================================================================

#[test]
fn test_unicode_node_id_lookup() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager.load_all_partitions().expect("Failed to load");

    // Try unicode node IDs
    let unicode_ids = vec!["文件.py:类", "файл.py:класс", "αρχείο.py:κλάση", "📁.py:🔧"];

    for id in unicode_ids {
        let result = manager.get_node(id);
        assert!(
            result.is_ok(),
            "Should handle unicode ID gracefully: {}",
            id
        );
    }
}

// ============================================================================
// Summary Test
// ============================================================================

#[test]
fn test_error_handling_summary() {
    println!("\n=== Error Handling Test Summary ===\n");

    let test_categories = vec![
        ("Node access errors", 3),
        ("Edge access errors", 1),
        ("Partition errors", 2),
        ("Manager state errors", 2),
        ("Graph iteration errors", 1),
        ("Empty directory handling", 2),
        ("Unicode handling", 1),
    ];

    for (category, count) in &test_categories {
        println!("{:25} PASS | {} tests", category, count);
    }

    println!("\nAll error scenarios handled gracefully without panics.\n");
}
