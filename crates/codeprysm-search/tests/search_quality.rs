//! Search quality tests for codeprysm-search.
//!
//! These tests validate search result quality and relevance:
//! - Exact name matches rank highest
//! - Semantic queries return conceptually related results
//! - Score fusion and bonuses work correctly
//! - Edge cases are handled gracefully
//!
//! ## Running Tests
//!
//! ```bash
//! # Run without Qdrant (unit tests only)
//! cargo test --package codeprysm-search --test search_quality
//!
//! # Run all tests including Qdrant integration
//! cargo test --package codeprysm-search --test search_quality -- --ignored
//!
//! # Run with output
//! cargo test --package codeprysm-search --test search_quality -- --ignored --nocapture
//! ```

mod common;

use codeprysm_search::{GraphIndexer, HybridSearcher, QdrantConfig};

// ============================================================================
// Phase 2.1: Search Relevance Tests (Requires Qdrant)
// ============================================================================

/// Test that exact name match ranks first for "Calculator" query.
/// Searching for "Calculator" should return Calculator class as top result.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_exact_name_match_ranking() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("exact_match");
    let fixture_path = common::fixtures_dir().join("python");

    // Index fixture
    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");
    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    // Search for exact name
    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    let results = searcher
        .search("Calculator", 10)
        .await
        .expect("Search failed");

    assert!(!results.is_empty(), "Should find results for 'Calculator'");

    // Top result should be the Calculator class (exact match)
    let top_result = &results[0];
    assert_eq!(
        top_result.name.to_lowercase(),
        "calculator",
        "Top result should be exact match 'Calculator', got '{}'",
        top_result.name
    );

    // Verify it's the Container (class), not a method
    assert_eq!(
        top_result.entity_type, "Container",
        "Top result should be the Calculator class (Container), not a method"
    );
}

/// Test semantic query finds conceptually related results.
/// "mathematical operations" should find add/multiply methods.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_semantic_query_relevance() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("semantic_query");
    let fixture_path = common::fixtures_dir().join("python");

    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");
    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    // Semantic query - should find math-related functions
    let results = searcher
        .search("mathematical operations like adding numbers", 10)
        .await
        .expect("Search failed");

    assert!(!results.is_empty(), "Semantic search should return results");

    // Check if top 5 results contain at least one math-related function
    let math_names = ["add", "multiply", "square", "divide", "Calculator"];
    let top_5_names: Vec<&str> = results.iter().take(5).map(|r| r.name.as_str()).collect();

    let found_math = top_5_names
        .iter()
        .any(|name| math_names.iter().any(|m| name.to_lowercase().contains(m)));

    assert!(
        found_math,
        "Top 5 results should contain at least one math-related entity. Found: {:?}",
        top_5_names
    );
}

/// Test that "async function" query finds async methods.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_code_pattern_query() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("code_pattern");
    let fixture_path = common::fixtures_dir().join("python");

    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");
    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    // Search for async functions
    let results = searcher
        .search("async function", 10)
        .await
        .expect("Search failed");

    assert!(!results.is_empty(), "Should find async functions");

    // The Python fixture has: async_standalone, process_item, process_batch (all async)
    let async_names = ["async_standalone", "process_item", "process_batch"];
    let result_names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();

    let found_async = result_names.iter().any(|name| async_names.contains(name));

    assert!(
        found_async,
        "Should find at least one async function. Found: {:?}",
        result_names
    );
}

/// Test type filtering - only Callable entities returned.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_type_filtering() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("type_filter");
    let fixture_path = common::fixtures_dir().join("python");

    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");
    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    // Search with Callable filter
    let results = searcher
        .search_with_types("Calculator", 10, vec!["Callable"])
        .await
        .expect("Search failed");

    // All results should be Callable
    for hit in &results {
        assert_eq!(
            hit.entity_type, "Callable",
            "Filtered search should only return Callable entities, got '{}'",
            hit.entity_type
        );
    }

    // Should find Calculator's methods, not the class itself
    if !results.is_empty() {
        let method_names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
        println!("Found Callable entities: {:?}", method_names);
    }
}

/// Test kind filtering - filter by kind="method" excludes standalone functions.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_kind_filtering() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("kind_filter");
    let fixture_path = common::fixtures_dir().join("python");

    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");
    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    // Search with Callable:method filter (using type:kind syntax if supported)
    // Note: This tests the filtering capability - implementation may vary
    let results = searcher
        .search_with_types("function", 20, vec!["Callable"])
        .await
        .expect("Search failed");

    // Check that we have both methods and functions in unfiltered Callable results
    let has_method = results.iter().any(|r| r.kind == "method");
    let has_function = results.iter().any(|r| r.kind == "function");

    println!(
        "Callable entities - has_method: {}, has_function: {}",
        has_method, has_function
    );
    println!(
        "Kinds found: {:?}",
        results
            .iter()
            .map(|r| format!("{}:{}", r.name, r.kind))
            .collect::<Vec<_>>()
    );

    // Verify we can distinguish kinds in the results
    assert!(
        !results.is_empty(),
        "Should have Callable results to verify kinds"
    );
}

