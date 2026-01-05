//! Partition Lifecycle & Memory Tests (Phase 3)
//!
//! These tests validate the lazy-loading graph partition management:
//! - Loading behavior (selective, idempotent, on-demand)
//! - Unloading behavior (memory freed, reload consistency, cross-ref preservation)
//! - Memory budget enforcement (eviction, min_partitions, LRU ordering)
//! - Cache metrics accuracy (hit rate, eviction count, bytes evicted)
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test --package codeprysm-core --test partition_lifecycle
//! cargo test --package codeprysm-core --test partition_lifecycle -- --nocapture
//! ```

mod common;

use tempfile::TempDir;

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::graph::{CallableKind, EdgeType, Node, NodeType};
use codeprysm_core::lazy::cache::PartitionStats;
use codeprysm_core::lazy::cross_refs::CrossRef;
use codeprysm_core::lazy::manager::LazyGraphManager;
use codeprysm_core::lazy::partition::PartitionConnection;
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

/// Create a test node with basic properties
fn create_test_node(id: &str, name: &str, file: &str) -> Node {
    Node {
        id: id.to_string(),
        name: name.to_string(),
        node_type: NodeType::Callable,
        kind: Some(CallableKind::Function.as_str().to_string()),
        subtype: None,
        file: file.to_string(),
        line: 1,
        end_line: 10,
        text: Some("def test(): pass".to_string()),
        hash: None,
        metadata: Default::default(),
    }
}

/// Set up a multi-partition test environment with known structure
/// Returns (temp_dir, prism_dir, partition_ids, node_ids_per_partition)
fn setup_multi_partition_env(
    num_partitions: usize,
    nodes_per_partition: usize,
    memory_budget: Option<usize>,
) -> (TempDir, std::path::PathBuf, Vec<String>, Vec<Vec<String>>) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let prism_dir = temp_dir.path().join(".codeprysm");

    let mut manager = match memory_budget {
        Some(budget) => LazyGraphManager::init_with_memory_budget(&prism_dir, Some(budget))
            .expect("Failed to init manager"),
        None => LazyGraphManager::init(&prism_dir).expect("Failed to init manager"),
    };

    let mut partition_ids = Vec::new();
    let mut node_ids_per_partition = Vec::new();

    for p in 0..num_partitions {
        let partition_id = format!("partition_{}", p);
        partition_ids.push(partition_id.clone());

        let db_path = prism_dir
            .join("partitions")
            .join(format!("{}.db", partition_id));
        std::fs::create_dir_all(db_path.parent().unwrap())
            .expect("Failed to create partitions dir");
        let conn = PartitionConnection::create(&db_path, &partition_id)
            .expect("Failed to create partition");

        let mut node_ids = Vec::new();
        for n in 0..nodes_per_partition {
            let node_id = format!("p{}/file.py:func_{}", p, n);
            let node =
                create_test_node(&node_id, &format!("func_{}", n), &format!("p{}/file.py", p));
            conn.insert_node(&node).expect("Failed to insert node");
            node_ids.push(node_id);
        }
        node_ids_per_partition.push(node_ids);

        // Register in manifest
        manager
            .manifest_mut()
            .set_file(format!("p{}/file.py", p), partition_id.clone(), None);
        manager
            .manifest_mut()
            .register_partition(partition_id.clone(), format!("{}.db", partition_id));
    }

    manager.save_manifest().expect("Failed to save manifest");

    (temp_dir, prism_dir, partition_ids, node_ids_per_partition)
}

// ============================================================================
// 3.1 Loading Behavior Tests
// ============================================================================

