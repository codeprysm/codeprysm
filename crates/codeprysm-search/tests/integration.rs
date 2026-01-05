//! Integration tests for codeprysm-search.
//!
//! These tests require Qdrant running at localhost:6334.
//! Start with: `just qdrant-start`
//!
//! ## Running Tests
//!
//! ```bash
//! # Run all tests (ignored tests require Qdrant)
//! cargo test --package codeprysm-search --test integration
//!
//! # Run Qdrant integration tests
//! cargo test --package codeprysm-search --test integration -- --ignored
//!
//! # Run with output
//! cargo test --package codeprysm-search --test integration -- --ignored --nocapture
//! ```

mod common;

use codeprysm_search::{GraphIndexer, HybridSearcher, QdrantConfig, QdrantStore};

// ============================================================================
// Configuration Tests (No Qdrant Required)
// ============================================================================

#[test]
fn test_qdrant_config_creation() {
    let config = QdrantConfig::default();
    assert_eq!(config.url, "http://localhost:6334");
    assert!(config.api_key.is_none());

    let config = QdrantConfig::with_url("http://custom:6334").api_key("secret");
    assert_eq!(config.url, "http://custom:6334");
    assert_eq!(config.api_key, Some("secret".to_string()));
}

#[test]
fn test_fixture_graph_building() {
    // Ensure fixtures work without Qdrant
    let graph = common::build_fixture_graph("python");
    let node_count = graph.iter_nodes().count();
    assert!(node_count > 0, "Python fixture should have nodes");
}

// ============================================================================
// Qdrant Connection Tests (Requires Qdrant)
// ============================================================================

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_qdrant_connection() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("conn_test");

    let store = QdrantStore::connect(config, &repo_id).await;
    assert!(store.is_ok(), "Should connect to Qdrant: {:?}", store.err());
}

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_collection_lifecycle() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("lifecycle_test");

    let store = QdrantStore::connect(config, &repo_id)
        .await
        .expect("Failed to connect");

    // Ensure collections exist
    store
        .ensure_collections()
        .await
        .expect("Failed to create collections");

    // Verify collections exist
    let semantic_exists = store
        .collection_exists("semantic_search")
        .await
        .expect("Failed to check semantic collection");
    let code_exists = store
        .collection_exists("code_search")
        .await
        .expect("Failed to check code collection");

    assert!(semantic_exists, "Semantic collection should exist");
    assert!(code_exists, "Code collection should exist");
}

// ============================================================================
// Indexing Tests (Requires Qdrant)
// ============================================================================

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_index_fixture_graph() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("index_test");
    let fixture_path = common::fixtures_dir().join("python");

    // Create indexer
    let mut indexer = GraphIndexer::new(config, &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");

    // Build and index graph
    let graph = common::build_fixture_graph("python");
    let stats = indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    // Verify indexing succeeded
    assert!(stats.total_processed > 0, "Should process entities");
    assert!(stats.total_indexed > 0, "Should index entities");
    // Some failures are expected (e.g., FILE nodes without code content)
    // As long as most entities are indexed, the test passes
    let success_rate = stats.total_indexed as f64 / stats.total_processed as f64;
    assert!(
        success_rate > 0.7,
        "Should index at least 70% of entities, got {:.1}% ({} of {} indexed)",
        success_rate * 100.0,
        stats.total_indexed,
        stats.total_processed
    );

    println!(
        "Indexed: {} processed, {} indexed, {} skipped, {} failed",
        stats.total_processed, stats.total_indexed, stats.total_skipped, stats.total_failed
    );
}

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_index_multiple_languages() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("multi_lang_test");

    let languages = ["python", "javascript", "typescript", "rust", "go"];

    for language in &languages {
        let lang_repo = format!("{}_{}", repo_id, language);
        let fixture_path = common::fixtures_dir().join(language);

        let mut indexer = GraphIndexer::new(config.clone(), &lang_repo, &fixture_path)
            .await
            .expect("Failed to create indexer");

        let graph = common::build_fixture_graph(language);
        let stats = indexer
            .index_graph(&graph)
            .await
            .unwrap_or_else(|_| panic!("Failed to index {}", language));

        assert!(
            stats.total_indexed > 0,
            "{}: Should index at least some entities",
            language
        );

        println!(
            "{:12} OK | processed: {:3} | indexed: {:3}",
            language, stats.total_processed, stats.total_indexed
        );
    }
}

// ============================================================================
// Search Tests (Requires Qdrant)
// ============================================================================

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_hybrid_search_basic() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("search_test");
    let fixture_path = common::fixtures_dir().join("python");

    // Index first
    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");

    let graph = common::build_fixture_graph("python");
    let stats = indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    assert!(stats.total_indexed > 0, "Should have indexed entities");

    // Now search
    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    let results = searcher
        .search("Calculator", 10)
        .await
        .expect("Search failed");

    // Should find Calculator class
    assert!(!results.is_empty(), "Should find results for 'Calculator'");

    let found_calculator = results.iter().any(|hit| hit.name.contains("Calculator"));
    assert!(
        found_calculator,
        "Should find Calculator in results: {:?}",
        results.iter().map(|r| &r.name).collect::<Vec<_>>()
    );
}

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_hybrid_search_semantic() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("semantic_test");
    let fixture_path = common::fixtures_dir().join("python");

    // Index
    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");

    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    // Search with semantic query
    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    let results = searcher
        .search("mathematical operations like adding numbers", 10)
        .await
        .expect("Search failed");

    // Should find math-related functions
    assert!(!results.is_empty(), "Semantic search should return results");

    println!("Semantic search results:");
    for (i, hit) in results.iter().enumerate().take(5) {
        println!("  {}: {} ({:.3})", i + 1, hit.name, hit.combined_score);
    }
}

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_search_with_type_filter() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("filter_test");
    let fixture_path = common::fixtures_dir().join("python");

    // Index
    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");

    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    // Search with type filter
    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    let results = searcher
        .search_with_types("Calculator", 10, vec!["Callable"])
        .await
        .expect("Search failed");

    // Should only find Callable entities (methods/functions)
    for hit in &results {
        assert_eq!(
            hit.entity_type, "Callable",
            "Filtered search should only return Callable, got: {}",
            hit.entity_type
        );
    }
}

