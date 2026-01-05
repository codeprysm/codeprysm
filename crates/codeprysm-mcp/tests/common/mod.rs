//! Common test utilities for codeprysm-mcp integration tests.
//!
//! This module provides helpers for setting up test fixtures, creating
//! server instances, and validating tool responses.

#![allow(dead_code)]

use std::path::PathBuf;
use tempfile::TempDir;

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::graph::PetCodeGraph;
use codeprysm_core::lazy::partitioner::GraphPartitioner;

/// Get path to codeprysm-core queries directory
pub fn queries_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codeprysm-core")
        .join("queries")
}

/// Get path to codeprysm-core integration test fixtures
pub fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codeprysm-core")
        .join("tests")
        .join("fixtures")
        .join("integration_repos")
}

/// Build a graph from a fixture directory
pub fn build_fixture_graph(language: &str) -> PetCodeGraph {
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

/// Set up a test environment with partitioned graph storage
///
/// Returns (temp_dir, repo_path, prism_dir) where:
/// - temp_dir: The temporary directory handle (keeps it alive)
/// - repo_path: Path to the fixture repository
/// - prism_dir: Path to the .codeprysm directory with partitioned storage
pub fn setup_test_environment(language: &str) -> (TempDir, PathBuf, PathBuf) {
    // Build graph from fixture
    let graph = build_fixture_graph(language);
    let fixture_path = fixtures_dir().join(language);

    // Create temp directory for .codeprysm storage
    let temp_dir = TempDir::new().expect("Failed to create temp directory");
    let prism_dir = temp_dir.path().join(".codeprysm");
    std::fs::create_dir_all(&prism_dir).expect("Failed to create prism directory");

    // Partition the graph (returns (Manifest, PartitioningStats) tuple)
    let (_manifest, stats) =
        GraphPartitioner::partition_with_stats(&graph, &prism_dir, Some("test"))
            .expect("Failed to partition graph");

    assert!(stats.partition_count > 0, "Expected at least one partition");
    assert!(stats.total_nodes > 0, "Expected at least one node");

    (temp_dir, fixture_path, prism_dir)
}

/// Parse JSON response from tool result
pub fn parse_tool_response(content: &str) -> serde_json::Value {
    serde_json::from_str(content).expect("Failed to parse tool response as JSON")
}

/// Assert that a JSON response contains a specific field
pub fn assert_field_exists(response: &serde_json::Value, field: &str) {
    assert!(
        response.get(field).is_some(),
        "Expected field '{}' in response: {:?}",
        field,
        response
    );
}

/// Assert that a JSON response field equals expected value
pub fn assert_field_eq<T: PartialEq + std::fmt::Debug>(
    response: &serde_json::Value,
    field: &str,
    expected: T,
) where
    serde_json::Value: PartialEq<T>,
{
    let value = response
        .get(field)
        .unwrap_or_else(|| panic!("Field '{}' not found", field));
    assert!(
        value == &expected,
        "Expected {} = {:?}, got {:?}",
        field,
        expected,
        value
    );
}