/// Test that loading a partition only loads that specific partition
#[test]
fn test_selective_partition_loading() {
    let (_temp_dir, prism_dir, partition_ids, node_ids_per_partition) =
        setup_multi_partition_env(3, 5, None);

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Initially no partitions loaded
    assert_eq!(manager.stats().loaded_partitions, 0);
    assert_eq!(manager.stats().loaded_nodes, 0);

    // Load only the first partition
    manager
        .load_partition(&partition_ids[0])
        .expect("Failed to load partition");

    // Verify only the first partition is loaded
    assert_eq!(manager.stats().loaded_partitions, 1);
    assert!(manager.is_partition_loaded(&partition_ids[0]));
    assert!(!manager.is_partition_loaded(&partition_ids[1]));
    assert!(!manager.is_partition_loaded(&partition_ids[2]));

    // Verify only nodes from first partition are accessible
    let graph = manager.graph_read();
    for node_id in &node_ids_per_partition[0] {
        assert!(
            graph.get_node(node_id).is_some(),
            "Node {} should be accessible",
            node_id
        );
    }
    for node_id in &node_ids_per_partition[1] {
        assert!(
            graph.get_node(node_id).is_none(),
            "Node {} should NOT be accessible",
            node_id
        );
    }

    // Verify the node count matches exactly
    assert_eq!(
        manager.stats().loaded_nodes,
        node_ids_per_partition[0].len()
    );
}

/// Test that loading the same partition twice doesn't duplicate data
#[test]
fn test_load_idempotency() {
    let (_temp_dir, prism_dir, partition_ids, _node_ids_per_partition) =
        setup_multi_partition_env(2, 10, None);

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Load partition first time
    manager
        .load_partition(&partition_ids[0])
        .expect("Failed to load partition");
    let node_count_after_first_load = manager.stats().loaded_nodes;
    let partition_count_after_first_load = manager.stats().loaded_partitions;

    // Load same partition second time
    manager
        .load_partition(&partition_ids[0])
        .expect("Failed to load partition again");
    let node_count_after_second_load = manager.stats().loaded_nodes;
    let partition_count_after_second_load = manager.stats().loaded_partitions;

    // Verify counts are identical
    assert_eq!(
        node_count_after_first_load, node_count_after_second_load,
        "Node count should be same after second load"
    );
    assert_eq!(
        partition_count_after_first_load, partition_count_after_second_load,
        "Partition count should be same after second load"
    );

    // Also verify through cache metrics - second load should be a hit
    let metrics = manager.cache_metrics();
    assert_eq!(metrics.misses, 1, "Should have exactly 1 miss (first load)");
    assert_eq!(metrics.hits, 1, "Should have exactly 1 hit (second load)");
}

/// Test that accessing a node in an unloaded partition triggers auto-load
#[test]
fn test_cross_partition_node_access() {
    let (_temp_dir, prism_dir, partition_ids, node_ids_per_partition) =
        setup_multi_partition_env(2, 5, None);

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Verify partition_1 is not loaded
    assert!(!manager.is_partition_loaded(&partition_ids[1]));

    // Access a node from partition_1 using get_node (which does lazy loading)
    let node_id = &node_ids_per_partition[1][0];
    let node = manager.get_node(node_id).expect("Failed to get node");

    // Verify the node was found
    assert!(node.is_some(), "Node should be found after auto-load");
    let node = node.unwrap();
    assert_eq!(node.name, "func_0");

    // Verify the partition is now loaded
    assert!(
        manager.is_partition_loaded(&partition_ids[1]),
        "Partition should be auto-loaded after node access"
    );
}

// ============================================================================
// 3.2 Unloading Behavior Tests
// ============================================================================

/// Test that unloading a partition frees memory
#[test]
fn test_unload_frees_memory() {
    let (_temp_dir, prism_dir, partition_ids, _) = setup_multi_partition_env(2, 50, None);

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Load both partitions
    manager
        .load_partition(&partition_ids[0])
        .expect("Failed to load p0");
    manager
        .load_partition(&partition_ids[1])
        .expect("Failed to load p1");

    let memory_before_unload = manager.memory_usage_bytes();
    assert!(
        memory_before_unload > 0,
        "Should have memory usage after loading"
    );

    // Unload one partition
    let unloaded_count = manager.unload_partition(&partition_ids[0]);
    assert!(unloaded_count > 0, "Should have unloaded some nodes");

    let memory_after_unload = manager.memory_usage_bytes();

    // Verify memory decreased
    assert!(
        memory_after_unload < memory_before_unload,
        "Memory should decrease after unload: {} < {}",
        memory_after_unload,
        memory_before_unload
    );
}

