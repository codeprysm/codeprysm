//! OpenAI-Compatible Provider Integration Tests (Ollama)
//!
//! These tests verify the OpenAI provider works with Ollama locally.
//! Ollama must be running with the nomic-embed-text model installed.
//!
//! ## Setup
//!
//! ```bash
//! # Install Ollama (macOS)
//! brew install ollama
//!
//! # Start Ollama server
//! ollama serve
//!
//! # Pull embedding model (in another terminal)
//! ollama pull nomic-embed-text
//! ```
//!
//! ## Running Tests
//!
//! ```bash
//! # Run Ollama integration tests
//! cargo test --package codeprysm-search --test openai_integration --release -- --ignored --nocapture
//! ```
//!
//! These tests are marked `#[ignore]` to prevent running in CI without Ollama.

use codeprysm_search::{EmbeddingProvider, EmbeddingProviderType, OpenAIConfig, OpenAIProvider};

/// Check if Ollama is running and accessible
async fn ollama_available() -> bool {
    let client = reqwest::Client::new();
    client
        .get("http://localhost:11434/v1/models")
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Skip test if Ollama is not available
macro_rules! require_ollama {
    () => {
        if !ollama_available().await {
            eprintln!("Skipping: Ollama not running at http://localhost:11434");
            eprintln!("Start with: ollama serve");
            eprintln!("Install model: ollama pull nomic-embed-text");
            return;
        }
    };
}

// ============================================================================
// Provider Creation Tests
// ============================================================================

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_create_ollama_provider() {
    require_ollama!();

    let config = OpenAIConfig::ollama();
    let result = OpenAIProvider::new(config);

    assert!(
        result.is_ok(),
        "Should create Ollama provider: {:?}",
        result.err()
    );

    let provider = result.unwrap();
    assert_eq!(provider.provider_type(), EmbeddingProviderType::Openai);
}

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_create_provider_custom_model() {
    require_ollama!();

    let config = OpenAIConfig {
        base_url: "http://localhost:11434/v1".into(),
        api_key: None,
        semantic_model: "nomic-embed-text".into(),
        code_model: None,
        timeout_secs: 60,
        max_retries: 3,
        azure_mode: false,
    };

    let result = OpenAIProvider::new(config);
    assert!(result.is_ok(), "Should create provider: {:?}", result.err());
}

// ============================================================================
// Health Check Tests
// ============================================================================

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_provider_status() {
    require_ollama!();

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");
    let status = provider.check_status().await.expect("Status check failed");

    println!("Ollama Provider Status:");
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
}

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_provider_warmup() {
    require_ollama!();

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");
    let result = provider.warmup().await;

    assert!(result.is_ok(), "Warmup should succeed: {:?}", result.err());
}

// ============================================================================
// Semantic Embedding Tests
// ============================================================================

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_encode_semantic_single() {
    require_ollama!();

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");
    let result = provider.encode_semantic(vec!["Hello, world!".into()]).await;

    assert!(
        result.is_ok(),
        "Semantic encoding should succeed: {:?}",
        result.err()
    );

    let embeddings = result.unwrap();
    assert_eq!(embeddings.len(), 1, "Should return one embedding");
    assert!(
        !embeddings[0].is_empty(),
        "Embedding should have dimensions"
    );

    // nomic-embed-text uses 768 dimensions
    println!("Embedding dimension: {}", embeddings[0].len());
    println!(
        "First 5 values: {:?}",
        &embeddings[0][..5.min(embeddings[0].len())]
    );

    // Verify it's not all zeros
    let sum: f32 = embeddings[0].iter().sum();
    assert!(sum.abs() > 0.01, "Embedding should not be all zeros");
}

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_encode_semantic_batch() {
    require_ollama!();

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");
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

    let dim = embeddings[0].len();
    for (i, emb) in embeddings.iter().enumerate() {
        assert_eq!(emb.len(), dim, "Embedding {} should have same dimension", i);
    }

    println!("Encoded 3 texts successfully, dimension: {}", dim);
}

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_encode_semantic_empty() {
    require_ollama!();

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");
    let result = provider.encode_semantic(vec![]).await;

    assert!(result.is_ok(), "Empty input should succeed");
    assert!(result.unwrap().is_empty(), "Should return empty list");
}

// ============================================================================
// Code Embedding Tests
// ============================================================================

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_encode_code_single() {
    require_ollama!();

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");
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
    assert!(
        !embeddings[0].is_empty(),
        "Embedding should have dimensions"
    );

    println!("Code embedding dimension: {}", embeddings[0].len());
}

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_encode_code_multiple_languages() {
    require_ollama!();

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");

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
#[ignore] // Requires Ollama running
async fn test_semantic_similarity() {
    require_ollama!();

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");

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
#[ignore] // Requires Ollama running
async fn test_code_similarity() {
    require_ollama!();

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");

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

    // Note: nomic-embed-text is a general model, not code-specific
    // Similarity differences may be smaller than with Jina code model
    println!("(nomic-embed-text is not code-specialized, differences may be subtle)");
}

// ============================================================================
// Performance Tests
// ============================================================================

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_encoding_latency() {
    require_ollama!();

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");

    // Warmup - Ollama may need to load model
    println!("Warming up (loading model)...");
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

    // Ollama is local, should be reasonably fast
    assert!(
        single_latency.as_secs() < 30,
        "Single encoding too slow: {:?}",
        single_latency
    );
}

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_dimension_detection() {
    require_ollama!();

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");

    // Initial dimension is estimated
    let initial_dim = provider.embedding_dim();
    println!("Initial estimated dimension: {}", initial_dim);

    // After encoding, dimension is detected from response
    let embeddings = provider
        .encode_semantic(vec!["test".into()])
        .await
        .expect("Encoding failed");

    let actual_dim = embeddings[0].len();
    let detected_dim = provider.embedding_dim();

    println!("Actual dimension: {}", actual_dim);
    println!("Detected dimension: {}", detected_dim);

    assert_eq!(
        actual_dim, detected_dim,
        "Detected dimension should match actual"
    );
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[tokio::test]
async fn test_connection_refused() {
    // Test error handling when server is not running
    let config = OpenAIConfig {
        base_url: "http://localhost:99999/v1".into(), // Invalid port
        api_key: None,
        semantic_model: "nomic-embed-text".into(),
        code_model: None,
        timeout_secs: 5,
        max_retries: 0,
        azure_mode: false,
    };

    let provider = OpenAIProvider::new(config).expect("Should create provider");
    let result = provider.encode_semantic(vec!["test".into()]).await;

    assert!(result.is_err(), "Should fail with connection error");
    println!("Expected error: {}", result.unwrap_err());
}

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_invalid_model() {
    require_ollama!();

    let config = OpenAIConfig {
        base_url: "http://localhost:11434/v1".into(),
        api_key: None,
        semantic_model: "nonexistent-model-xyz".into(),
        code_model: None,
        timeout_secs: 10,
        max_retries: 0,
        azure_mode: false,
    };

    let provider = OpenAIProvider::new(config).expect("Should create provider");
    let result = provider.encode_semantic(vec!["test".into()]).await;

    assert!(result.is_err(), "Should fail with invalid model");
    println!("Expected error: {}", result.unwrap_err());
}

// ============================================================================
// Configuration Tests
// ============================================================================

#[test]
fn test_ollama_config_defaults() {
    let config = OpenAIConfig::ollama();

    assert_eq!(config.base_url, "http://localhost:11434/v1");
    assert!(config.api_key.is_none());
    assert_eq!(config.semantic_model, "nomic-embed-text");
    assert!(config.code_model.is_none());
    assert!(!config.azure_mode);
}

#[test]
fn test_config_with_timeout() {
    let config = OpenAIConfig::ollama().with_timeout(120);
    assert_eq!(config.timeout_secs, 120);
}

#[test]
fn test_config_with_max_retries() {
    let config = OpenAIConfig::ollama().with_max_retries(5);
    assert_eq!(config.max_retries, 5);
}

#[test]
fn test_effective_code_model() {
    let config = OpenAIConfig::ollama();
    assert_eq!(config.effective_code_model(), "nomic-embed-text");

    let config_with_code = OpenAIConfig {
        code_model: Some("code-llama".into()),
        ..OpenAIConfig::ollama()
    };
    assert_eq!(config_with_code.effective_code_model(), "code-llama");
}

// ============================================================================
// Summary Test
// ============================================================================

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_ollama_integration_summary() {
    require_ollama!();

    println!("\n=== Ollama Provider Integration Test Summary ===\n");

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");

    // Check status
    let status = provider.check_status().await.expect("Status check failed");
    println!("Provider Status: {:?}", status.available);
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

    if let Ok(ref emb) = semantic_result {
        println!("  Dimension: {}", emb[0].len());
    }

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
    println!("\nSimilarity test: {:.4} (related queries)", similarity);

    println!("\n=== Integration Test Complete ===\n");

    assert!(semantic_result.is_ok());
    assert!(code_result.is_ok());
}
