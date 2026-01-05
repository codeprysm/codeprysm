//! Integration tests for codeprysm-core graph generation.
//!
//! This module provides two tiers of integration testing:
//!
//! ## Tier 1: Fixture Tests (Every PR)
//! Fast tests using curated fixture repositories. Run on every PR.
//! Target: <30 seconds per language.
//!
//! ## Tier 2: Real-World Tests (Nightly)
//! Comprehensive tests using popular GitHub repositories.
//! Run nightly/weekly via `cargo test -- --ignored`.
//!
//! ## Running Tests
//!
//! ```bash
//! # Run all fixture tests
//! cargo test --test integration
//!
//! # Run tests for a specific language
//! cargo test --test integration python
//!
//! # Run nightly tests (real-world repos)
//! cargo test --test integration -- --ignored
//! ```

mod common;

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::graph::{NodeType, PetCodeGraph};
use common::{
    assert_entity_exists, compute_stats, validate_all, validate_relational, validate_semantic,
    validate_structural,
};
use std::path::PathBuf;

// ============================================================================
// Test Fixtures Path Helpers
// ============================================================================

/// Get the path to the integration fixtures directory.
fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("integration_repos")
}

/// Get the path to the queries directory.
fn queries_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("queries")
}

/// Build a graph from a fixture directory.
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

/// Assert that the graph passes all validation levels.
fn assert_graph_valid(graph: &PetCodeGraph, language: &str) {
    let result = validate_all(graph);

    if !result.is_valid() {
        let errors = result.all_errors();
        panic!(
            "Graph validation failed for {} ({} errors):\n{}",
            language,
            errors.len(),
            errors.join("\n")
        );
    }
}

// ============================================================================
// Python Fixture Tests
// ============================================================================

