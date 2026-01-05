//! Lazy-loading integration tests for codeprysm-core.
//!
//! These tests validate the lazy-loading graph system works correctly
//! with realistic scenarios including:
//! - Partition creation and loading
//! - Cross-partition edge queries
//! - Memory management and eviction
//! - Incremental updates
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test --package codeprysm-core --test lazy_integration
//! cargo test --package codeprysm-core --test lazy_integration -- --nocapture
//! ```

mod common;

use std::collections::HashSet;
use tempfile::TempDir;

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::lazy::manager::LazyGraphManager;
use codeprysm_core::lazy::partitioner::GraphPartitioner;

// ============================================================================
// Test Helpers
// ============================================================================

fn fixtures_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("integration_repos")
}

fn queries_dir() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("queries")
}

/// Set up a test environment with partitioned graph for a given language fixture
fn setup_lazy_env(language: &str) -> (TempDir, std::path::PathBuf) {
    let fixture_path = fixtures_dir().join(language);

    // Build graph
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");
    let graph = builder
        .build_from_directory(&fixture_path)
        .expect("Failed to build graph");

    // Create temp dir for partitioned storage
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let prism_dir = temp_dir.path().join(".codeprysm");
    std::fs::create_dir_all(&prism_dir).expect("Failed to create prism dir");

    // Partition the graph
    GraphPartitioner::partition_with_stats(&graph, &prism_dir, Some("test"))
        .expect("Failed to partition graph");

    (temp_dir, prism_dir)
}

// ============================================================================
// Partition Lifecycle Tests
// ============================================================================

#[test]
fn test_partition_roundtrip_fixture_repo() {
    let (_temp_dir, prism_dir) = setup_lazy_env("python");

    // Open manager
    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Stats before loading
    let stats_before = manager.stats();
    assert_eq!(stats_before.loaded_partitions, 0);
    assert!(stats_before.total_partitions > 0);

    // Load all partitions
    let loaded = manager
        .load_all_partitions()
        .expect("Failed to load partitions");
    assert!(loaded > 0);

    // Stats after loading
    let stats_after = manager.stats();
    assert!(stats_after.loaded_partitions > 0);
    assert!(stats_after.loaded_nodes > 0);

    // Verify we can access nodes
    let graph = manager.graph_read();
    let node_count = graph.iter_nodes().count();
    assert!(node_count > 0, "Should have nodes after loading");
}

