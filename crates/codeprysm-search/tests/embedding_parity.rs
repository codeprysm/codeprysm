//! Embedding Provider Parity Tests
//!
//! These tests compare embeddings across different providers to verify:
//! 1. Jina models produce identical results regardless of runtime (Local vs Azure ML)
//! 2. Different models (Jina vs Ollama/nomic) have reasonable similarity behavior
//!
//! ## Prerequisites
//!
//! - Local provider: Always available (uses candle for local inference)
//! - Azure ML: Requires CODEPRYSM_AZURE_ML_* environment variables
//! - Ollama: Requires Ollama running with nomic-embed-text model
//!
//! ## Running Tests
//!
//! ```bash
//! # Run parity tests (requires all providers)
//! cargo test --package codeprysm-search --test embedding_parity --features metal --release -- --ignored --nocapture
//! ```

use codeprysm_search::{
    AzureMLProvider, EmbeddingProvider, LocalProvider, OpenAIConfig, OpenAIProvider,
};

/// Check if Azure ML is configured
fn azure_ml_configured() -> bool {
    std::env::var("CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT").is_ok()
        && std::env::var("CODEPRYSM_AZURE_ML_CODE_ENDPOINT").is_ok()
        && std::env::var("CODEPRYSM_AZURE_ML_API_KEY").is_ok()
}

/// Check if Ollama is running
async fn ollama_available() -> bool {
    let client = reqwest::Client::new();
    client
        .get("http://localhost:11434/v1/models")
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// Calculate cosine similarity between two vectors
fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "Vectors must have same dimension");
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

/// Calculate mean squared error between two vectors
fn mean_squared_error(a: &[f32], b: &[f32]) -> f32 {
    assert_eq!(a.len(), b.len(), "Vectors must have same dimension");
    let sum: f32 = a.iter().zip(b.iter()).map(|(x, y)| (x - y).powi(2)).sum();
    sum / a.len() as f32
}

// ============================================================================
// Local vs Azure ML Parity Tests (Same Jina Models)
// ============================================================================

#[tokio::test]
#[ignore] // Requires Azure ML credentials and local model
async fn test_local_vs_azure_ml_semantic_parity() {
    if !azure_ml_configured() {
        eprintln!("Skipping: Azure ML not configured");
        return;
    }

    println!("=== Local vs Azure ML Semantic Parity Test ===\n");

    // Create providers
    let local = LocalProvider::new().expect("Failed to create local provider");
    let azure = AzureMLProvider::from_env().expect("Failed to create Azure ML provider");

    // Test texts
    let texts = vec![
        "Calculate the sum of two numbers".to_string(),
        "A function that adds integers together".to_string(),
        "Machine learning for natural language processing".to_string(),
    ];

    // Encode with both providers
    let local_embeddings = local
        .encode_semantic(texts.clone())
        .await
        .expect("Local encoding failed");
    let azure_embeddings = azure
        .encode_semantic(texts.clone())
        .await
        .expect("Azure encoding failed");

    assert_eq!(local_embeddings.len(), azure_embeddings.len());

    println!("Comparing {} embeddings:\n", texts.len());

    for (i, text) in texts.iter().enumerate() {
        let local_emb = &local_embeddings[i];
        let azure_emb = &azure_embeddings[i];

        assert_eq!(
            local_emb.len(),
            azure_emb.len(),
            "Embedding dimensions should match"
        );

        let similarity = cosine_similarity(local_emb, azure_emb);
        let mse = mean_squared_error(local_emb, azure_emb);

        println!("Text {}: \"{}...\"", i + 1, &text[..text.len().min(40)]);
        println!("  Dimension: {} vs {}", local_emb.len(), azure_emb.len());
        println!("  Cosine Similarity: {:.6}", similarity);
        println!("  Mean Squared Error: {:.8}", mse);
        println!();

        // Same model should produce very similar results
        // Allow small differences due to floating point and normalization
        assert!(
            similarity > 0.99,
            "Jina embeddings should be nearly identical: {} (expected > 0.99)",
            similarity
        );
        assert!(
            mse < 0.001,
            "MSE should be very small: {} (expected < 0.001)",
            mse
        );
    }

    println!("Parity check PASSED");
}