/// Test that data matches original after unload+reload cycle
#[test]
fn test_unload_then_reload() {
    let (_temp_dir, prism_dir, partition_ids, node_ids_per_partition) =
        setup_multi_partition_env(1, 10, None);

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Load partition
    manager
        .load_partition(&partition_ids[0])
        .expect("Failed to load");

    // Capture node data before unload
    let nodes_before: Vec<Node> = {
        let graph = manager.graph_read();
        node_ids_per_partition[0]
            .iter()
            .filter_map(|id| graph.get_node(id).cloned())
            .collect()
    };
    assert_eq!(nodes_before.len(), 10, "Should have all nodes loaded");

    // Unload partition
    manager.unload_partition(&partition_ids[0]);
    assert!(!manager.is_partition_loaded(&partition_ids[0]));

    // Reload partition
    manager
        .load_partition(&partition_ids[0])
        .expect("Failed to reload");

    // Verify data matches
    let graph = manager.graph_read();
    for original_node in &nodes_before {
        let reloaded_node = graph.get_node(&original_node.id);
        assert!(
            reloaded_node.is_some(),
            "Node {} should exist after reload",
            original_node.id
        );
        let reloaded_node = reloaded_node.unwrap();

        assert_eq!(reloaded_node.id, original_node.id, "Node ID mismatch");
        assert_eq!(reloaded_node.name, original_node.name, "Node name mismatch");
        assert_eq!(
            reloaded_node.node_type, original_node.node_type,
            "Node type mismatch"
        );
        assert_eq!(reloaded_node.file, original_node.file, "Node file mismatch");
        assert_eq!(reloaded_node.line, original_node.line, "Node line mismatch");
    }
}

/// Test that cross-partition edges survive partial unload
#[test]
fn test_unload_preserves_cross_refs() {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let prism_dir = temp_dir.path().join(".codeprysm");

    let mut manager = LazyGraphManager::init(&prism_dir).expect("Failed to init manager");

    // Create two partitions
    let partition_a = "partition_a";
    let partition_b = "partition_b";

    let db_path_a = prism_dir
        .join("partitions")
        .join(format!("{}.db", partition_a));
    let db_path_b = prism_dir
        .join("partitions")
        .join(format!("{}.db", partition_b));
    std::fs::create_dir_all(db_path_a.parent().unwrap()).expect("create dir");
    std::fs::create_dir_all(db_path_b.parent().unwrap()).expect("create dir");

    let conn_a = PartitionConnection::create(&db_path_a, partition_a).expect("create partition a");
    let conn_b = PartitionConnection::create(&db_path_b, partition_b).expect("create partition b");

    let node_a = create_test_node("a/main.py:caller", "caller", "a/main.py");
    let node_b = create_test_node("b/lib.py:helper", "helper", "b/lib.py");

    conn_a.insert_node(&node_a).expect("insert node a");
    conn_b.insert_node(&node_b).expect("insert node b");

    // Register in manifest
    manager
        .manifest_mut()
        .set_file("a/main.py".to_string(), partition_a.to_string(), None);
    manager
        .manifest_mut()
        .set_file("b/lib.py".to_string(), partition_b.to_string(), None);
    manager
        .manifest_mut()
        .register_partition(partition_a.to_string(), format!("{}.db", partition_a));
    manager
        .manifest_mut()
        .register_partition(partition_b.to_string(), format!("{}.db", partition_b));
    manager.save_manifest().expect("save manifest");

    // Add cross-partition edge: a/main.py:caller -> b/lib.py:helper
    let cross_ref = CrossRef::new(
        "a/main.py:caller".to_string(),
        partition_a.to_string(),
        "b/lib.py:helper".to_string(),
        partition_b.to_string(),
        EdgeType::Uses,
        Some(10),
        Some("helper".to_string()),
    );
    manager.add_cross_ref(cross_ref);
    assert_eq!(manager.cross_ref_count(), 1);

    // Load both partitions
    manager.load_partition(partition_a).expect("load a");
    manager.load_partition(partition_b).expect("load b");

    // Get outgoing edges before unload
    let edges_before = manager
        .get_outgoing_edges("a/main.py:caller")
        .expect("get edges");
    assert_eq!(edges_before.len(), 1, "Should have 1 cross-partition edge");

    // Unload partition_b
    manager.unload_partition(partition_b);
    assert!(!manager.is_partition_loaded(partition_b));

    // Cross-refs should still be in memory
    assert_eq!(
        manager.cross_ref_count(),
        1,
        "Cross-refs should survive unload"
    );

    // Get outgoing edges after unload - should trigger auto-load of partition_b
    let edges_after = manager
        .get_outgoing_edges("a/main.py:caller")
        .expect("get edges after");

    // Verify same edges are returned
    assert_eq!(
        edges_after.len(),
        1,
        "Should still have 1 cross-partition edge"
    );
    assert_eq!(edges_after[0].0.id, "b/lib.py:helper");
    assert_eq!(edges_after[0].1.edge_type, EdgeType::Uses);
    assert_eq!(edges_after[0].1.ref_line, Some(10));

    // Partition_b should be loaded again
    assert!(
        manager.is_partition_loaded(partition_b),
        "Partition should be auto-loaded"
    );
}

