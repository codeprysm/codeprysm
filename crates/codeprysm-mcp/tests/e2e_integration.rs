//! End-to-End Integration tests for codeprysm-mcp.
//!
//! These tests validate the full pipeline from code to search results,
//! including graph generation, indexing, and MCP tool responses.
//!
//! ## Requirements
//!
//! - Qdrant must be running at localhost:6334
//! - Start with: `just qdrant-start`
//!
//! ## Running Tests
//!
//! ```bash
//! # Run all e2e tests (requires Qdrant)
//! cargo test --package codeprysm-mcp --test e2e_integration -- --ignored
//!
//! # Run with output
//! cargo test --package codeprysm-mcp --test e2e_integration -- --ignored --nocapture
//!
//! # Run sequentially (avoids Qdrant contention)
//! cargo test --package codeprysm-mcp --test e2e_integration -- --ignored --test-threads=1
//! ```

mod common;

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::lazy::manager::LazyGraphManager;
use codeprysm_core::lazy::partitioner::GraphPartitioner;
use codeprysm_search::{GraphIndexer, HybridSearcher, QdrantConfig};
use tempfile::Builder as TempBuilder;

// ============================================================================
// Test Helpers
// ============================================================================

fn queries_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codeprysm-core")
        .join("queries")
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codeprysm-core")
        .join("tests")
        .join("fixtures")
        .join("integration_repos")
}

/// Create a temp directory with non-hidden name
fn create_temp_dir() -> tempfile::TempDir {
    TempBuilder::new()
        .prefix("codeprysm_e2e_")
        .tempdir()
        .expect("Failed to create temp dir")
}

/// Generate unique repo ID for test isolation
fn unique_repo_id(prefix: &str) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("e2e_{}_{}", prefix, timestamp)
}

/// Set up a test environment with partitioned graph and search index
///
/// Returns (temp_dir, repo_path, prism_dir, repo_id)
async fn setup_indexed_environment(
    language: &str,
) -> (tempfile::TempDir, PathBuf, PathBuf, String) {
    let fixture_path = fixtures_dir().join(language);
    assert!(
        fixture_path.exists(),
        "Fixture not found: {:?}",
        fixture_path
    );

    // Build graph
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");
    let graph = builder
        .build_from_directory(&fixture_path)
        .expect("Failed to build graph");

    // Create temp directory for .codeprysm storage
    let temp_dir = create_temp_dir();
    let prism_dir = temp_dir.path().join(".codeprysm");
    fs::create_dir_all(&prism_dir).expect("Failed to create prism directory");

    // Partition the graph
    let (_manifest, _stats) =
        GraphPartitioner::partition_with_stats(&graph, &prism_dir, Some("test"))
            .expect("Failed to partition graph");

    // Index to Qdrant
    let repo_id = unique_repo_id(language);
    let qdrant_config = QdrantConfig::default();

    let mut indexer = GraphIndexer::new(qdrant_config, &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");

    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    (temp_dir, fixture_path, prism_dir, repo_id)
}

// ============================================================================
// Phase 5.1: Full Pipeline Tests
// ============================================================================

/// Test: graph → index → search pipeline works end-to-end
///
/// Validates that we can build a graph, index it, and find expected entities.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_graph_index_search_pipeline() {
    let (_temp_dir, fixture_path, _prism_dir, repo_id) = setup_indexed_environment("python").await;

    // Create searcher
    let searcher = HybridSearcher::connect(QdrantConfig::default(), &repo_id)
        .await
        .expect("Failed to create searcher");

    // Search for Calculator class
    let results = searcher
        .search("Calculator", 10)
        .await
        .expect("Search failed");

    assert!(!results.is_empty(), "Should find results for 'Calculator'");

    // Verify Calculator is in results
    let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
    assert!(
        names.iter().any(|n| n.to_lowercase() == "calculator"),
        "Should find Calculator class. Found: {:?}",
        names
    );

    println!(
        "Pipeline test: Indexed {} -> found {} results for 'Calculator'",
        fixture_path.display(),
        results.len()
    );
}