#[test]
fn test_python_fixture_structural() {
    let graph = build_fixture_graph("python");
    let errors = validate_structural(&graph);
    assert!(
        errors.is_empty(),
        "Python structural validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_python_fixture_semantic() {
    let graph = build_fixture_graph("python");
    let errors = validate_semantic(&graph);
    assert!(
        errors.is_empty(),
        "Python semantic validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_python_fixture_relational() {
    let graph = build_fixture_graph("python");
    let errors = validate_relational(&graph);
    assert!(
        errors.is_empty(),
        "Python relational validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_python_fixture_entities() {
    let graph = build_fixture_graph("python");

    // Check expected entities
    assert_entity_exists(&graph, "Calculator", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "__init__", NodeType::Callable, Some("constructor"));
    assert_entity_exists(&graph, "add", NodeType::Callable, Some("method"));
    assert_entity_exists(&graph, "multiply", NodeType::Callable, Some("method"));
    assert_entity_exists(
        &graph,
        "standalone_function",
        NodeType::Callable,
        Some("function"),
    );
    assert_entity_exists(
        &graph,
        "async_standalone",
        NodeType::Callable,
        Some("function"),
    );
    assert_entity_exists(&graph, "AsyncProcessor", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "InheritedClass", NodeType::Container, Some("type"));
}

#[test]
fn test_python_fixture_full() {
    let graph = build_fixture_graph("python");
    assert_graph_valid(&graph, "python");

    let stats = compute_stats(&graph);
    assert!(stats.file_count >= 1, "Should have at least 1 FILE node");
    assert!(
        stats.container_count >= 3,
        "Should have at least 3 Container nodes"
    );
    assert!(
        stats.callable_count >= 10,
        "Should have at least 10 Callable nodes"
    );
}

// ============================================================================
// JavaScript Fixture Tests
// ============================================================================

#[test]
fn test_javascript_fixture_structural() {
    let graph = build_fixture_graph("javascript");
    let errors = validate_structural(&graph);
    assert!(
        errors.is_empty(),
        "JavaScript structural validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_javascript_fixture_semantic() {
    let graph = build_fixture_graph("javascript");
    let errors = validate_semantic(&graph);
    assert!(
        errors.is_empty(),
        "JavaScript semantic validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_javascript_fixture_relational() {
    let graph = build_fixture_graph("javascript");
    let errors = validate_relational(&graph);
    assert!(
        errors.is_empty(),
        "JavaScript relational validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_javascript_fixture_entities() {
    let graph = build_fixture_graph("javascript");

    // Check expected entities
    assert_entity_exists(&graph, "Calculator", NodeType::Container, Some("type"));
    assert_entity_exists(
        &graph,
        "constructor",
        NodeType::Callable,
        Some("constructor"),
    );
    assert_entity_exists(&graph, "add", NodeType::Callable, Some("method"));
    assert_entity_exists(
        &graph,
        "standaloneFunction",
        NodeType::Callable,
        Some("function"),
    );
    assert_entity_exists(&graph, "AsyncProcessor", NodeType::Container, Some("type"));
}

#[test]
fn test_javascript_fixture_full() {
    let graph = build_fixture_graph("javascript");
    assert_graph_valid(&graph, "javascript");

    let stats = compute_stats(&graph);
    assert!(stats.file_count >= 1, "Should have at least 1 FILE node");
    assert!(
        stats.container_count >= 2,
        "Should have at least 2 Container nodes"
    );
    assert!(
        stats.callable_count >= 8,
        "Should have at least 8 Callable nodes"
    );
}

// ============================================================================
// TypeScript Fixture Tests
// ============================================================================

#[test]
fn test_typescript_fixture_structural() {
    let graph = build_fixture_graph("typescript");
    let errors = validate_structural(&graph);
    assert!(
        errors.is_empty(),
        "TypeScript structural validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_typescript_fixture_semantic() {
    let graph = build_fixture_graph("typescript");
    let errors = validate_semantic(&graph);
    assert!(
        errors.is_empty(),
        "TypeScript semantic validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_typescript_fixture_relational() {
    let graph = build_fixture_graph("typescript");
    let errors = validate_relational(&graph);
    assert!(
        errors.is_empty(),
        "TypeScript relational validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_typescript_fixture_entities() {
    let graph = build_fixture_graph("typescript");

    // Check expected entities - interfaces and classes
    assert_entity_exists(&graph, "User", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "Repository", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "UserRepository", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "DataProcessor", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "BaseService", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "UserService", NodeType::Container, Some("type"));
    assert_entity_exists(
        &graph,
        "standaloneFunction",
        NodeType::Callable,
        Some("function"),
    );
}

#[test]
fn test_typescript_fixture_full() {
    let graph = build_fixture_graph("typescript");
    assert_graph_valid(&graph, "typescript");

    let stats = compute_stats(&graph);
    assert!(stats.file_count >= 1, "Should have at least 1 FILE node");
    assert!(
        stats.container_count >= 5,
        "Should have at least 5 Container nodes"
    );
}

// ============================================================================
// C Fixture Tests
// ============================================================================

#[test]
fn test_c_fixture_structural() {
    let graph = build_fixture_graph("c");
    let errors = validate_structural(&graph);
    assert!(
        errors.is_empty(),
        "C structural validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_c_fixture_semantic() {
    let graph = build_fixture_graph("c");
    let errors = validate_semantic(&graph);
    assert!(
        errors.is_empty(),
        "C semantic validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_c_fixture_relational() {
    let graph = build_fixture_graph("c");
    let errors = validate_relational(&graph);
    assert!(
        errors.is_empty(),
        "C relational validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_c_fixture_entities() {
    let graph = build_fixture_graph("c");

    // Check expected entities - structs and functions
    assert_entity_exists(&graph, "Calculator", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "Processor", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "Status", NodeType::Container, Some("type"));
    assert_entity_exists(
        &graph,
        "calculator_new",
        NodeType::Callable,
        Some("function"),
    );
    assert_entity_exists(
        &graph,
        "calculator_add",
        NodeType::Callable,
        Some("function"),
    );
    assert_entity_exists(
        &graph,
        "standalone_function",
        NodeType::Callable,
        Some("function"),
    );
    assert_entity_exists(&graph, "main", NodeType::Callable, Some("function"));
}

#[test]
fn test_c_fixture_full() {
    let graph = build_fixture_graph("c");
    assert_graph_valid(&graph, "c");

    let stats = compute_stats(&graph);
    assert!(stats.file_count >= 1, "Should have at least 1 FILE node");
    assert!(
        stats.callable_count >= 5,
        "Should have at least 5 Callable nodes"
    );
}

// ============================================================================
// C++ Fixture Tests
// ============================================================================

#[test]
fn test_cpp_fixture_structural() {
    let graph = build_fixture_graph("cpp");
    let errors = validate_structural(&graph);
    assert!(
        errors.is_empty(),
        "C++ structural validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_cpp_fixture_semantic() {
    let graph = build_fixture_graph("cpp");
    let errors = validate_semantic(&graph);
    assert!(
        errors.is_empty(),
        "C++ semantic validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_cpp_fixture_relational() {
    let graph = build_fixture_graph("cpp");
    let errors = validate_relational(&graph);
    assert!(
        errors.is_empty(),
        "C++ relational validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_cpp_fixture_entities() {
    let graph = build_fixture_graph("cpp");

    // Check expected entities - classes and namespaces
    assert_entity_exists(&graph, "math", NodeType::Container, Some("namespace"));
    assert_entity_exists(&graph, "Calculator", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "AsyncProcessor", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "DataProcessor", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "ICalculator", NodeType::Container, Some("type"));
    assert_entity_exists(
        &graph,
        "standaloneFunction",
        NodeType::Callable,
        Some("function"),
    );
    assert_entity_exists(&graph, "main", NodeType::Callable, Some("function"));
}

#[test]
fn test_cpp_fixture_full() {
    let graph = build_fixture_graph("cpp");
    assert_graph_valid(&graph, "cpp");

    let stats = compute_stats(&graph);
    assert!(stats.file_count >= 1, "Should have at least 1 FILE node");
    assert!(
        stats.container_count >= 3,
        "Should have at least 3 Container nodes"
    );
}

// ============================================================================
// C# Fixture Tests
// ============================================================================

#[test]
fn test_csharp_fixture_structural() {
    let graph = build_fixture_graph("csharp");
    let errors = validate_structural(&graph);
    assert!(
        errors.is_empty(),
        "C# structural validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_csharp_fixture_semantic() {
    let graph = build_fixture_graph("csharp");
    let errors = validate_semantic(&graph);
    assert!(
        errors.is_empty(),
        "C# semantic validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_csharp_fixture_relational() {
    let graph = build_fixture_graph("csharp");
    let errors = validate_relational(&graph);
    assert!(
        errors.is_empty(),
        "C# relational validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_csharp_fixture_entities() {
    let graph = build_fixture_graph("csharp");

    // Check expected entities - classes and interfaces
    assert_entity_exists(
        &graph,
        "IntegrationTests",
        NodeType::Container,
        Some("namespace"),
    );
    assert_entity_exists(&graph, "ICalculator", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "Calculator", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "AsyncProcessor", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "UserService", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "User", NodeType::Container, Some("type"));
}

#[test]
fn test_csharp_fixture_full() {
    let graph = build_fixture_graph("csharp");
    assert_graph_valid(&graph, "csharp");

    let stats = compute_stats(&graph);
    assert!(stats.file_count >= 1, "Should have at least 1 FILE node");
    assert!(
        stats.container_count >= 5,
        "Should have at least 5 Container nodes"
    );
}

// ============================================================================
// Go Fixture Tests
// ============================================================================

#[test]
fn test_go_fixture_structural() {
    let graph = build_fixture_graph("go");
    let errors = validate_structural(&graph);
    assert!(
        errors.is_empty(),
        "Go structural validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_go_fixture_semantic() {
    let graph = build_fixture_graph("go");
    let errors = validate_semantic(&graph);
    assert!(
        errors.is_empty(),
        "Go semantic validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_go_fixture_relational() {
    let graph = build_fixture_graph("go");
    let errors = validate_relational(&graph);
    assert!(
        errors.is_empty(),
        "Go relational validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_go_fixture_entities() {
    let graph = build_fixture_graph("go");

    // Check expected entities - structs and interfaces
    assert_entity_exists(&graph, "User", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "Calculator", NodeType::Container, Some("type"));
    assert_entity_exists(
        &graph,
        "SimpleCalculator",
        NodeType::Container,
        Some("type"),
    );
    assert_entity_exists(&graph, "AsyncProcessor", NodeType::Container, Some("type"));
    assert_entity_exists(
        &graph,
        "NewSimpleCalculator",
        NodeType::Callable,
        Some("function"),
    );
    assert_entity_exists(
        &graph,
        "StandaloneFunction",
        NodeType::Callable,
        Some("function"),
    );
    assert_entity_exists(&graph, "Square", NodeType::Callable, Some("function"));
}

#[test]
fn test_go_fixture_full() {
    let graph = build_fixture_graph("go");
    assert_graph_valid(&graph, "go");

    let stats = compute_stats(&graph);
    assert!(stats.file_count >= 1, "Should have at least 1 FILE node");
    assert!(
        stats.container_count >= 3,
        "Should have at least 3 Container nodes"
    );
    assert!(
        stats.callable_count >= 5,
        "Should have at least 5 Callable nodes"
    );
}

// ============================================================================
// Rust Fixture Tests
// ============================================================================

#[test]
fn test_rust_fixture_structural() {
    let graph = build_fixture_graph("rust");
    let errors = validate_structural(&graph);
    assert!(
        errors.is_empty(),
        "Rust structural validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_rust_fixture_semantic() {
    let graph = build_fixture_graph("rust");
    let errors = validate_semantic(&graph);
    assert!(
        errors.is_empty(),
        "Rust semantic validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_rust_fixture_relational() {
    let graph = build_fixture_graph("rust");
    let errors = validate_relational(&graph);
    assert!(
        errors.is_empty(),
        "Rust relational validation failed:\n{}",
        errors.join("\n")
    );
}

#[test]
fn test_rust_fixture_entities() {
    let graph = build_fixture_graph("rust");

    // Check expected entities - structs, traits, and impl methods
    assert_entity_exists(&graph, "User", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "UserRole", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "Calculator", NodeType::Container, Some("type"));
    assert_entity_exists(&graph, "Repository", NodeType::Container, Some("type"));
    assert_entity_exists(
        &graph,
        "SimpleCalculator",
        NodeType::Container,
        Some("type"),
    );
    assert_entity_exists(&graph, "AsyncProcessor", NodeType::Container, Some("type"));
    assert_entity_exists(
        &graph,
        "standalone_function",
        NodeType::Callable,
        Some("function"),
    );
    assert_entity_exists(
        &graph,
        "async_standalone",
        NodeType::Callable,
        Some("function"),
    );
}

#[test]
fn test_rust_fixture_full() {
    let graph = build_fixture_graph("rust");
    assert_graph_valid(&graph, "rust");

    let stats = compute_stats(&graph);
    assert!(stats.file_count >= 1, "Should have at least 1 FILE node");
    assert!(
        stats.container_count >= 5,
        "Should have at least 5 Container nodes"
    );
    assert!(
        stats.callable_count >= 5,
        "Should have at least 5 Callable nodes"
    );
}

// ============================================================================
// Cross-Language Summary Test
// ============================================================================

/// Run all fixture tests and report summary statistics.
#[test]
fn test_all_fixtures_summary() {
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

    println!("\n=== Integration Test Summary ===\n");

    for lang in &languages {
        let graph = build_fixture_graph(lang);
        let result = validate_all(&graph);
        let stats = compute_stats(&graph);

        let status = if result.is_valid() { "PASS" } else { "FAIL" };

        println!(
            "{:12} {} | Nodes: {:3} (F:{}, C:{}, L:{}, D:{}) | Edges: {:3}",
            lang,
            status,
            stats.total_nodes(),
            stats.file_count,
            stats.container_count,
            stats.callable_count,
            stats.data_count,
            stats.total_edges()
        );

        if !result.is_valid() {
            for err in result.all_errors().iter().take(3) {
                println!("    Error: {}", err);
            }
        }
    }

    println!();
}