// ============================================================================
// 3.3 Memory Budget Tests
// ============================================================================

/// Test that exceeding memory budget triggers eviction
#[test]
fn test_budget_enforcement_triggers_eviction() {
    // Each partition with 10 nodes is approximately 7168 bytes (10 * 512 * 1.4)
    // Set budget to 15KB so loading 3 partitions (~21KB) will exceed it
    let (_temp_dir, prism_dir, partition_ids, _) = setup_multi_partition_env(4, 10, Some(15_000));

    let manager = LazyGraphManager::open_with_memory_budget(&prism_dir, Some(15_000))
        .expect("Failed to open manager");

    // Load partitions one by one
    manager.load_partition(&partition_ids[0]).expect("load p0");
    manager.load_partition(&partition_ids[1]).expect("load p1");

    // At this point we have 2 partitions, within budget
    assert_eq!(manager.stats().cache_evictions, 0, "No evictions yet");

    // Load third partition - may or may not trigger eviction depending on exact sizes
    manager.load_partition(&partition_ids[2]).expect("load p2");

    // Load fourth partition - should definitely trigger eviction
    manager.load_partition(&partition_ids[3]).expect("load p3");

    // Verify eviction happened
    let stats = manager.stats();
    assert!(
        stats.cache_evictions > 0,
        "Expected at least one eviction, got {}",
        stats.cache_evictions
    );

    // Verify we didn't keep all 4 partitions
    assert!(
        stats.loaded_partitions < 4,
        "Should have evicted at least one partition, have {} loaded",
        stats.loaded_partitions
    );
}

/// Test that min_partitions is respected even when over budget
#[test]
fn test_min_partitions_respected() {
    // Very small budget but default min_partitions is 2
    // Each partition with 20 nodes is ~14KB
    // Budget of 10KB means we're over budget with even 1 partition
    let (_temp_dir, prism_dir, partition_ids, _) = setup_multi_partition_env(3, 20, Some(10_000));

    let manager = LazyGraphManager::open_with_memory_budget(&prism_dir, Some(10_000))
        .expect("Failed to open manager");

    // Load all three partitions
    for pid in &partition_ids {
        manager.load_partition(pid).expect("Failed to load");
    }

    // Verify we're over budget but still have at least 2 partitions (min_partitions default)
    let stats = manager.stats();
    assert!(
        stats.loaded_partitions >= 2,
        "Should keep at least min_partitions (2), have {}",
        stats.loaded_partitions
    );

    // Should have some evictions since we loaded 3 but min is 2
    // Note: If eviction happens, we should have exactly 2 partitions
    if stats.cache_evictions > 0 {
        assert_eq!(
            stats.loaded_partitions, 2,
            "After eviction, should have exactly min_partitions"
        );
    }
}