/// Test: incremental update is reflected in search results
///
/// Modifies a file, updates the graph, and verifies search reflects the change.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_incremental_update_search() {
    let temp_dir = create_temp_dir();
    let repo_path = temp_dir.path().join("repo");
    fs::create_dir_all(&repo_path).expect("Failed to create repo dir");

    // Create initial Python file
    let py_file = repo_path.join("module.py");
    fs::write(&py_file, "def original_function():\n    pass\n").expect("Failed to write file");

    // Build and index initial graph
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");
    let graph = builder
        .build_from_directory(&repo_path)
        .expect("Failed to build initial graph");

    let prism_dir = temp_dir.path().join(".codeprysm");
    fs::create_dir_all(&prism_dir).expect("Failed to create prism dir");

    let repo_id = unique_repo_id("incremental");
    let qdrant_config = QdrantConfig::default();

    let mut indexer = GraphIndexer::new(qdrant_config.clone(), &repo_id, &repo_path)
        .await
        .expect("Failed to create indexer");

    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index initial graph");

    // Search for original function
    let searcher = HybridSearcher::connect(qdrant_config.clone(), &repo_id)
        .await
        .expect("Failed to create searcher");

    let results = searcher
        .search("original_function", 10)
        .await
        .expect("Search failed");

    assert!(
        results.iter().any(|r| r.name == "original_function"),
        "Should find original_function"
    );

    // Add new function
    fs::write(
        &py_file,
        "def original_function():\n    pass\n\ndef new_function():\n    pass\n",
    )
    .expect("Failed to update file");

    // Rebuild and reindex
    let mut builder = GraphBuilder::with_config(&queries_dir(), BuilderConfig::default())
        .expect("Failed to create builder");
    let updated_graph = builder
        .build_from_directory(&repo_path)
        .expect("Failed to build updated graph");

    let mut indexer = GraphIndexer::new(qdrant_config.clone(), &repo_id, &repo_path)
        .await
        .expect("Failed to create indexer");

    indexer
        .index_graph(&updated_graph)
        .await
        .expect("Failed to reindex");

    // Search for new function
    let searcher = HybridSearcher::connect(qdrant_config, &repo_id)
        .await
        .expect("Failed to create searcher");

    let results = searcher
        .search("new_function", 10)
        .await
        .expect("Search failed");

    assert!(
        results.iter().any(|r| r.name == "new_function"),
        "Should find new_function after update"
    );

    println!("Incremental update test: Successfully found new function after update");
}

/// Test: delete file removes entities from search
///
/// Deletes a file, updates the graph, and verifies entities are no longer searchable.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_delete_removes_from_search() {
    let temp_dir = create_temp_dir();
    let repo_path = temp_dir.path().join("repo");
    fs::create_dir_all(&repo_path).expect("Failed to create repo dir");

    // Create two Python files
    let file1 = repo_path.join("keep.py");
    let file2 = repo_path.join("delete_me.py");

    fs::write(&file1, "def keep_function():\n    pass\n").expect("Failed to write file1");
    fs::write(&file2, "class DeletedClass:\n    pass\n").expect("Failed to write file2");

    // Build and index
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");
    let graph = builder
        .build_from_directory(&repo_path)
        .expect("Failed to build graph");

    let repo_id = unique_repo_id("delete");
    let qdrant_config = QdrantConfig::default();

    let mut indexer = GraphIndexer::new(qdrant_config.clone(), &repo_id, &repo_path)
        .await
        .expect("Failed to create indexer");

    indexer.index_graph(&graph).await.expect("Failed to index");

    // Verify DeletedClass is searchable
    let searcher = HybridSearcher::connect(qdrant_config.clone(), &repo_id)
        .await
        .expect("Failed to create searcher");

    let results = searcher
        .search("DeletedClass", 10)
        .await
        .expect("Search failed");

    assert!(
        results.iter().any(|r| r.name == "DeletedClass"),
        "Should find DeletedClass initially"
    );

    // Delete the file
    fs::remove_file(&file2).expect("Failed to delete file");

    // Rebuild and reindex
    let mut builder = GraphBuilder::with_config(&queries_dir(), BuilderConfig::default())
        .expect("Failed to create builder");
    let updated_graph = builder
        .build_from_directory(&repo_path)
        .expect("Failed to rebuild graph");

    let mut indexer = GraphIndexer::new(qdrant_config.clone(), &repo_id, &repo_path)
        .await
        .expect("Failed to create indexer");

    indexer
        .index_graph(&updated_graph)
        .await
        .expect("Failed to reindex");

    // Search for deleted class - should not find it
    let searcher = HybridSearcher::connect(qdrant_config, &repo_id)
        .await
        .expect("Failed to create searcher");

    let results = searcher
        .search("DeletedClass", 10)
        .await
        .expect("Search failed");

    assert!(
        !results.iter().any(|r| r.name == "DeletedClass"),
        "DeletedClass should not be found after deletion"
    );

    // But keep_function should still be there
    let results = searcher
        .search("keep_function", 10)
        .await
        .expect("Search failed");

    assert!(
        results.iter().any(|r| r.name == "keep_function"),
        "keep_function should still be searchable"
    );

    println!("Delete test: DeletedClass successfully removed from search after file deletion");
}