#[tokio::test]
#[ignore] // Requires Azure ML credentials and local model
async fn test_local_vs_azure_ml_code_parity() {
    if !azure_ml_configured() {
        eprintln!("Skipping: Azure ML not configured");
        return;
    }

    println!("=== Local vs Azure ML Code Parity Test ===\n");

    let local = LocalProvider::new().expect("Failed to create local provider");
    let azure = AzureMLProvider::from_env().expect("Failed to create Azure ML provider");

    let code_samples = vec![
        "fn calculate_sum(a: i32, b: i32) -> i32 { a + b }".to_string(),
        "def greet(name): return f\"Hello, {name}!\"".to_string(),
        "class Calculator { add(a, b) { return a + b; } }".to_string(),
    ];

    let local_embeddings = local
        .encode_code(code_samples.clone())
        .await
        .expect("Local encoding failed");
    let azure_embeddings = azure
        .encode_code(code_samples.clone())
        .await
        .expect("Azure encoding failed");

    println!("Comparing {} code embeddings:\n", code_samples.len());

    for (i, code) in code_samples.iter().enumerate() {
        let local_emb = &local_embeddings[i];
        let azure_emb = &azure_embeddings[i];

        let similarity = cosine_similarity(local_emb, azure_emb);
        let mse = mean_squared_error(local_emb, azure_emb);

        println!("Code {}: \"{}...\"", i + 1, &code[..code.len().min(50)]);
        println!("  Cosine Similarity: {:.6}", similarity);
        println!("  MSE: {:.8}", mse);
        println!();

        assert!(
            similarity > 0.99,
            "Code embeddings should be nearly identical: {}",
            similarity
        );
    }

    println!("Code parity check PASSED");
}

// ============================================================================
// Cross-Provider Similarity Tests (Different Models)
// ============================================================================

#[tokio::test]
#[ignore] // Requires Ollama running
async fn test_local_vs_ollama_similarity_behavior() {
    if !ollama_available().await {
        eprintln!("Skipping: Ollama not available");
        return;
    }

    println!("=== Local (Jina) vs Ollama (nomic) Similarity Behavior Test ===\n");

    let local = LocalProvider::new().expect("Failed to create local provider");
    let ollama =
        OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create Ollama provider");

    // Test: Related queries should have higher similarity than unrelated
    let queries = vec![
        "How do I calculate the sum of two numbers?".to_string(), // Related to query 1
        "What is the result of adding two integers?".to_string(), // Related to query 0
        "The weather forecast for tomorrow".to_string(),          // Unrelated
    ];

    let local_embeddings = local
        .encode_semantic(queries.clone())
        .await
        .expect("Local encoding failed");
    let ollama_embeddings = ollama
        .encode_semantic(queries.clone())
        .await
        .expect("Ollama encoding failed");

    println!("Testing similarity ranking consistency:\n");

    // Calculate similarities for both providers
    let local_sim_related = cosine_similarity(&local_embeddings[0], &local_embeddings[1]);
    let local_sim_unrelated = cosine_similarity(&local_embeddings[0], &local_embeddings[2]);

    let ollama_sim_related = cosine_similarity(&ollama_embeddings[0], &ollama_embeddings[1]);
    let ollama_sim_unrelated = cosine_similarity(&ollama_embeddings[0], &ollama_embeddings[2]);

    println!("Local (Jina) similarities:");
    println!("  Related (0,1): {:.4}", local_sim_related);
    println!("  Unrelated (0,2): {:.4}", local_sim_unrelated);
    println!(
        "  Difference: {:.4}",
        local_sim_related - local_sim_unrelated
    );
    println!();

    println!("Ollama (nomic) similarities:");
    println!("  Related (0,1): {:.4}", ollama_sim_related);
    println!("  Unrelated (0,2): {:.4}", ollama_sim_unrelated);
    println!(
        "  Difference: {:.4}",
        ollama_sim_related - ollama_sim_unrelated
    );
    println!();

    // Both providers should rank related queries higher
    assert!(
        local_sim_related > local_sim_unrelated,
        "Local: Related should be more similar than unrelated"
    );
    assert!(
        ollama_sim_related > ollama_sim_unrelated,
        "Ollama: Related should be more similar than unrelated"
    );

    println!("Both providers correctly rank related queries higher - PASSED");
}

