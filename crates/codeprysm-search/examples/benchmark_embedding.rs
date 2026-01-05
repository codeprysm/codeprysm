//! Benchmark embedding performance across providers
//!
//! Usage:
//!   cargo run --example benchmark_embedding --features metal --release
//!   cargo run --example benchmark_embedding --features metal,rate-limit --release
//!
//! To test remote providers, set environment variables:
//!   CODEPRYSM_AZURE_ML_SEMANTIC_ENDPOINT=https://...
//!   CODEPRYSM_AZURE_ML_CODE_ENDPOINT=https://...
//!   CODEPRYSM_AZURE_ML_API_KEY=...
//!   CODEPRYSM_OPENAI_API_KEY=...

use std::time::{Duration, Instant};

use codeprysm_search::{
    create_provider, EmbeddingConfig, EmbeddingProvider, EmbeddingProviderType,
};

/// Test data representing typical code snippets
const TEST_TEXTS: &[&str] = &[
    "class Calculator:",
    "def add(self, x, y):",
    "async function fetchData(url)",
    "struct Node { id: String }",
    "function fibonacci(n) { return n <= 1 ? n : fibonacci(n-1) + fibonacci(n-2); }",
];

/// Benchmark result for a single run
#[allow(dead_code)]
struct BenchmarkResult {
    iterations: u32,
    batch_size: usize,
    total_time: Duration,
    avg_latency: Duration,
    throughput: f64, // texts per second
}

impl BenchmarkResult {
    fn new(iterations: u32, batch_size: usize, total_time: Duration) -> Self {
        let total_texts = iterations as usize * batch_size;
        let avg_latency = total_time / iterations;
        let throughput = total_texts as f64 / total_time.as_secs_f64();
        Self {
            iterations,
            batch_size,
            total_time,
            avg_latency,
            throughput,
        }
    }
}

/// Benchmark a provider's encode function
async fn benchmark_encode<F, Fut>(
    name: &str,
    iterations: u32,
    texts: Vec<String>,
    encode_fn: F,
) -> BenchmarkResult
where
    F: Fn(Vec<String>) -> Fut,
    Fut: std::future::Future<Output = Result<Vec<Vec<f32>>, codeprysm_search::SearchError>>,
{
    let batch_size = texts.len();

    // Warm up
    let _ = encode_fn(texts.clone()).await;

    // Benchmark
    let start = Instant::now();
    for _ in 0..iterations {
        let _ = encode_fn(texts.clone()).await.expect("Encode failed");
    }
    let total_time = start.elapsed();

    let result = BenchmarkResult::new(iterations, batch_size, total_time);

    println!(
        "  {}: {:?} total, {:?}/iter, {:.1} texts/sec",
        name, result.total_time, result.avg_latency, result.throughput
    );

    result
}

/// Run benchmarks for a provider
async fn benchmark_provider(
    provider: &dyn EmbeddingProvider,
    iterations: u32,
) -> Option<(BenchmarkResult, BenchmarkResult)> {
    let provider_name = format!("{:?}", provider.provider_type());
    println!("\n=== {} Provider ===", provider_name);

    // Check provider status
    match provider.check_status().await {
        Ok(status) => {
            if status.available {
                println!("Status: Available ({})", status.device);
                if let Some(latency) = status.latency_ms {
                    println!("Latency: {}ms", latency);
                }
            } else {
                println!("Status: Unavailable");
                if let Some(err) = &status.error {
                    println!("Error: {}", err);
                }
                return None;
            }
        }
        Err(e) => {
            println!("Status check failed: {}", e);
            return None;
        }
    }

    println!("Dimension: {}", provider.embedding_dim());

    let texts: Vec<String> = TEST_TEXTS.iter().map(|s| s.to_string()).collect();

    // Warm up and measure model loading
    println!("\nWarm-up (includes model loading):");
    let start = Instant::now();
    let _ = provider
        .encode_semantic(vec![texts[0].clone()])
        .await
        .expect("Semantic warm-up failed");
    println!("  Semantic model load: {:?}", start.elapsed());

    let start = Instant::now();
    let _ = provider
        .encode_code(vec![texts[0].clone()])
        .await
        .expect("Code warm-up failed");
    println!("  Code model load: {:?}", start.elapsed());

    // Run benchmarks
    println!(
        "\nBenchmarks ({} iterations, {} texts/batch):",
        iterations,
        texts.len()
    );

    let provider_clone = provider;
    let texts_clone = texts.clone();
    let semantic_result = benchmark_encode("Semantic", iterations, texts_clone, |t| async move {
        provider_clone.encode_semantic(t).await
    })
    .await;

    let texts_clone = texts.clone();
    let code_result = benchmark_encode("Code", iterations, texts_clone, |t| async move {
        provider_clone.encode_code(t).await
    })
    .await;

    Some((semantic_result, code_result))
}