// ============================================================================
// Multi-Tenant Isolation Tests (Requires Qdrant)
// ============================================================================

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_repo_isolation() {
    let config = QdrantConfig::default();
    let repo_a = common::unique_repo_id("repo_a");
    let repo_b = common::unique_repo_id("repo_b");

    // Index Python to repo_a
    let python_fixture = common::fixtures_dir().join("python");
    let mut indexer_a = GraphIndexer::new(config.clone(), &repo_a, &python_fixture)
        .await
        .expect("Failed to create indexer A");
    let python_graph = common::build_fixture_graph("python");
    indexer_a
        .index_graph(&python_graph)
        .await
        .expect("Failed to index Python");

    // Index JavaScript to repo_b
    let js_fixture = common::fixtures_dir().join("javascript");
    let mut indexer_b = GraphIndexer::new(config.clone(), &repo_b, &js_fixture)
        .await
        .expect("Failed to create indexer B");
    let js_graph = common::build_fixture_graph("javascript");
    indexer_b
        .index_graph(&js_graph)
        .await
        .expect("Failed to index JavaScript");

    // Search repo_a for "Calculator" - should find Python Calculator
    let searcher_a = HybridSearcher::connect(config.clone(), &repo_a)
        .await
        .expect("Failed to create searcher A");
    let results_a = searcher_a
        .search("Calculator", 10)
        .await
        .expect("Search A failed");

    // Search repo_b for "Calculator" - should find JavaScript Calculator
    let searcher_b = HybridSearcher::connect(config, &repo_b)
        .await
        .expect("Failed to create searcher B");
    let results_b = searcher_b
        .search("Calculator", 10)
        .await
        .expect("Search B failed");

    // Both should find results, but they should be from different repos
    assert!(
        !results_a.is_empty(),
        "Repo A should have Calculator results"
    );
    assert!(
        !results_b.is_empty(),
        "Repo B should have Calculator results"
    );

    // Verify files are different (Python vs JS)
    let files_a: Vec<_> = results_a.iter().map(|h| &h.file_path).collect();
    let files_b: Vec<_> = results_b.iter().map(|h| &h.file_path).collect();

    let has_python_files = files_a.iter().any(|f| f.ends_with(".py"));
    let has_js_files = files_b.iter().any(|f| f.ends_with(".js"));

    assert!(has_python_files, "Repo A should have Python files");
    assert!(has_js_files, "Repo B should have JavaScript files");
}

// ============================================================================
// Cleanup Tests (Requires Qdrant)
// ============================================================================

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_delete_repo_points() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("cleanup_test");
    let fixture_path = common::fixtures_dir().join("python");

    // Index
    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");

    let graph = common::build_fixture_graph("python");
    let stats = indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    assert!(stats.total_indexed > 0, "Should have indexed points");

    // Get store and delete
    let store = QdrantStore::connect(config.clone(), &repo_id)
        .await
        .expect("Failed to connect");

    store
        .delete_repo_points("semantic_search")
        .await
        .expect("Failed to delete semantic points");

    store
        .delete_repo_points("code_search")
        .await
        .expect("Failed to delete code points");

    // Verify search returns empty (or no results for this repo)
    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    let results = searcher
        .search("Calculator", 10)
        .await
        .expect("Search failed");

    assert!(
        results.is_empty(),
        "After cleanup, search should return no results"
    );
}

// ============================================================================
// Summary Test
// ============================================================================

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_search_integration_summary() {
    println!("\n=== codeprysm-search Integration Test Summary ===\n");

    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("summary_test");

    // Index multiple languages
    let languages = ["python", "javascript", "typescript"];

    for language in &languages {
        let lang_repo = format!("{}_{}", repo_id, language);
        let fixture_path = common::fixtures_dir().join(language);

        let mut indexer = GraphIndexer::new(config.clone(), &lang_repo, &fixture_path)
            .await
            .expect("Failed to create indexer");

        let graph = common::build_fixture_graph(language);
        let stats = indexer.index_graph(&graph).await.expect("Failed to index");

        // Quick search test
        let searcher = HybridSearcher::connect(config.clone(), &lang_repo)
            .await
            .expect("Failed to create searcher");

        let results = searcher
            .search("Calculator", 5)
            .await
            .expect("Search failed");

        println!(
            "{:12} PASS | indexed: {:3} | search results: {:2}",
            language,
            stats.total_indexed,
            results.len()
        );
    }

    println!();
}
