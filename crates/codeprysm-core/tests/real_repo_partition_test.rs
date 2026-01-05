//! Integration test for lazy-loading with a real multi-partition repository.
//!
//! This test uses the prism crates directory which generates multiple partitions
//! and cross-partition edges.
//!
//! Run with: cargo test --package codeprysm-core --test real_repo_partition_test -- --nocapture

use std::path::PathBuf;
use tempfile::TempDir;

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::lazy::manager::LazyGraphManager;
use codeprysm_core::lazy::partitioner::GraphPartitioner;

fn queries_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("queries")
}

fn crates_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Build graph and partition for the prism crates directory
fn setup_real_repo() -> (TempDir, PathBuf) {
    let crates_path = crates_dir();

    // Build graph
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");
    let graph = builder
        .build_from_directory(&crates_path)
        .expect("Failed to build graph");

    println!("Built graph: {} nodes", graph.iter_nodes().count());

    // Create temp dir for partitioned storage
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let prism_dir = temp_dir.path().join(".codeprysm");
    std::fs::create_dir_all(&prism_dir).expect("Failed to create prism dir");

    // Partition the graph
    let (_manifest, stats) =
        GraphPartitioner::partition_with_stats(&graph, &prism_dir, Some("crates"))
            .expect("Failed to partition graph");

    println!(
        "Partitioned: {} partitions, {} cross-partition edges",
        stats.partition_count, stats.cross_partition_edges
    );

    (temp_dir, prism_dir)
}

#[test]
fn test_real_repo_multiple_partitions() {
    println!("\n=== Testing Real Repository Partitioning ===\n");

    let (_temp_dir, prism_dir) = setup_real_repo();

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    let stats = manager.stats();

    // Should have multiple partitions
    assert!(
        stats.total_partitions > 1,
        "Real repo should have multiple partitions, got {}",
        stats.total_partitions
    );

    // Should have cross-partition edges
    assert!(
        stats.cross_partition_edges > 0,
        "Real repo should have cross-partition edges, got {}",
        stats.cross_partition_edges
    );

    println!("Partitions: {}", stats.total_partitions);
    println!("Cross-partition edges: {}", stats.cross_partition_edges);
    println!("Total files tracked: {}", stats.total_files);
}

#[test]
fn test_real_repo_incremental_loading() {
    println!("\n=== Testing Incremental Partition Loading ===\n");

    let (_temp_dir, prism_dir) = setup_real_repo();

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Initially nothing loaded
    assert_eq!(manager.stats().loaded_partitions, 0);
    assert_eq!(manager.stats().loaded_nodes, 0);

    // Load partitions one by one
    let partition_ids: Vec<String> = manager.manifest().partitions.keys().cloned().collect();

    println!(
        "Loading {} partitions incrementally...",
        partition_ids.len()
    );

    let mut prev_nodes = 0;
    for (i, pid) in partition_ids.iter().enumerate() {
        manager
            .load_partition(pid)
            .expect("Failed to load partition");

        let stats = manager.stats();
        let new_nodes = stats.loaded_nodes - prev_nodes;

        println!(
            "  [{}/{}] {} -> +{} nodes (total: {})",
            i + 1,
            partition_ids.len(),
            pid,
            new_nodes,
            stats.loaded_nodes
        );

        assert!(
            stats.loaded_nodes >= prev_nodes,
            "Node count should not decrease"
        );
        prev_nodes = stats.loaded_nodes;
    }

    // After loading all, should have all nodes
    let final_stats = manager.stats();
    assert_eq!(final_stats.loaded_partitions, final_stats.total_partitions);
    println!(
        "\nFinal: {} nodes loaded across {} partitions",
        final_stats.loaded_nodes, final_stats.loaded_partitions
    );
}