/// Benchmark different batch sizes
async fn benchmark_batch_sizes(provider: &dyn EmbeddingProvider) {
    println!("\n=== Batch Size Comparison ===");

    let batch_sizes = [1, 5, 10, 20];

    for &size in &batch_sizes {
        // Generate test texts of requested size
        let texts: Vec<String> = (0..size)
            .map(|i| TEST_TEXTS[i % TEST_TEXTS.len()].to_string())
            .collect();

        let iterations = match size {
            1 => 20,
            5 => 10,
            10 => 5,
            _ => 3,
        };

        let start = Instant::now();
        for _ in 0..iterations {
            let _ = provider
                .encode_semantic(texts.clone())
                .await
                .expect("Encode failed");
        }
        let total_time = start.elapsed();
        let result = BenchmarkResult::new(iterations, size, total_time);

        println!(
            "  Batch {}: {:?}/iter, {:.1} texts/sec",
            size, result.avg_latency, result.throughput
        );
    }
}

#[tokio::main]
async fn main() {
    println!("╔════════════════════════════════════════╗");
    println!("║   Prism Embedding Provider Benchmark   ║");
    println!("╚════════════════════════════════════════╝");

    #[cfg(feature = "metal")]
    println!("GPU: Metal enabled");
    #[cfg(feature = "cuda")]
    println!("GPU: CUDA enabled");
    #[cfg(not(any(feature = "metal", feature = "cuda")))]
    println!("GPU: None (CPU only)");

    #[cfg(feature = "rate-limit")]
    println!("Rate limiting: Enabled");
    #[cfg(not(feature = "rate-limit"))]
    println!("Rate limiting: Disabled");

    // Local provider (always available)
    let local_config = EmbeddingConfig::local();
    match create_provider(&local_config) {
        Ok(provider) => {
            let _ = benchmark_provider(provider.as_ref(), 10).await;
            benchmark_batch_sizes(provider.as_ref()).await;
        }
        Err(e) => println!("\nLocal provider error: {}", e),
    }

    // Azure ML provider (if configured)
    let azure_config = EmbeddingConfig::azure_ml();
    match create_provider(&azure_config) {
        Ok(provider) => {
            // Use fewer iterations for remote providers
            let _ = benchmark_provider(provider.as_ref(), 5).await;
        }
        Err(e) => println!("\nAzure ML provider: Not configured ({})", e),
    }

    // OpenAI provider (if configured)
    let openai_config = EmbeddingConfig::openai();
    match create_provider(&openai_config) {
        Ok(provider) => {
            // Skip if no API key configured (default config has no key)
            if provider.provider_type() == EmbeddingProviderType::Openai {
                // Only benchmark if we can actually reach the API
                match provider.check_status().await {
                    Ok(status) if status.available => {
                        let _ = benchmark_provider(provider.as_ref(), 5).await;
                    }
                    _ => println!("\nOpenAI provider: Not configured or unreachable"),
                }
            }
        }
        Err(e) => println!("\nOpenAI provider: Not configured ({})", e),
    }

    println!("\n╔════════════════════════════════════════╗");
    println!("║           Benchmark Complete           ║");
    println!("╚════════════════════════════════════════╝");
}