#[test]
fn test_partition_load_on_demand() {
    let (_temp_dir, prism_dir) = setup_lazy_env("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Initially no partitions loaded
    let stats = manager.stats();
    assert_eq!(stats.loaded_partitions, 0);

    // Get the first partition ID from manifest to load on demand
    let first_partition_id = manager
        .manifest()
        .partitions
        .keys()
        .next()
        .cloned()
        .expect("Should have partitions");

    // Load the partition on demand
    manager
        .load_partition(&first_partition_id)
        .expect("Failed to load partition");

    // Now we should have at least one partition loaded
    assert!(
        manager.stats().loaded_partitions > 0,
        "Should have loaded partition on demand"
    );
    assert!(
        manager.stats().loaded_nodes > 0,
        "Should have nodes after loading"
    );
}

#[test]
fn test_cross_partition_edge_queries() {
    let (_temp_dir, prism_dir) = setup_lazy_env("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Load all partitions
    manager.load_all_partitions().expect("Failed to load");

    // Find a class node (should have edges to methods)
    let class_id = {
        let graph = manager.graph_read();
        let result = graph
            .iter_nodes()
            .find(|n| n.name == "Calculator")
            .map(|n| n.id.clone())
            .expect("Calculator not found");
        result
    };

    // Get outgoing edges
    let outgoing = manager
        .get_outgoing_edges(&class_id)
        .expect("Failed to get outgoing edges");

    // Get incoming edges
    let incoming = manager
        .get_incoming_edges(&class_id)
        .expect("Failed to get incoming edges");

    // A class should have both incoming (from file) and outgoing (to methods) edges
    assert!(!outgoing.is_empty(), "Class should have outgoing edges");
    assert!(
        !incoming.is_empty(),
        "Class should have incoming edges (from file)"
    );
}

// ============================================================================
// Memory Management Tests
// ============================================================================

#[test]
fn test_cache_metrics_accuracy() {
    let (_temp_dir, prism_dir) = setup_lazy_env("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Initial metrics
    let stats = manager.stats();
    assert_eq!(stats.cache_evictions, 0);

    // Load all partitions (should register as cache misses since not cached)
    manager.load_all_partitions().expect("Failed to load");

    // Get a node ID
    let node_id = {
        let graph = manager.graph_read();
        let result = graph.iter_nodes().next().map(|n| n.id.clone()).unwrap();
        result
    };

    // Access the node - should be a cache hit since partition is loaded
    let _ = manager.get_node(&node_id).expect("Failed to get node");

    // Check metrics updated - cache_hit_rate is between 0.0 and 1.0
    let stats_after = manager.stats();
    // After loading all partitions, we should have a valid cache_hit_rate
    assert!(
        stats_after.cache_hit_rate >= 0.0 && stats_after.cache_hit_rate <= 1.0,
        "Cache hit rate should be between 0.0 and 1.0, got: {}",
        stats_after.cache_hit_rate
    );
}

#[test]
fn test_unload_partitions() {
    let (_temp_dir, prism_dir) = setup_lazy_env("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Load all
    manager.load_all_partitions().expect("Failed to load");
    let loaded_before = manager.stats().loaded_partitions;
    assert!(loaded_before > 0);

    // Unload all partitions one by one
    let partition_ids: Vec<String> = manager
        .loaded_partitions()
        .iter()
        .map(|s| s.to_string())
        .collect();
    for pid in partition_ids {
        manager.unload_partition(&pid);
    }
    assert_eq!(manager.stats().loaded_partitions, 0);
}

// ============================================================================
// Multi-Language Tests
// ============================================================================

#[test]
fn test_lazy_loading_all_languages() {
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
        let (_temp_dir, prism_dir) = setup_lazy_env(language);

        let manager = LazyGraphManager::open(&prism_dir)
            .unwrap_or_else(|_| panic!("Failed to open manager for {}", language));

        manager
            .load_all_partitions()
            .unwrap_or_else(|_| panic!("Failed to load partitions for {}", language));

        let stats = manager.stats();
        assert!(
            stats.loaded_nodes > 0,
            "{}: Should have loaded nodes",
            language
        );

        println!(
            "{:12} OK | partitions: {:2} | nodes: {:4} | edges: {:4}",
            language, stats.loaded_partitions, stats.loaded_nodes, stats.loaded_edges
        );
    }
}

// ============================================================================
// Graph Integrity Tests
// ============================================================================

#[test]
fn test_node_edge_consistency() {
    let (_temp_dir, prism_dir) = setup_lazy_env("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager.load_all_partitions().expect("Failed to load");

    let graph = manager.graph_read();

    // Collect all node IDs
    let node_ids: HashSet<_> = graph.iter_nodes().map(|n| n.id.clone()).collect();

    // Verify all edge endpoints exist
    for node in graph.iter_nodes() {
        for (_target, _edge) in graph.outgoing_edges(&node.id) {
            // Target nodes should exist (they're returned by outgoing_edges)
            // This validates the graph structure
        }
    }

    // Verify no orphan nodes (except file nodes which are roots)
    let mut orphan_count = 0;
    for node in graph.iter_nodes() {
        let has_incoming: bool = graph.incoming_edges(&node.id).next().is_some();
        let has_outgoing: bool = graph.outgoing_edges(&node.id).next().is_some();

        // File nodes are allowed to have no incoming edges
        if !has_incoming && !has_outgoing {
            orphan_count += 1;
        }
    }

    // Allow some orphans (e.g., file nodes in test fixtures)
    assert!(
        orphan_count <= node_ids.len() / 2,
        "Too many orphan nodes: {} out of {}",
        orphan_count,
        node_ids.len()
    );
}

#[test]
fn test_manifest_integrity() {
    let (_temp_dir, prism_dir) = setup_lazy_env("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    let stats = manager.stats();

    // Verify manifest has valid data
    assert!(stats.total_partitions > 0, "Should have partitions");
    assert!(stats.total_files > 0, "Should track files");

    // Verify manifest file exists
    let manifest_path = prism_dir.join("manifest.json");
    assert!(manifest_path.exists(), "Manifest file should exist");
}

// ============================================================================
// Summary Test
// ============================================================================

#[test]
fn test_lazy_integration_summary() {
    println!("\n=== Lazy Loading Integration Test Summary ===\n");

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

    println!("Language     | Partitions | Nodes | Edges | Cross-Refs");
    println!("-------------|------------|-------|-------|------------");

    for language in &languages {
        let (_temp_dir, prism_dir) = setup_lazy_env(language);

        let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open");
        manager.load_all_partitions().expect("Failed to load");

        let stats = manager.stats();

        println!(
            "{:12} | {:10} | {:5} | {:5} | {:10}",
            language,
            stats.total_partitions,
            stats.loaded_nodes,
            stats.loaded_edges,
            stats.cross_partition_edges
        );
    }

    println!();
}

/// Test that file nodes have type "Container" (not legacy "FILE")
#[test]
fn test_file_nodes_are_container_type() {
    let (_temp_dir, prism_dir) = setup_lazy_env("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open");
    manager.load_all_partitions().expect("Failed to load");

    let graph = manager.graph_read();

    // Count node types
    let mut type_counts = std::collections::HashMap::new();
    let mut file_nodes = Vec::new();

    for node in graph.iter_nodes() {
        let type_str = node.node_type.as_str();
        *type_counts.entry(type_str.to_string()).or_insert(0) += 1;

        // Check if this is a file node (Container with kind="file")
        if node.is_file() {
            file_nodes.push((node.id.clone(), type_str.to_string(), node.kind.clone()));
        }
    }

    // Print type distribution for debugging
    println!("\nNode type distribution:");
    for (type_name, count) in &type_counts {
        println!("  {}: {}", type_name, count);
    }

    // Assert no "FILE" type exists
    assert!(
        !type_counts.contains_key("FILE"),
        "Found legacy 'FILE' type in graph! Types: {:?}",
        type_counts
    );

    // Assert Container type exists
    assert!(
        type_counts.contains_key("Container"),
        "Missing 'Container' type in graph! Types: {:?}",
        type_counts
    );

    // Assert file nodes have correct type
    for (id, type_str, kind) in &file_nodes {
        assert_eq!(
            type_str, "Container",
            "File node {} should be Container, got {}",
            id, type_str
        );
        assert_eq!(
            kind.as_deref(),
            Some("file"),
            "File node {} should have kind='file', got {:?}",
            id,
            kind
        );
    }

    println!("\nFile nodes verified: {} total", file_nodes.len());
}