#[test]
fn test_real_repo_cross_partition_edges() {
    println!("\n=== Testing Cross-Partition Edge Queries ===\n");

    let (_temp_dir, prism_dir) = setup_real_repo();

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager.load_all_partitions().expect("Failed to load all");

    // Find LazyGraphManager or another substantial node
    let test_candidates = ["LazyGraphManager", "PetCodeGraph", "Node", "GraphBuilder"];

    // Extract node info first (to release graph borrow)
    let test_nodes: Vec<(String, String, String)> = {
        let graph = manager.graph_read();
        test_candidates
            .iter()
            .filter_map(|name| {
                graph
                    .iter_nodes()
                    .find(|n| n.name == *name)
                    .map(|n| (name.to_string(), n.id.clone(), n.file.clone()))
            })
            .collect()
    };

    for (name, node_id, node_file) in test_nodes {
        println!("Testing node: {} (file: {})", name, node_file);

        let incoming = manager
            .get_incoming_edges(&node_id)
            .expect("Failed to get incoming");
        let outgoing = manager
            .get_outgoing_edges(&node_id)
            .expect("Failed to get outgoing");

        println!("  Incoming edges: {}", incoming.len());
        println!("  Outgoing edges: {}", outgoing.len());

        // Check if any edges come from different partitions
        let node_partition = manager.get_partition_for_file(&node_file);

        let cross_incoming: Vec<_> = incoming
            .iter()
            .filter(|(src, _)| {
                let src_partition = manager.get_partition_for_file(&src.file);
                src_partition != node_partition
            })
            .collect();

        let cross_outgoing: Vec<_> = outgoing
            .iter()
            .filter(|(tgt, _)| {
                let tgt_partition = manager.get_partition_for_file(&tgt.file);
                tgt_partition != node_partition
            })
            .collect();

        println!("  Cross-partition incoming: {}", cross_incoming.len());
        println!("  Cross-partition outgoing: {}", cross_outgoing.len());

        if !cross_incoming.is_empty() || !cross_outgoing.is_empty() {
            println!("  SUCCESS: Found cross-partition edges!");

            // Show sample cross-partition edges
            if let Some((src, edge)) = cross_incoming.first() {
                println!(
                    "    Sample incoming: {} -> {} ({:?})",
                    src.name, name, edge.edge_type
                );
            }
            if let Some((tgt, edge)) = cross_outgoing.first() {
                println!(
                    "    Sample outgoing: {} -> {} ({:?})",
                    name, tgt.name, edge.edge_type
                );
            }

            return; // Test passed
        }
    }

    // If we get here, query nodes directly from cross_refs for deterministic testing
    let stats = manager.stats();
    println!(
        "\nSearching for cross-partition edges using cross_refs ({} total)...",
        stats.cross_partition_edges
    );

    // Get source node IDs directly from cross_refs - these are guaranteed to have cross-partition edges
    let cross_ref_sources: Vec<_> = manager
        .iter_cross_refs()
        .take(10) // Sample a few cross refs
        .map(|cr| (cr.source_id.clone(), cr.target_id.clone(), cr.edge_type))
        .collect();

    for (source_id, target_id, edge_type) in cross_ref_sources {
        // Try to get outgoing edges from this source
        let outgoing = manager.get_outgoing_edges(&source_id).unwrap_or_default();

        // Check if the cross-partition edge is included
        let found_cross = outgoing
            .iter()
            .any(|(node, edge)| node.id == target_id && edge.edge_type == edge_type);

        if found_cross {
            // Get node info for display
            let source_node = {
                let graph = manager.graph_read();
                graph.get_node(&source_id).cloned()
            };
            let target_node = {
                let graph = manager.graph_read();
                graph.get_node(&target_id).cloned()
            };

            if let (Some(src), Some(tgt)) = (source_node, target_node) {
                println!("\nFound cross-partition edge via query:");
                println!("  Source: {} ({})", src.name, src.file);
                println!("  Target: {} ({})", tgt.name, tgt.file);
                println!("  Edge type: {:?}", edge_type);
                println!("\n  SUCCESS: Cross-partition edges verified!");
                return;
            }
        }
    }

    // Fail if we have cross-partition edges but can't query them
    assert!(
        stats.cross_partition_edges == 0,
        "Have {} cross-partition edges but couldn't find them via queries!",
        stats.cross_partition_edges
    );
}

#[test]
fn test_real_repo_unload_reload() {
    println!("\n=== Testing Unload and Reload ===\n");

    let (_temp_dir, prism_dir) = setup_real_repo();

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");

    // Load all
    manager.load_all_partitions().expect("Failed to load all");
    let loaded_nodes = manager.stats().loaded_nodes;
    println!("Loaded {} nodes", loaded_nodes);

    // Unload all
    let partition_ids: Vec<String> = manager
        .loaded_partitions()
        .iter()
        .map(|s| s.to_string())
        .collect();
    for pid in partition_ids {
        manager.unload_partition(&pid);
    }

    assert_eq!(manager.stats().loaded_partitions, 0);
    assert_eq!(manager.stats().loaded_nodes, 0);
    println!("Unloaded all partitions");

    // Reload all
    manager.load_all_partitions().expect("Failed to reload");
    assert_eq!(
        manager.stats().loaded_nodes,
        loaded_nodes,
        "Should reload same number of nodes"
    );
    println!("Reloaded {} nodes - matches original!", loaded_nodes);
}

#[test]
fn test_real_repo_summary() {
    println!("\n=== Real Repository Partition Test Summary ===\n");

    let (_temp_dir, prism_dir) = setup_real_repo();

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager.load_all_partitions().expect("Failed to load all");

    let stats = manager.stats();

    println!("Repository: prism/crates");
    println!("─────────────────────────────────────");
    println!("Total partitions:      {:>6}", stats.total_partitions);
    println!("Total files:           {:>6}", stats.total_files);
    println!("Total nodes:           {:>6}", stats.loaded_nodes);
    println!("Total edges:           {:>6}", stats.loaded_edges);
    println!("Cross-partition edges: {:>6}", stats.cross_partition_edges);
    println!(
        "Memory usage:          {:>6} KB",
        stats.memory_usage_bytes / 1024
    );
    println!(
        "Cache hit rate:        {:>5.1}%",
        stats.cache_hit_rate * 100.0
    );
    println!();

    // Verify reasonable values
    assert!(stats.total_partitions >= 10, "Should have many partitions");
    assert!(stats.loaded_nodes > 1000, "Should have many nodes");
    assert!(
        stats.cross_partition_edges > 100,
        "Should have many cross-partition edges"
    );
}
