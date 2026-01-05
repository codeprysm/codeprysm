//! Azure ML Provider Integration Tests
//!
//! These tests require Azure ML endpoints to be configured and accessible.
//! Set the following environment variables (or use direnv with .envrc):
//!
//! - `CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT` - Semantic endpoint URL
//! - `CODEPRYSM_AZURE_ML_CODE_ENDPOINT` - Code endpoint URL
//! - `CODEPRYSM_AZURE_ML_SEMANTIC_API_KEY` - API key for semantic endpoint
//! - `CODEPRYSM_AZURE_ML_CODE_API_KEY` - API key for code endpoint
//!
//! Or for backwards compatibility (single key for both):
//! - `CODEPRYSM_AZURE_ML_API_KEY` - Shared API key
//!
//! ## Running Tests
//!
//! ```bash
//! # Run Azure ML integration tests (requires credentials)
//! cargo test --package codeprysm-search --test azure_ml_integration --release -- --ignored --nocapture
//! ```
//!
//! These tests are marked `#[ignore]` to prevent running in CI without credentials.

use codeprysm_search::{
    AzureMLAuth, AzureMLConfig, AzureMLProvider, EmbeddingProvider, EmbeddingProviderType,
};

/// Check if Azure ML environment variables are set
fn azure_ml_configured() -> bool {
    let endpoints_ok = std::env::var("CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT").is_ok()
        && std::env::var("CODEPRYSM_AZURE_ML_CODE_ENDPOINT").is_ok();

    // Accept either separate keys or legacy single key
    let keys_ok = (std::env::var("CODEPRYSM_AZURE_ML_SEMANTIC_API_KEY").is_ok()
        && std::env::var("CODEPRYSM_AZURE_ML_CODE_API_KEY").is_ok())
        || std::env::var("CODEPRYSM_AZURE_ML_API_KEY").is_ok();

    endpoints_ok && keys_ok
}

/// Skip test if Azure ML is not configured
macro_rules! require_azure_ml {
    () => {
        if !azure_ml_configured() {
            eprintln!("Skipping: Azure ML environment variables not set");
            eprintln!("Set CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT, CODEPRYSM_AZURE_ML_CODE_ENDPOINT");
            eprintln!(
                "And either CODEPRYSM_AZURE_ML_SEMANTIC_API_KEY + CODEPRYSM_AZURE_ML_CODE_API_KEY"
            );
            eprintln!("Or legacy CODEPRYSM_AZURE_ML_API_KEY");
            return;
        }
    };
}

// ============================================================================
// Provider Creation Tests
// ============================================================================

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_create_provider_from_env() {
    require_azure_ml!();

    let result = AzureMLProvider::from_env();
    assert!(
        result.is_ok(),
        "Should create provider from env: {:?}",
        result.err()
    );

    let provider = result.unwrap();
    assert_eq!(provider.provider_type(), EmbeddingProviderType::AzureMl);
    assert_eq!(provider.embedding_dim(), 768);
}

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_create_provider_with_config() {
    require_azure_ml!();

    let config = AzureMLConfig {
        semantic_endpoint: std::env::var("CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT").unwrap(),
        code_endpoint: std::env::var("CODEPRYSM_AZURE_ML_CODE_ENDPOINT").unwrap(),
        semantic_auth: AzureMLAuth::ApiKeyEnv("CODEPRYSM_AZURE_ML_SEMANTIC_API_KEY".into()),
        code_auth: Some(AzureMLAuth::ApiKeyEnv(
            "CODEPRYSM_AZURE_ML_CODE_API_KEY".into(),
        )),
        timeout_secs: 30,
        max_retries: 3,
    };

    let result = AzureMLProvider::new(config);
    assert!(
        result.is_ok(),
        "Should create provider with config: {:?}",
        result.err()
    );
}

// ============================================================================
// Health Check Tests
// ============================================================================

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_provider_status() {
    require_azure_ml!();

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");
    let status = provider.check_status().await.expect("Status check failed");

    println!("Azure ML Provider Status:");
    println!("  Available: {}", status.available);
    println!("  Semantic Ready: {}", status.semantic_ready);
    println!("  Code Ready: {}", status.code_ready);
    println!("  Device: {}", status.device);
    if let Some(latency) = status.latency_ms {
        println!("  Latency: {}ms", latency);
    }
    if let Some(ref error) = status.error {
        println!("  Error: {}", error);
    }

    assert!(status.available, "Provider should be available");
    assert!(status.semantic_ready, "Semantic endpoint should be ready");
    assert!(status.code_ready, "Code endpoint should be ready");
    assert!(status.all_ready(), "All endpoints should be ready");
}

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_provider_warmup() {
    require_azure_ml!();

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");
    let result = provider.warmup().await;

    assert!(result.is_ok(), "Warmup should succeed: {:?}", result.err());
}