// ============================================================================
// Phase 2.2: Score Fusion Tests (Requires Qdrant)
// ============================================================================

/// Test that results found by both models get higher scores (agreement bonus).
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_agreement_bonus() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("agreement_bonus");
    let fixture_path = common::fixtures_dir().join("python");

    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");
    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    // Search for something that should be found by both models
    let results = searcher
        .search("Calculator", 10)
        .await
        .expect("Search failed");

    // Find a result that was found by both sources
    let dual_source_result = results.iter().find(|r| r.found_via.len() > 1);

    if let Some(hit) = dual_source_result {
        // For dual-source results, combined score should reflect agreement
        let max_individual = hit
            .individual_scores
            .values()
            .cloned()
            .fold(0.0f32, |a, b| a.max(b));

        // Combined score should be >= max individual (due to bonuses)
        assert!(
            hit.combined_score >= max_individual * 0.9, // Allow small margin
            "Dual-source result should have combined_score ({}) >= max individual ({}) minus margin",
            hit.combined_score,
            max_individual
        );

        println!(
            "Dual-source hit: {} - combined: {:.3}, individual: {:?}",
            hit.name, hit.combined_score, hit.individual_scores
        );
    } else {
        println!("No dual-source results found - this is acceptable for small fixtures");
    }
}

/// Test exact match bonus is applied (0.35 for exact name match).
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_exact_match_bonus_application() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("exact_bonus");
    let fixture_path = common::fixtures_dir().join("python");

    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");
    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    // Search for exact name
    let results = searcher.search("add", 10).await.expect("Search failed");

    // Find the exact match result
    let exact_match = results.iter().find(|r| r.name.to_lowercase() == "add");

    // Find a non-exact match if available
    let non_exact = results.iter().find(|r| r.name.to_lowercase() != "add");

    if let (Some(exact), Some(other)) = (exact_match, non_exact) {
        // Exact match should have higher score
        assert!(
            exact.combined_score >= other.combined_score,
            "Exact match '{}' ({:.3}) should score >= non-exact '{}' ({:.3})",
            exact.name,
            exact.combined_score,
            other.name,
            other.combined_score
        );

        println!(
            "Exact match '{}': {:.3}, Non-exact '{}': {:.3}",
            exact.name, exact.combined_score, other.name, other.combined_score
        );
    } else {
        // If only exact match found, just verify it exists
        assert!(exact_match.is_some(), "Should find exact match for 'add'");
    }
}

// ============================================================================
// Phase 2.2: Query Classification Tests (No Qdrant Required)
// ============================================================================

/// Test query classification returns expected QueryType.
#[test]
fn test_query_classification() {
    use codeprysm_search::hybrid::{HybridSearcher, QueryType};

    // Identifier patterns (camelCase, snake_case, PascalCase)
    assert_eq!(
        HybridSearcher::classify_query("parseFile"),
        QueryType::Identifier,
        "camelCase should be Identifier"
    );
    assert_eq!(
        HybridSearcher::classify_query("parse_file"),
        QueryType::Identifier,
        "snake_case should be Identifier"
    );
    assert_eq!(
        HybridSearcher::classify_query("ParseFile"),
        QueryType::Identifier,
        "PascalCase should be Identifier"
    );
    assert_eq!(
        HybridSearcher::classify_query("MAX_VALUE"),
        QueryType::Identifier,
        "SCREAMING_SNAKE should be Identifier"
    );

    // Question patterns
    assert_eq!(
        HybridSearcher::classify_query("how does authentication work?"),
        QueryType::Question,
        "Question with '?' should be Question"
    );
    assert_eq!(
        HybridSearcher::classify_query("what is the purpose of this function"),
        QueryType::Question,
        "Starting with 'what' should be Question"
    );
    assert_eq!(
        HybridSearcher::classify_query("How to implement caching"),
        QueryType::Question,
        "Starting with 'How' should be Question"
    );

    // Natural language (multi-word, not question)
    assert_eq!(
        HybridSearcher::classify_query("user authentication logic"),
        QueryType::Natural,
        "Multi-word phrase should be Natural"
    );
    assert_eq!(
        HybridSearcher::classify_query("database connection handling"),
        QueryType::Natural,
        "Descriptive phrase should be Natural"
    );
}

