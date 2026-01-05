//! Common test utilities for codeprysm-search integration tests.
//!
//! These tests require Qdrant running at localhost:6334.
//! Start with: `just qdrant-start`

use std::path::PathBuf;

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::graph::PetCodeGraph;

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

/// Generate a unique repo ID for test isolation
pub fn unique_repo_id(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{}_{}", prefix, timestamp)
}