// ============================================================================
// Semantic Embedding Tests
// ============================================================================

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_encode_semantic_single() {
    require_azure_ml!();

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");
    let result = provider.encode_semantic(vec!["Hello, world!".into()]).await;

    assert!(
        result.is_ok(),
        "Semantic encoding should succeed: {:?}",
        result.err()
    );

    let embeddings = result.unwrap();
    assert_eq!(embeddings.len(), 1, "Should return one embedding");
    assert_eq!(
        embeddings[0].len(),
        768,
        "Embedding should have 768 dimensions"
    );

    // Verify it's not all zeros
    let sum: f32 = embeddings[0].iter().sum();
    assert!(sum.abs() > 0.01, "Embedding should not be all zeros");

    println!(
        "Semantic embedding (first 5 dims): {:?}",
        &embeddings[0][..5]
    );
}

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_encode_semantic_batch() {
    require_azure_ml!();

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");
    let texts = vec![
        "The quick brown fox jumps over the lazy dog".into(),
        "A software engineer writes code to solve problems".into(),
        "Machine learning models learn patterns from data".into(),
    ];

    let result = provider.encode_semantic(texts).await;
    assert!(
        result.is_ok(),
        "Batch encoding should succeed: {:?}",
        result.err()
    );

    let embeddings = result.unwrap();
    assert_eq!(embeddings.len(), 3, "Should return 3 embeddings");

    for (i, emb) in embeddings.iter().enumerate() {
        assert_eq!(emb.len(), 768, "Embedding {} should have 768 dimensions", i);
    }

    println!("Encoded 3 texts successfully");
}

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_encode_semantic_empty() {
    require_azure_ml!();

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");
    let result = provider.encode_semantic(vec![]).await;

    assert!(result.is_ok(), "Empty input should succeed");
    assert!(result.unwrap().is_empty(), "Should return empty list");
}

// ============================================================================
// Code Embedding Tests
// ============================================================================

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_encode_code_single() {
    require_azure_ml!();

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");
    let code = r#"
fn calculate_sum(a: i32, b: i32) -> i32 {
    a + b
}
"#;

    let result = provider.encode_code(vec![code.into()]).await;
    assert!(
        result.is_ok(),
        "Code encoding should succeed: {:?}",
        result.err()
    );

    let embeddings = result.unwrap();
    assert_eq!(embeddings.len(), 1, "Should return one embedding");
    assert_eq!(
        embeddings[0].len(),
        768,
        "Embedding should have 768 dimensions"
    );

    println!("Code embedding (first 5 dims): {:?}", &embeddings[0][..5]);
}

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_encode_code_multiple_languages() {
    require_azure_ml!();

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");

    let code_samples = vec![
        // Rust
        "fn main() { println!(\"Hello, world!\"); }".to_string(),
        // Python
        "def greet(name): return f\"Hello, {name}!\"".to_string(),
        // JavaScript
        "const greet = (name) => `Hello, ${name}!`;".to_string(),
    ];

    let result = provider.encode_code(code_samples).await;
    assert!(
        result.is_ok(),
        "Multi-language encoding should succeed: {:?}",
        result.err()
    );

    let embeddings = result.unwrap();
    assert_eq!(embeddings.len(), 3, "Should return 3 embeddings");

    println!("Encoded code in 3 languages successfully");
}

// ============================================================================
// Similarity Tests
// ============================================================================

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (norm_a * norm_b)
}

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_semantic_similarity() {
    require_azure_ml!();

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");

    let texts = vec![
        "How do I add two numbers together?".into(),
        "What is the sum of two integers?".into(),
        "The weather is nice today".into(),
    ];

    let embeddings = provider
        .encode_semantic(texts)
        .await
        .expect("Encoding failed");

    let sim_related = cosine_similarity(&embeddings[0], &embeddings[1]);
    let sim_unrelated = cosine_similarity(&embeddings[0], &embeddings[2]);

    println!("Similarity (related queries): {:.4}", sim_related);
    println!("Similarity (unrelated queries): {:.4}", sim_unrelated);

    assert!(
        sim_related > sim_unrelated,
        "Related queries should have higher similarity: {} vs {}",
        sim_related,
        sim_unrelated
    );
}

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_code_similarity() {
    require_azure_ml!();

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");

    let code_samples = vec![
        // Two similar functions (adding numbers)
        "def add(a, b): return a + b".into(),
        "fn add(a: i32, b: i32) -> i32 { a + b }".into(),
        // Unrelated function (string manipulation)
        "fn reverse(s: &str) -> String { s.chars().rev().collect() }".into(),
    ];

    let embeddings = provider
        .encode_code(code_samples)
        .await
        .expect("Encoding failed");

    let sim_similar = cosine_similarity(&embeddings[0], &embeddings[1]);
    let sim_different = cosine_similarity(&embeddings[0], &embeddings[2]);

    println!("Similarity (similar functions): {:.4}", sim_similar);
    println!("Similarity (different functions): {:.4}", sim_different);

    assert!(
        sim_similar > sim_different,
        "Similar functions should have higher similarity: {} vs {}",
        sim_similar,
        sim_different
    );
}