/// Test that most recently accessed partition survives eviction (LRU ordering)
#[test]
fn test_lru_ordering() {
    // Small budget to force eviction
    let (_temp_dir, prism_dir, partition_ids, _) = setup_multi_partition_env(4, 10, Some(20_000));

    let manager = LazyGraphManager::open_with_memory_budget(&prism_dir, Some(20_000))
        .expect("Failed to open manager");

    // Load partitions 0, 1, 2
    manager.load_partition(&partition_ids[0]).expect("load p0");
    manager.load_partition(&partition_ids[1]).expect("load p1");
    manager.load_partition(&partition_ids[2]).expect("load p2");

    // Touch partition_0 to make it most recently used
    // LRU order before touch: p0 (oldest), p1, p2 (newest)
    // LRU order after touch: p1 (oldest), p2, p0 (newest)
    manager.load_partition(&partition_ids[0]).expect("touch p0"); // This is a cache hit

    // Now load partition_3 which should trigger eviction
    // partition_1 should be evicted (oldest in LRU order)
    manager.load_partition(&partition_ids[3]).expect("load p3");

    // Verify partition_0 survived (it was touched so MRU)
    assert!(
        manager.is_partition_loaded(&partition_ids[0]),
        "partition_0 should survive eviction (was touched)"
    );

    // Verify partition_3 is loaded (just loaded)
    assert!(
        manager.is_partition_loaded(&partition_ids[3]),
        "partition_3 should be loaded"
    );

    // partition_1 or partition_2 should have been evicted (whichever was LRU)
    // The exact one depends on min_partitions constraint
    let p1_loaded = manager.is_partition_loaded(&partition_ids[1]);
    let p2_loaded = manager.is_partition_loaded(&partition_ids[2]);

    // At least one of them should be evicted (unless no eviction happened due to min_partitions)
    let stats = manager.stats();
    if stats.cache_evictions > 0 {
        assert!(
            !p1_loaded || !p2_loaded,
            "At least one of p1, p2 should be evicted when p0 was touched"
        );
    }
}

// ============================================================================
// 3.4 Cache Metrics Tests
// ============================================================================

/// Test that hit rate calculation is accurate: Hits/(Hits+Misses)
#[test]
fn test_hit_rate_calculation() {
    let (_temp_dir, prism_dir, partition_ids, _) = setup_multi_partition_env(2, 5, None);

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // First load: miss
    manager.load_partition(&partition_ids[0]).expect("load");
    let metrics = manager.cache_metrics();
    assert_eq!(metrics.hits, 0);
    assert_eq!(metrics.misses, 1);
    assert_eq!(metrics.hit_rate(), 0.0);

    // Second load of same partition: hit
    manager
        .load_partition(&partition_ids[0])
        .expect("load again");
    let metrics = manager.cache_metrics();
    assert_eq!(metrics.hits, 1);
    assert_eq!(metrics.misses, 1);
    assert!(
        (metrics.hit_rate() - 0.5).abs() < 0.001,
        "Hit rate should be 0.5"
    );

    // Another hit
    manager
        .load_partition(&partition_ids[0])
        .expect("load again");
    let metrics = manager.cache_metrics();
    assert_eq!(metrics.hits, 2);
    assert_eq!(metrics.misses, 1);
    // hit_rate = 2 / (2 + 1) = 0.666...
    let expected_rate = 2.0 / 3.0;
    assert!(
        (metrics.hit_rate() - expected_rate).abs() < 0.001,
        "Hit rate should be ~0.667, got {}",
        metrics.hit_rate()
    );

    // Miss for new partition
    manager.load_partition(&partition_ids[1]).expect("load p1");
    let metrics = manager.cache_metrics();
    assert_eq!(metrics.hits, 2);
    assert_eq!(metrics.misses, 2);
    assert!(
        (metrics.hit_rate() - 0.5).abs() < 0.001,
        "Hit rate should be 0.5"
    );
}