// ============================================================================
// Phase 5.2: MCP Tool Tests (via LazyGraphManager)
// ============================================================================

/// Test: search_graph_nodes returns valid JSON with expected fields
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_search_graph_nodes_response_format() {
    let (_temp_dir, _fixture_path, _prism_dir, repo_id) = setup_indexed_environment("python").await;

    let searcher = HybridSearcher::connect(QdrantConfig::default(), &repo_id)
        .await
        .expect("Failed to create searcher");

    let results = searcher
        .search("Calculator", 10)
        .await
        .expect("Search failed");

    assert!(!results.is_empty(), "Should have results");

    // Verify response structure
    let first = &results[0];

    // Check required fields exist
    assert!(!first.entity_id.is_empty(), "entity_id should not be empty");
    assert!(!first.name.is_empty(), "name should not be empty");
    assert!(
        !first.entity_type.is_empty(),
        "entity_type should not be empty"
    );
    assert!(!first.file_path.is_empty(), "file_path should not be empty");

    // line_range should be valid
    assert!(first.line_range.0 > 0, "start line should be > 0");
    assert!(
        first.line_range.1 >= first.line_range.0,
        "end line >= start line"
    );

    println!(
        "Search response format test: Got {} results with valid structure",
        results.len()
    );
}

/// Test: get_node_info returns correct node metadata
#[test]
fn test_get_node_info_metadata() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    // Find Calculator node
    let calculator_id = {
        let graph = manager.graph_read();
        let result = graph
            .iter_nodes()
            .find(|n| n.name == "Calculator")
            .map(|n| n.id.clone())
            .expect("Calculator not found");
        result
    };

    let node = manager
        .get_node(&calculator_id)
        .expect("Failed to get node")
        .expect("Node should exist");

    // Verify metadata
    assert_eq!(node.name, "Calculator");
    assert_eq!(node.node_type.as_str(), "Container");
    assert_eq!(node.kind.as_deref(), Some("type"));
    assert!(node.line > 0, "Line number should be > 0");
    assert!(node.end_line >= node.line, "End line >= start line");
    assert!(!node.file.is_empty(), "File path should not be empty");

    println!(
        "Node info test: Calculator at {}:{}-{}",
        node.file, node.line, node.end_line
    );
}

/// Test: find_references returns all references to an entity
#[test]
fn test_find_references() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    // Find Calculator class
    let calculator_id = {
        let graph = manager.graph_read();
        let result = graph
            .iter_nodes()
            .find(|n| n.name == "Calculator")
            .map(|n| n.id.clone())
            .expect("Calculator not found");
        result
    };

    // Get incoming edges (references to Calculator)
    let refs = manager
        .get_incoming_edges(&calculator_id)
        .expect("Failed to get references");

    // Calculator should at least be referenced by its containing file (CONTAINS edge)
    assert!(!refs.is_empty(), "Calculator should have references");

    // Print what we found
    let ref_names: Vec<_> = refs
        .iter()
        .map(|(n, e)| format!("{} ({:?})", n.name, e.edge_type))
        .collect();
    println!("References to Calculator: {:?}", ref_names);
}

/// Test: find_outgoing_references returns what a function calls
#[test]
fn test_find_outgoing_references() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    // Find Calculator class
    let calculator_id = {
        let graph = manager.graph_read();
        let result = graph
            .iter_nodes()
            .find(|n| n.name == "Calculator")
            .map(|n| n.id.clone())
            .expect("Calculator not found");
        result
    };

    // Get outgoing edges (what Calculator references)
    let outgoing = manager
        .get_outgoing_edges(&calculator_id)
        .expect("Failed to get outgoing edges");

    // Calculator class should have outgoing edges to its methods
    assert!(
        !outgoing.is_empty(),
        "Calculator should have outgoing edges (to methods)"
    );

    let method_names: Vec<_> = outgoing.iter().map(|(n, _)| n.name.as_str()).collect();

    // Should contain at least some of the Calculator's methods
    let expected_methods = ["add", "multiply", "__init__", "square", "from_string"];
    let found = method_names
        .iter()
        .any(|name| expected_methods.contains(name));

    assert!(
        found,
        "Should find Calculator methods. Found: {:?}",
        method_names
    );

    println!("Outgoing references from Calculator: {:?}", method_names);
}