// ============================================================================
// Performance Tests
// ============================================================================

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_encoding_latency() {
    require_azure_ml!();

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");

    // Warmup
    provider.warmup().await.expect("Warmup failed");

    // Measure single encoding
    let start = std::time::Instant::now();
    provider
        .encode_semantic(vec!["Latency test".into()])
        .await
        .expect("Encoding failed");
    let single_latency = start.elapsed();

    // Measure batch encoding
    let batch_texts: Vec<String> = (0..10)
        .map(|i| format!("Batch text number {}", i))
        .collect();
    let start = std::time::Instant::now();
    provider
        .encode_semantic(batch_texts)
        .await
        .expect("Batch encoding failed");
    let batch_latency = start.elapsed();

    println!("Single text latency: {:?}", single_latency);
    println!("Batch (10 texts) latency: {:?}", batch_latency);
    println!("Per-text latency in batch: {:?}", batch_latency / 10);

    // Sanity check: encoding should complete in reasonable time
    assert!(
        single_latency.as_secs() < 10,
        "Single encoding too slow: {:?}",
        single_latency
    );
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_invalid_api_key() {
    // This test doesn't require valid credentials - it tests error handling
    let config = AzureMLConfig {
        semantic_endpoint: "https://jina-semantic.eastus2.inference.ml.azure.com/score".into(),
        code_endpoint: "https://jina-code.eastus2.inference.ml.azure.com/score".into(),
        semantic_auth: AzureMLAuth::ApiKey("invalid-key".into()),
        code_auth: None, // Will use semantic_auth
        timeout_secs: 10,
        max_retries: 0, // No retries for faster test
    };

    let provider = AzureMLProvider::new(config).expect("Should create provider");
    let result = provider.encode_semantic(vec!["test".into()]).await;

    // Should fail with auth error
    assert!(result.is_err(), "Should fail with invalid key");
    let err = result.unwrap_err();
    println!("Expected error: {}", err);
}

#[tokio::test]
async fn test_missing_env_var() {
    // Test that missing env var is handled gracefully
    let config = AzureMLConfig {
        semantic_endpoint: "https://example.com/score".into(),
        code_endpoint: "https://example.com/score".into(),
        semantic_auth: AzureMLAuth::ApiKeyEnv("NONEXISTENT_CODEPRYSM_VAR_12345".into()),
        code_auth: None,
        timeout_secs: 10,
        max_retries: 0,
    };

    let result = AzureMLProvider::new(config);
    assert!(result.is_err(), "Should fail with missing env var");
    println!("Expected error: {}", result.unwrap_err());
}

// ============================================================================
// Summary Test
// ============================================================================

#[tokio::test]
#[ignore] // Requires Azure ML credentials
async fn test_azure_ml_integration_summary() {
    require_azure_ml!();

    println!("\n=== Azure ML Provider Integration Test Summary ===\n");

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");

    // Check status
    let status = provider.check_status().await.expect("Status check failed");
    println!("Provider Status: {:?}", status.available);
    println!("  Semantic: {}", status.semantic_ready);
    println!("  Code: {}", status.code_ready);
    if let Some(latency) = status.latency_ms {
        println!("  Latency: {}ms", latency);
    }

    // Test semantic encoding
    let semantic_result = provider
        .encode_semantic(vec!["Test semantic embedding".into()])
        .await;
    println!(
        "\nSemantic encoding: {}",
        if semantic_result.is_ok() {
            "PASS"
        } else {
            "FAIL"
        }
    );

    // Test code encoding
    let code_result = provider.encode_code(vec!["fn test() {}".into()]).await;
    println!(
        "Code encoding: {}",
        if code_result.is_ok() { "PASS" } else { "FAIL" }
    );

    // Test similarity
    let texts = vec![
        "Calculate the sum of two numbers".into(),
        "Add two integers together".into(),
    ];
    let embeddings = provider
        .encode_semantic(texts)
        .await
        .expect("Encoding failed");
    let similarity = cosine_similarity(&embeddings[0], &embeddings[1]);
    println!("\nSimilarity test: {:.4} (expected > 0.7)", similarity);

    println!("\n=== Integration Test Complete ===\n");

    assert!(semantic_result.is_ok());
    assert!(code_result.is_ok());
    assert!(
        similarity > 0.5,
        "Similar queries should have > 0.5 similarity"
    );
}