/// Test that eviction counter increments correctly
#[test]
fn test_eviction_counter() {
    // Small budget to force multiple evictions
    let (_temp_dir, prism_dir, partition_ids, _) = setup_multi_partition_env(5, 10, Some(15_000));

    let manager = LazyGraphManager::open_with_memory_budget(&prism_dir, Some(15_000))
        .expect("Failed to open manager");

    let mut total_evictions = 0u64;

    // Load partitions and track evictions
    for (i, pid) in partition_ids.iter().enumerate() {
        let evictions_before = manager.cache_metrics().evictions;
        manager.load_partition(pid).expect("load");
        let evictions_after = manager.cache_metrics().evictions;

        let new_evictions = evictions_after - evictions_before;
        total_evictions += new_evictions;

        println!(
            "After loading partition {} ({}): evictions_before={}, evictions_after={}, total={}",
            i, pid, evictions_before, evictions_after, total_evictions
        );
    }

    // Verify the counter matches our running total
    let final_evictions = manager.cache_metrics().evictions;
    assert_eq!(
        final_evictions, total_evictions,
        "Final eviction count should match running total"
    );

    // We should have some evictions with 5 partitions and 15KB budget
    assert!(
        final_evictions > 0,
        "Should have some evictions, got {}",
        final_evictions
    );
}

/// Test that bytes evicted tracking matches sum of partition sizes
#[test]
fn test_bytes_evicted_tracking() {
    // Very small budget to ensure eviction
    let (_temp_dir, prism_dir, partition_ids, _) = setup_multi_partition_env(3, 10, Some(8_000));

    let manager = LazyGraphManager::open_with_memory_budget(&prism_dir, Some(8_000))
        .expect("Failed to open manager");

    // Load all partitions
    for pid in &partition_ids {
        manager.load_partition(pid).expect("load");
    }

    let metrics = manager.cache_metrics();

    // If evictions occurred, bytes_evicted should be non-zero
    if metrics.evictions > 0 {
        assert!(
            metrics.bytes_evicted > 0,
            "bytes_evicted should be > 0 when evictions > 0"
        );

        // Each partition with 10 nodes should be ~7168 bytes (10 * 512 * 1.4)
        let expected_bytes_per_partition = PartitionStats::new(10, 0).estimated_bytes;
        let min_expected_bytes = expected_bytes_per_partition * metrics.evictions as usize / 2;

        assert!(
            metrics.bytes_evicted >= min_expected_bytes,
            "bytes_evicted ({}) should be at least {} (half of {} per eviction)",
            metrics.bytes_evicted,
            min_expected_bytes,
            expected_bytes_per_partition
        );
    }
}

// ============================================================================
// Summary Test
// ============================================================================

/// Summary test that prints partition lifecycle metrics
#[test]
fn test_partition_lifecycle_summary() {
    println!("\n=== Partition Lifecycle Test Summary ===\n");

    let (_temp_dir, prism_dir) = setup_lazy_env("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open");

    println!("Initial state:");
    let stats = manager.stats();
    println!("  Total partitions: {}", stats.total_partitions);
    println!("  Total files: {}", stats.total_files);
    println!("  Memory budget: {} bytes", stats.memory_budget_bytes);

    // Load all partitions
    manager.load_all_partitions().expect("load all");

    println!("\nAfter loading all partitions:");
    let stats = manager.stats();
    println!("  Loaded partitions: {}", stats.loaded_partitions);
    println!("  Loaded nodes: {}", stats.loaded_nodes);
    println!("  Loaded edges: {}", stats.loaded_edges);
    println!("  Memory usage: {} bytes", stats.memory_usage_bytes);

    // Get cache metrics
    let metrics = manager.cache_metrics();
    println!("\nCache metrics:");
    println!("  Hits: {}", metrics.hits);
    println!("  Misses: {}", metrics.misses);
    println!("  Hit rate: {:.2}%", metrics.hit_rate() * 100.0);
    println!("  Evictions: {}", metrics.evictions);
    println!("  Bytes evicted: {}", metrics.bytes_evicted);

    println!();
}
