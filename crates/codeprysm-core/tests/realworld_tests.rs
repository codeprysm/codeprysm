//! Real-world integration tests (Tier 2).
//!
//! These tests clone popular open-source repositories and validate that
//! codeprysm-core correctly parses and generates graphs for real codebases.
//!
//! ## Running These Tests
//!
//! These tests are marked with `#[ignore]` and don't run by default.
//! Run them with:
//!
//! ```bash
//! # Run all real-world tests (slow, clones repos)
//! cargo test --package codeprysm-core --test realworld_tests -- --ignored
//!
//! # Run for a specific language
//! cargo test --package codeprysm-core --test realworld_tests python -- --ignored
//!
//! # Run with output showing
//! cargo test --package codeprysm-core --test realworld_tests -- --ignored --nocapture
//! ```
//!
//! ## Test Repositories
//!
//! Each language has a curated repository selected for:
//! - Popularity and stability
//! - Reasonable size (<50MB source)
//! - Representative code patterns
//! - Pinned to specific commit for reproducibility

mod common;

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::graph::{NodeType, PetCodeGraph};
use common::{check_min_counts, compute_stats, validate_all, GraphStats, RepoCache, TestRepo};
use serde::Deserialize;
use std::path::PathBuf;

// ============================================================================
// Expected Results Schema
// ============================================================================

/// Expected test results loaded from YAML
#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct ExpectedResults {
    repo: String,
    version: String,
    language: String,
    subdir: Option<String>,
    min_counts: MinCounts,
    required_entities: Vec<RequiredEntity>,
    #[serde(default)]
    containment: Vec<ContainmentEdge>,
}

/// Minimum entity counts
#[derive(Debug, Deserialize)]
struct MinCounts {
    files: usize,
    containers: usize,
    callables: usize,
    data: usize,
}

/// A required entity that must exist in the graph
#[derive(Debug, Deserialize)]
struct RequiredEntity {
    name: String,
    #[serde(rename = "type")]
    entity_type: String,
    kind: String,
}

/// A containment relationship to verify
#[derive(Debug, Deserialize)]
struct ContainmentEdge {
    parent: String,
    child: String,
}

impl RequiredEntity {
    /// Convert type string to NodeType
    fn node_type(&self) -> NodeType {
        match self.entity_type.as_str() {
            "Container" | "File" | "FILE" => NodeType::Container, // Files are now Container with kind="file"
            "Callable" => NodeType::Callable,
            "Data" => NodeType::Data,
            other => panic!("Unknown entity type: {}", other),
        }
    }
}

// ============================================================================
// Test Infrastructure
// ============================================================================

/// Get the path to the expected results directory.
fn expected_results_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("expected_results")
}

/// Get the path to the queries directory.
fn queries_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("queries")
}

/// Load expected results for a language
fn load_expected_results(language: &str) -> ExpectedResults {
    let path = expected_results_dir().join(format!("{}.yaml", language));
    let contents = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("Failed to read {}: {}", path.display(), e));
    serde_yaml::from_str(&contents)
        .unwrap_or_else(|e| panic!("Failed to parse {}: {}", path.display(), e))
}

/// Build a graph from a real-world repository
fn build_realworld_graph(repo: &TestRepo) -> PetCodeGraph {
    let cache = RepoCache::new().expect("Failed to create repo cache");
    let repo_path = cache.get_repo(repo).expect("Failed to get repository");

    println!("Building graph for {} at {:?}", repo.repo, repo_path);

    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");

    builder
        .build_from_directory(&repo_path)
        .expect("Failed to build graph")
}

/// Validate graph against expected results
fn validate_against_expected(graph: &PetCodeGraph, expected: &ExpectedResults) -> Vec<String> {
    let mut errors = Vec::new();

    // Check minimum counts with 10% tolerance
    let stats = compute_stats(graph);
    errors.extend(check_min_counts(
        &stats,
        expected.min_counts.files,
        expected.min_counts.containers,
        expected.min_counts.callables,
        expected.min_counts.data,
        10.0, // 10% tolerance
    ));

    // Check required entities
    for entity in &expected.required_entities {
        let found = graph.iter_nodes().find(|n| {
            n.name == entity.name
                && n.node_type == entity.node_type()
                && n.kind.as_deref() == Some(entity.kind.as_str())
        });

        if found.is_none() {
            errors.push(format!(
                "Required entity '{}' ({:?}/{}) not found",
                entity.name,
                entity.node_type(),
                entity.kind
            ));
        }
    }

    // Check containment relationships
    for edge in &expected.containment {
        let parent = graph.iter_nodes().find(|n| n.name == edge.parent);
        let child = graph.iter_nodes().find(|n| n.name == edge.child);

        match (parent, child) {
            (Some(p), Some(c)) => {
                let has_edge = graph.iter_edges().any(|e| {
                    e.edge_type == codeprysm_core::graph::EdgeType::Contains
                        && e.source == p.id
                        && e.target == c.id
                });
                if !has_edge {
                    errors.push(format!(
                        "Expected CONTAINS edge from '{}' to '{}' not found",
                        edge.parent, edge.child
                    ));
                }
            }
            (None, _) => {
                errors.push(format!(
                    "Parent entity '{}' for containment check not found",
                    edge.parent
                ));
            }
            (_, None) => {
                errors.push(format!(
                    "Child entity '{}' for containment check not found",
                    edge.child
                ));
            }
        }
    }

    errors
}