#[tokio::test]
#[ignore] // Requires Azure ML and Ollama
async fn test_all_providers_similarity_ranking() {
    let has_azure = azure_ml_configured();
    let has_ollama = ollama_available().await;

    if !has_azure && !has_ollama {
        eprintln!("Skipping: Neither Azure ML nor Ollama available");
        return;
    }

    println!("=== All Providers Similarity Ranking Test ===\n");

    let local = LocalProvider::new().expect("Failed to create local provider");

    // Test texts with known relationships
    let texts = vec![
        "Authentication and user login".to_string(),
        "User authentication and session management".to_string(),
        "Database query optimization".to_string(),
    ];

    // Local results
    let local_embeddings = local
        .encode_semantic(texts.clone())
        .await
        .expect("Local failed");
    let local_sim_01 = cosine_similarity(&local_embeddings[0], &local_embeddings[1]);
    let local_sim_02 = cosine_similarity(&local_embeddings[0], &local_embeddings[2]);

    println!(
        "Local (Jina) - sim(auth, auth) = {:.4}, sim(auth, db) = {:.4}",
        local_sim_01, local_sim_02
    );
    assert!(
        local_sim_01 > local_sim_02,
        "Local: auth topics should be more similar"
    );

    // Azure ML results (if available)
    if has_azure {
        let azure = AzureMLProvider::from_env().expect("Failed to create Azure ML provider");
        let azure_embeddings = azure
            .encode_semantic(texts.clone())
            .await
            .expect("Azure failed");
        let azure_sim_01 = cosine_similarity(&azure_embeddings[0], &azure_embeddings[1]);
        let azure_sim_02 = cosine_similarity(&azure_embeddings[0], &azure_embeddings[2]);

        println!(
            "Azure ML (Jina) - sim(auth, auth) = {:.4}, sim(auth, db) = {:.4}",
            azure_sim_01, azure_sim_02
        );
        assert!(
            azure_sim_01 > azure_sim_02,
            "Azure: auth topics should be more similar"
        );

        // Local and Azure should produce very similar rankings
        let ranking_match = (local_sim_01 > local_sim_02) == (azure_sim_01 > azure_sim_02);
        assert!(ranking_match, "Local and Azure should produce same ranking");
    }

    // Ollama results (if available)
    if has_ollama {
        let ollama = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create Ollama");
        let ollama_embeddings = ollama
            .encode_semantic(texts.clone())
            .await
            .expect("Ollama failed");
        let ollama_sim_01 = cosine_similarity(&ollama_embeddings[0], &ollama_embeddings[1]);
        let ollama_sim_02 = cosine_similarity(&ollama_embeddings[0], &ollama_embeddings[2]);

        println!(
            "Ollama (nomic) - sim(auth, auth) = {:.4}, sim(auth, db) = {:.4}",
            ollama_sim_01, ollama_sim_02
        );
        assert!(
            ollama_sim_01 > ollama_sim_02,
            "Ollama: auth topics should be more similar"
        );
    }

    println!("\nAll available providers produce consistent similarity rankings - PASSED");
}

// ============================================================================
// Dimension Consistency Tests
// ============================================================================

#[tokio::test]
async fn test_local_provider_dimension() {
    let provider = LocalProvider::new().expect("Failed to create provider");

    // Jina models use 768 dimensions
    assert_eq!(
        provider.embedding_dim(),
        768,
        "Local provider should use 768 dimensions"
    );

    let embeddings = provider
        .encode_semantic(vec!["test".into()])
        .await
        .expect("Encoding failed");

    assert_eq!(
        embeddings[0].len(),
        768,
        "Actual embeddings should be 768-dim"
    );
}

#[tokio::test]
#[ignore] // Requires Azure ML
async fn test_azure_ml_provider_dimension() {
    if !azure_ml_configured() {
        eprintln!("Skipping: Azure ML not configured");
        return;
    }

    let provider = AzureMLProvider::from_env().expect("Failed to create provider");

    assert_eq!(
        provider.embedding_dim(),
        768,
        "Azure ML should use 768 dimensions"
    );

    let embeddings = provider
        .encode_semantic(vec!["test".into()])
        .await
        .expect("Encoding failed");

    assert_eq!(
        embeddings[0].len(),
        768,
        "Actual embeddings should be 768-dim"
    );
}

#[tokio::test]
#[ignore] // Requires Ollama
async fn test_ollama_provider_dimension() {
    if !ollama_available().await {
        eprintln!("Skipping: Ollama not available");
        return;
    }

    let provider = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create provider");

    // nomic-embed-text uses 768 dimensions
    let embeddings = provider
        .encode_semantic(vec!["test".into()])
        .await
        .expect("Encoding failed");

    println!("Ollama nomic-embed-text dimension: {}", embeddings[0].len());

    // nomic-embed-text is 768-dim like Jina
    assert_eq!(
        embeddings[0].len(),
        768,
        "nomic-embed-text should be 768-dim"
    );
}

// ============================================================================
// Performance Comparison
// ============================================================================