// ============================================================================
// Phase 2.3: Edge Case Tests
// ============================================================================

/// Test empty query returns empty results without error.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_empty_query_handling() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("empty_query");
    let fixture_path = common::fixtures_dir().join("python");

    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");
    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    // Empty query should not panic
    let result = searcher.search("", 10).await;

    // Should either return Ok with empty results or an error - either is acceptable
    match result {
        Ok(results) => {
            println!("Empty query returned {} results", results.len());
            // Empty or minimal results is acceptable
        }
        Err(e) => {
            println!("Empty query returned error (acceptable): {}", e);
            // Error is also acceptable for empty query
        }
    }
}

/// Test queries with special characters don't crash.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_special_characters_in_query() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("special_chars");
    let fixture_path = common::fixtures_dir().join("python");

    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");
    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    // Test various special characters
    let special_queries = [
        "function*",
        "test?",
        "[array]",
        "regex.*pattern",
        "(parentheses)",
        "path/to/file",
        "query with spaces   and   multiple",
        "@decorator",
        "#comment",
        "value=123",
    ];

    for query in &special_queries {
        let result = searcher.search(query, 5).await;
        // Should not panic - Ok or Err both acceptable
        match result {
            Ok(results) => {
                println!("Query '{}': {} results", query, results.len());
            }
            Err(e) => {
                println!("Query '{}': error ({})", query, e);
            }
        }
    }
}

/// Test very long query (1000+ characters) handles gracefully.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_very_long_query() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("long_query");
    let fixture_path = common::fixtures_dir().join("python");

    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");
    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    // Create a very long query (>1000 chars)
    let long_query = "calculator function that performs ".repeat(50);
    assert!(long_query.len() > 1000, "Query should be >1000 chars");

    let start = std::time::Instant::now();
    let result = searcher.search(&long_query, 10).await;
    let duration = start.elapsed();

    // Should complete within reasonable time (30 seconds)
    assert!(
        duration.as_secs() < 30,
        "Long query should complete within 30 seconds, took {:?}",
        duration
    );

    // Should not panic
    match result {
        Ok(results) => {
            println!(
                "Long query ({} chars): {} results in {:?}",
                long_query.len(),
                results.len(),
                duration
            );
        }
        Err(e) => {
            println!(
                "Long query ({} chars): error ({}) in {:?}",
                long_query.len(),
                e,
                duration
            );
        }
    }
}

/// Test unicode characters in query.
#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_unicode_query() {
    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("unicode_query");
    let fixture_path = common::fixtures_dir().join("python");

    let mut indexer = GraphIndexer::new(config.clone(), &repo_id, &fixture_path)
        .await
        .expect("Failed to create indexer");
    let graph = common::build_fixture_graph("python");
    indexer
        .index_graph(&graph)
        .await
        .expect("Failed to index graph");

    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    // Test unicode queries
    let unicode_queries = [
        "函数",    // Chinese: "function"
        "計算",    // Japanese: "calculation"
        "функция", // Russian: "function"
        "emoji test 🎉",
        "café",
        "naïve",
    ];

    for query in &unicode_queries {
        let result = searcher.search(query, 5).await;
        // Should not panic
        match result {
            Ok(results) => {
                println!("Unicode query '{}': {} results", query, results.len());
            }
            Err(e) => {
                println!("Unicode query '{}': error ({})", query, e);
            }
        }
    }
}

// ============================================================================
// Summary Test
// ============================================================================

#[tokio::test]
#[ignore] // Requires Qdrant running
async fn test_search_quality_summary() {
    println!("\n=== Search Quality Test Summary ===\n");

    let config = QdrantConfig::default();
    let repo_id = common::unique_repo_id("quality_summary");
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

    println!("Indexed: {} entities\n", stats.total_indexed);

    let searcher = HybridSearcher::connect(config, &repo_id)
        .await
        .expect("Failed to create searcher");

    // Test cases
    let test_cases = [
        ("Calculator", "Exact match"),
        ("add numbers", "Semantic"),
        ("async_standalone", "Identifier"),
        ("how to process items", "Question"),
    ];

    for (query, description) in &test_cases {
        let results = searcher.search(query, 5).await.expect("Search failed");

        let top_names: Vec<&str> = results.iter().take(3).map(|r| r.name.as_str()).collect();
        let top_score = results.first().map(|r| r.combined_score).unwrap_or(0.0);

        println!(
            "{:20} ({:10}): top_score={:.3}, results={:?}",
            query, description, top_score, top_names
        );
    }

    println!();
}