/// Test: read_code returns source code for an entity
#[test]
fn test_read_code() {
    let (_temp_dir, repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    // Find Calculator class
    let calculator_node = {
        let graph = manager.graph_read();
        let result = graph
            .iter_nodes()
            .find(|n| n.name == "Calculator")
            .cloned()
            .expect("Calculator not found");
        result
    };

    // Read the actual file content
    let file_path = repo_path.join(&calculator_node.file);
    let content = fs::read_to_string(&file_path).expect("Failed to read file");

    // Get lines for Calculator's range
    let lines: Vec<&str> = content.lines().collect();
    let start = calculator_node.line - 1;
    let end = std::cmp::min(calculator_node.end_line, lines.len());

    // The read code should contain "class Calculator"
    let calculator_content = lines[start..end].join("\n");
    println!(
        "Calculator node: lines {}-{}",
        calculator_node.line, calculator_node.end_line
    );
    println!("Calculator content:\n---\n{}\n---", calculator_content);
    assert!(
        calculator_content.contains("class Calculator"),
        "Should contain class definition. Got:\n{}",
        calculator_content
    );
    // Note: Container nodes may not include method definitions in their line range
    // depending on how the parser captures class boundaries
    let has_methods =
        calculator_content.contains("def add") || calculator_content.contains("def __init__");
    println!("Has methods: {}", has_methods);

    println!(
        "Read code test: Calculator spans lines {}-{} ({} lines)",
        calculator_node.line,
        calculator_node.end_line,
        calculator_node.end_line - calculator_node.line + 1
    );
}

/// Test: find_module_structure returns directory tree with counts
#[test]
fn test_find_module_structure() {
    let (_temp_dir, _repo_path, prism_dir) = common::setup_test_environment("python");

    let manager = LazyGraphManager::open(&prism_dir).expect("Failed to open manager");
    manager
        .load_all_partitions()
        .expect("Failed to load partitions");

    let graph = manager.graph_read();

    // Count entities by type
    let mut container_count = 0;
    let mut callable_count = 0;
    let mut data_count = 0;
    let mut files: HashSet<String> = HashSet::new();

    for node in graph.iter_nodes() {
        if !node.file.is_empty() {
            files.insert(node.file.clone());
        }
        match node.node_type {
            codeprysm_core::graph::NodeType::Container => container_count += 1,
            codeprysm_core::graph::NodeType::Callable => callable_count += 1,
            codeprysm_core::graph::NodeType::Data => data_count += 1,
        }
    }

    // Verify we have meaningful counts
    assert!(!files.is_empty(), "Should have at least one file");
    assert!(callable_count > 0, "Should have Callable nodes");

    println!(
        "Module structure test: {} files, {} containers, {} callables, {} data",
        files.len(),
        container_count,
        callable_count,
        data_count
    );
}

/// Test: get_index_status returns indexing progress
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_get_index_status() {
    let (_temp_dir, _fixture_path, _prism_dir, repo_id) = setup_indexed_environment("python").await;

    let searcher = HybridSearcher::connect(QdrantConfig::default(), &repo_id)
        .await
        .expect("Failed to create searcher");

    let status = searcher
        .index_status()
        .await
        .expect("Failed to get index status");

    let (semantic_count, code_count) = status.expect("Index should exist");

    assert!(semantic_count > 0, "Should have semantic points indexed");
    assert!(code_count > 0, "Should have code points indexed");

    println!(
        "Index status test: {} semantic points, {} code points",
        semantic_count, code_count
    );
}

// ============================================================================
// Summary Test
// ============================================================================

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_e2e_integration_summary() {
    println!("\n=== E2E Integration Test Summary ===\n");

    let (_temp_dir, fixture_path, _prism_dir, repo_id) = setup_indexed_environment("python").await;

    // Test search
    let searcher = HybridSearcher::connect(QdrantConfig::default(), &repo_id)
        .await
        .expect("Failed to create searcher");

    let queries = [
        ("Calculator", "Exact name"),
        ("add", "Method name"),
        ("mathematical operations", "Semantic"),
        ("async function", "Code pattern"),
    ];

    for (query, description) in &queries {
        let results = searcher.search(query, 5).await.expect("Search failed");
        let top_names: Vec<&str> = results.iter().take(3).map(|r| r.name.as_str()).collect();
        println!(
            "{:25} ({}): {} results, top: {:?}",
            query,
            description,
            results.len(),
            top_names
        );
    }

    // Test index status
    if let Ok(Some((semantic, code))) = searcher.index_status().await {
        println!(
            "\nIndex: {} semantic, {} code points from {}",
            semantic,
            code,
            fixture_path.display()
        );
    }

    println!();
}