/// Print test summary
fn print_summary(language: &str, stats: &GraphStats, errors: &[String]) {
    let status = if errors.is_empty() { "PASS" } else { "FAIL" };

    println!(
        "\n=== {} Real-World Test: {} ===",
        language.to_uppercase(),
        status
    );
    println!(
        "Nodes: {} (Files: {}, Containers: {}, Callables: {}, Data: {})",
        stats.total_nodes(),
        stats.file_count,
        stats.container_count,
        stats.callable_count,
        stats.data_count
    );
    println!(
        "Edges: {} (Contains: {}, Uses: {}, Defines: {})",
        stats.total_edges(),
        stats.contains_edges,
        stats.uses_edges,
        stats.defines_edges
    );

    if !errors.is_empty() {
        println!("\nErrors ({}):", errors.len());
        for err in errors.iter().take(10) {
            println!("  - {}", err);
        }
        if errors.len() > 10 {
            println!("  ... and {} more errors", errors.len() - 10);
        }
    }
}

/// Run a real-world test for a specific language
fn run_realworld_test(language: &str) {
    let expected = load_expected_results(language);
    let repo = common::test_repos::repos::by_language(language)
        .unwrap_or_else(|| panic!("No test repo defined for language: {}", language));

    let graph = build_realworld_graph(repo);

    // Run all standard validations
    let validation_result = validate_all(&graph);
    let mut all_errors = validation_result.all_errors();

    // Run expected results validation
    all_errors.extend(validate_against_expected(&graph, &expected));

    let stats = compute_stats(&graph);
    print_summary(language, &stats, &all_errors);

    assert!(
        all_errors.is_empty(),
        "Real-world test for {} failed with {} errors:\n{}",
        language,
        all_errors.len(),
        all_errors.join("\n")
    );
}

// ============================================================================
// Real-World Tests (Tier 2)
// ============================================================================

/// Test Python: Flask web framework
#[test]
#[ignore]
fn test_realworld_python() {
    run_realworld_test("python");
}

/// Test JavaScript: Express.js web framework
#[test]
#[ignore]
fn test_realworld_javascript() {
    run_realworld_test("javascript");
}

/// Test TypeScript: TypeORM
#[test]
#[ignore]
fn test_realworld_typescript() {
    run_realworld_test("typescript");
}

/// Test C: hiredis Redis client
#[test]
#[ignore]
fn test_realworld_c() {
    run_realworld_test("c");
}

/// Test C++: nlohmann JSON library
#[test]
#[ignore]
fn test_realworld_cpp() {
    run_realworld_test("cpp");
}

/// Test C#: Newtonsoft.Json
#[test]
#[ignore]
fn test_realworld_csharp() {
    run_realworld_test("csharp");
}

/// Test Go: Echo web framework
#[test]
#[ignore]
fn test_realworld_go() {
    run_realworld_test("go");
}

/// Test Rust: Serde serialization
#[test]
#[ignore]
fn test_realworld_rust() {
    run_realworld_test("rust");
}

// ============================================================================
// Summary Test
// ============================================================================

/// Run all real-world tests and report summary
#[test]
#[ignore]
fn test_realworld_all_summary() {
    let languages = [
        "python",
        "javascript",
        "typescript",
        "c",
        "cpp",
        "csharp",
        "go",
        "rust",
    ];

    println!("\n=== Real-World Integration Tests Summary ===\n");

    let mut total_errors = 0;
    let mut passed = 0;
    let mut failed = 0;

    for lang in &languages {
        let expected = load_expected_results(lang);
        let repo = match common::test_repos::repos::by_language(lang) {
            Some(r) => r,
            None => {
                println!("{:12} SKIP - No test repo defined", lang);
                continue;
            }
        };

        match std::panic::catch_unwind(|| build_realworld_graph(repo)) {
            Ok(graph) => {
                let validation = validate_all(&graph);
                let mut errors = validation.all_errors();
                errors.extend(validate_against_expected(&graph, &expected));

                let stats = compute_stats(&graph);
                let status = if errors.is_empty() {
                    passed += 1;
                    "PASS"
                } else {
                    failed += 1;
                    total_errors += errors.len();
                    "FAIL"
                };

                println!(
                    "{:12} {} | Nodes: {:4} | Errors: {}",
                    lang,
                    status,
                    stats.total_nodes(),
                    errors.len()
                );
            }
            Err(_) => {
                failed += 1;
                println!("{:12} FAIL | Build panicked", lang);
            }
        }
    }

    println!("\n---");
    println!(
        "Total: {} passed, {} failed, {} errors",
        passed, failed, total_errors
    );
    println!();

    assert_eq!(failed, 0, "Some real-world tests failed");
}