#[tokio::test]
#[ignore] // Requires all providers
async fn test_provider_latency_comparison() {
    let has_azure = azure_ml_configured();
    let has_ollama = ollama_available().await;

    println!("=== Provider Latency Comparison ===\n");

    // Local provider
    let local = LocalProvider::new().expect("Failed to create local provider");
    local.warmup().await.expect("Local warmup failed");

    let test_text = vec!["Performance test for embedding latency measurement".to_string()];
    let iterations = 5;

    // Local latency
    let mut local_times = Vec::new();
    for _ in 0..iterations {
        let start = std::time::Instant::now();
        local
            .encode_semantic(test_text.clone())
            .await
            .expect("Encoding failed");
        local_times.push(start.elapsed());
    }
    let local_avg = local_times.iter().sum::<std::time::Duration>() / iterations as u32;
    println!(
        "Local (Jina/Candle): {:?} avg over {} iterations",
        local_avg, iterations
    );

    // Azure ML latency
    if has_azure {
        let azure = AzureMLProvider::from_env().expect("Failed to create Azure provider");
        azure.warmup().await.expect("Azure warmup failed");

        let mut azure_times = Vec::new();
        for _ in 0..iterations {
            let start = std::time::Instant::now();
            azure
                .encode_semantic(test_text.clone())
                .await
                .expect("Encoding failed");
            azure_times.push(start.elapsed());
        }
        let azure_avg = azure_times.iter().sum::<std::time::Duration>() / iterations as u32;
        println!(
            "Azure ML (Jina remote): {:?} avg over {} iterations",
            azure_avg, iterations
        );
    } else {
        println!("Azure ML: Not configured");
    }

    // Ollama latency
    if has_ollama {
        let ollama = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create Ollama");
        ollama.warmup().await.expect("Ollama warmup failed");

        let mut ollama_times = Vec::new();
        for _ in 0..iterations {
            let start = std::time::Instant::now();
            ollama
                .encode_semantic(test_text.clone())
                .await
                .expect("Encoding failed");
            ollama_times.push(start.elapsed());
        }
        let ollama_avg = ollama_times.iter().sum::<std::time::Duration>() / iterations as u32;
        println!(
            "Ollama (nomic local): {:?} avg over {} iterations",
            ollama_avg, iterations
        );
    } else {
        println!("Ollama: Not available");
    }

    println!("\nLatency comparison complete");
}

// ============================================================================
// Summary Test
// ============================================================================

#[tokio::test]
#[ignore] // Requires external providers
async fn test_embedding_parity_summary() {
    let has_azure = azure_ml_configured();
    let has_ollama = ollama_available().await;

    println!("\n=== Embedding Provider Parity Summary ===\n");

    println!("Available Providers:");
    println!("  Local (Jina/Candle): Always available");
    println!(
        "  Azure ML (Jina remote): {}",
        if has_azure {
            "Configured"
        } else {
            "Not configured"
        }
    );
    println!(
        "  Ollama (nomic local): {}",
        if has_ollama {
            "Available"
        } else {
            "Not available"
        }
    );
    println!();

    let local = LocalProvider::new().expect("Failed to create local provider");
    local.warmup().await.expect("Local warmup failed");

    let test_texts = vec![
        "Calculate the sum of two numbers".to_string(),
        "Add two integers together".to_string(),
    ];

    let local_emb = local
        .encode_semantic(test_texts.clone())
        .await
        .expect("Local failed");
    let local_sim = cosine_similarity(&local_emb[0], &local_emb[1]);
    println!("Local provider similarity test: {:.4}", local_sim);

    if has_azure {
        let azure = AzureMLProvider::from_env().expect("Failed to create Azure provider");
        let azure_emb = azure
            .encode_semantic(test_texts.clone())
            .await
            .expect("Azure failed");
        let azure_sim = cosine_similarity(&azure_emb[0], &azure_emb[1]);
        println!("Azure ML similarity test: {:.4}", azure_sim);

        // Parity check
        let parity = cosine_similarity(&local_emb[0], &azure_emb[0]);
        println!("Local-Azure parity (same input): {:.6}", parity);
        assert!(
            parity > 0.99,
            "Local and Azure should produce near-identical embeddings"
        );
    }

    if has_ollama {
        let ollama = OpenAIProvider::new(OpenAIConfig::ollama()).expect("Failed to create Ollama");
        let ollama_emb = ollama
            .encode_semantic(test_texts.clone())
            .await
            .expect("Ollama failed");
        let ollama_sim = cosine_similarity(&ollama_emb[0], &ollama_emb[1]);
        println!("Ollama similarity test: {:.4}", ollama_sim);
    }

    println!("\n=== Parity Summary Complete ===\n");
}
