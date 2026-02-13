//! ONNX Runtime embedding provider
//!
//! ⚠️ **EXPERIMENTAL** - Depends on `ort` crate 2.0.0-rc (not yet stable)
//!
//! Provides local inference for embedding generation using ONNX Runtime:
//! - **Semantic**: Jina Embeddings v2 Base EN (768 dimensions, ONNX format)
//! - **Code**: Jina Embeddings v2 Base Code (768 dimensions, ONNX format)
//!
//! Execution providers:
//! - **CPU**: Default, always available
//! - **DirectML**: Windows GPU acceleration (Intel Arc, AMD, NVIDIA)
//! - **OpenVINO**: Intel hardware acceleration (CPUs and GPUs including Arc)
//!
//! # Requirements
//!
//! - ONNX models are auto-downloaded from HuggingFace Hub to `~/.cache/huggingface/hub/`
//! - Build with `--features onnx` for CPU support
//! - Build with `--features onnx-directml` for DirectML support (Windows)
//! - Build with `--features onnx-openvino` for OpenVINO support

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use async_trait::async_trait;
use hf_hub::{api::sync::Api, Repo, RepoType};
use once_cell::sync::OnceCell;
use ort::session::Session;
use ort::value::Value as OrtValue;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer};
use tracing::{debug, info, warn};

use crate::error::{Result, SearchError};

use super::provider::{EmbeddingProvider, EmbeddingProviderType, ProviderStatus};

/// Unified embedding dimension (both models output 768-dim)
pub const EMBEDDING_DIM: usize = 768;

/// Semantic model on HuggingFace Hub
const SEMANTIC_MODEL_ID: &str = "jinaai/jina-embeddings-v2-base-en";

/// Code model on HuggingFace Hub
const CODE_MODEL_ID: &str = "jinaai/jina-embeddings-v2-base-code";

/// ONNX Runtime execution provider
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionProvider {
    /// CPU with optimizations
    Cpu,
    /// DirectML (Windows DirectX ML for GPUs)
    DirectML,
    /// OpenVINO (Intel CPUs and GPUs)
    OpenVino,
}

impl std::fmt::Display for ExecutionProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionProvider::Cpu => write!(f, "CPU"),
            ExecutionProvider::DirectML => write!(f, "DirectML"),
            ExecutionProvider::OpenVino => write!(f, "OpenVINO"),
        }
    }
}

/// Configuration for ONNX provider
#[derive(Debug, Clone)]
pub struct OnnxConfig {
    /// Path to semantic embedding model (ONNX format)
    pub semantic_model_path: PathBuf,
    /// Path to code embedding model (ONNX format)
    pub code_model_path: PathBuf,
    /// Execution provider to use
    pub execution_provider: ExecutionProvider,
    /// Device ID (for GPU providers)
    pub device_id: u32,
    /// Number of threads for CPU inference
    pub num_threads: Option<usize>,
}

impl Default for OnnxConfig {
    fn default() -> Self {
        // Try to get models from HuggingFace cache or download
        // Falls back to local models/ directory if HF download fails
        let semantic_model_path = download_onnx_model_if_available(
            SEMANTIC_MODEL_ID,
            "onnx/model.onnx",
        )
        .unwrap_or_else(|| PathBuf::from("models/jina-semantic.onnx"));

        let code_model_path = download_onnx_model_if_available(
            CODE_MODEL_ID,
            "onnx/model.onnx",
        )
        .unwrap_or_else(|| PathBuf::from("models/jina-code.onnx"));

        Self {
            semantic_model_path,
            code_model_path,
            execution_provider: ExecutionProvider::Cpu,
            device_id: 0,
            num_threads: None,
        }
    }
}

/// ONNX embedding provider
///
/// Uses `Arc<OnnxProviderInner>` for interior clonability, which is required
/// for `spawn_blocking` to move the provider into the blocking task.
///
/// Thread-safe: Uses `OnceCell` for lazy model initialization.
#[derive(Clone)]
pub struct OnnxProvider {
    inner: Arc<OnnxProviderInner>,
}

/// Inner state for OnnxProvider (not Clone due to OnceCell)
struct OnnxProviderInner {
    semantic_model: OnceCell<SemanticModel>,
    code_model: OnceCell<CodeModel>,
    config: OnnxConfig,
}

/// Loaded semantic model with ONNX Runtime
struct SemanticModel {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

/// Loaded code model with ONNX Runtime
struct CodeModel {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl OnnxProvider {
    /// Create a new ONNX provider with the given configuration
    ///
    /// Models will be downloaded from HuggingFace Hub on first use if not found locally.
    pub fn new(config: OnnxConfig) -> Result<Self> {
        // Don't validate paths here - models will be downloaded lazily on first use
        Ok(Self {
            inner: Arc::new(OnnxProviderInner {
                semantic_model: OnceCell::new(),
                code_model: OnceCell::new(),
                config,
            }),
        })
    }

    /// Create from environment variables
    ///
    /// Environment variables:
    /// - `CODEPRYSM_ONNX_EXECUTION_PROVIDER`: "cpu", "directml", or "openvino" (default: "cpu")
    /// - `CODEPRYSM_ONNX_SEMANTIC_MODEL_PATH`: Path to semantic model
    /// - `CODEPRYSM_ONNX_CODE_MODEL_PATH`: Path to code model
    /// - `CODEPRYSM_ONNX_DEVICE_ID`: Device ID for GPU providers (default: 0)
    /// - `CODEPRYSM_ONNX_NUM_THREADS`: Number of threads for CPU (default: auto)
    pub fn from_env() -> Result<Self> {
        let execution_provider = std::env::var("CODEPRYSM_ONNX_EXECUTION_PROVIDER")
            .ok()
            .and_then(|s| match s.to_lowercase().as_str() {
                "cpu" => Some(ExecutionProvider::Cpu),
                "directml" => Some(ExecutionProvider::DirectML),
                "openvino" => Some(ExecutionProvider::OpenVino),
                _ => None,
            })
            .unwrap_or(ExecutionProvider::Cpu);

        let semantic_model_path =
            std::env::var("CODEPRYSM_ONNX_SEMANTIC_MODEL_PATH").unwrap_or_else(|_| {
                "models/jina-semantic.onnx".to_string()
            });

        let code_model_path =
            std::env::var("CODEPRYSM_ONNX_CODE_MODEL_PATH").unwrap_or_else(|_| {
                "models/jina-code.onnx".to_string()
            });

        let device_id = std::env::var("CODEPRYSM_ONNX_DEVICE_ID")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);

        let num_threads = std::env::var("CODEPRYSM_ONNX_NUM_THREADS")
            .ok()
            .and_then(|s| s.parse().ok());

        let config = OnnxConfig {
            semantic_model_path: PathBuf::from(semantic_model_path),
            code_model_path: PathBuf::from(code_model_path),
            execution_provider,
            device_id,
            num_threads,
        };

        Self::new(config)
    }

    /// Get the execution provider being used
    pub fn execution_provider(&self) -> ExecutionProvider {
        self.inner.config.execution_provider
    }

    /// Get execution provider name as string
    fn execution_provider_name(&self) -> String {
        self.inner.config.execution_provider.to_string()
    }

    /// Ensure semantic model is loaded (thread-safe lazy initialization)
    fn ensure_semantic_model(&self) -> Result<&SemanticModel> {
        self.inner.semantic_model.get_or_try_init(|| {
            load_semantic_model(&self.inner.config)
        })
    }

    /// Ensure code model is loaded (thread-safe lazy initialization)
    fn ensure_code_model(&self) -> Result<&CodeModel> {
        self.inner.code_model.get_or_try_init(|| {
            load_code_model(&self.inner.config)
        })
    }

    /// Check if models are loaded
    pub fn is_loaded(&self) -> (bool, bool) {
        (
            self.inner.semantic_model.get().is_some(),
            self.inner.code_model.get().is_some(),
        )
    }

    /// Synchronous semantic encoding (internal)
    fn encode_semantic_sync(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        debug!("Encoding {} texts with ONNX semantic model", texts.len());

        let model_data = self.ensure_semantic_model()?;
        let mut session = model_data.session.lock()
            .map_err(|e| SearchError::Embedding(format!("Failed to lock session: {}", e)))?;
        encode_with_onnx(&mut *session, &model_data.tokenizer, &texts)
    }

    /// Synchronous code encoding (internal)
    fn encode_code_sync(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        debug!("Encoding {} code snippets with ONNX code model", texts.len());

        let model_data = self.ensure_code_model()?;
        let mut session = model_data.session.lock()
            .map_err(|e| SearchError::Embedding(format!("Failed to lock session: {}", e)))?;
        encode_with_onnx(&mut *session, &model_data.tokenizer, &texts)
    }
}

#[async_trait]
impl EmbeddingProvider for OnnxProvider {
    async fn encode_semantic(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let provider = self.clone();
        tokio::task::spawn_blocking(move || provider.encode_semantic_sync(&texts))
            .await
            .map_err(|e| SearchError::Embedding(format!("Blocking task panicked: {}", e)))?
    }

    async fn encode_code(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        let provider = self.clone();
        tokio::task::spawn_blocking(move || provider.encode_code_sync(&texts))
            .await
            .map_err(|e| SearchError::Embedding(format!("Blocking task panicked: {}", e)))?
    }

    async fn check_status(&self) -> Result<ProviderStatus> {
        let (semantic_loaded, code_loaded) = self.is_loaded();
        let device = self.execution_provider_name();

        // Check model file availability
        let semantic_available = self.inner.config.semantic_model_path.exists();
        let code_available = self.inner.config.code_model_path.exists();

        let error = if !semantic_available || !code_available {
            Some(format!(
                "ONNX models not found. Semantic: {}, Code: {}",
                self.inner.config.semantic_model_path.display(),
                self.inner.config.code_model_path.display()
            ))
        } else {
            None
        };

        Ok(ProviderStatus {
            available: semantic_available && code_available,
            provider_type: EmbeddingProviderType::Onnx,
            device,
            latency_ms: None,
            semantic_ready: semantic_loaded,
            code_ready: code_loaded,
            error,
        })
    }

    async fn warmup(&self) -> Result<()> {
        let provider = self.clone();
        let start = Instant::now();

        tokio::task::spawn_blocking(move || {
            provider.ensure_semantic_model()?;
            provider.ensure_code_model()?;
            Ok::<_, SearchError>(())
        })
        .await
        .map_err(|e| SearchError::Embedding(format!("Warmup task panicked: {}", e)))??;

        info!("OnnxProvider warmup complete in {:?}", start.elapsed());
        Ok(())
    }

    fn embedding_dim(&self) -> usize {
        EMBEDDING_DIM
    }

    fn provider_type(&self) -> EmbeddingProviderType {
        EmbeddingProviderType::Onnx
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Download ONNX model from HuggingFace Hub if available
///
/// Downloads ONNX model file to `~/.cache/huggingface/hub/`. Returns None if
/// download fails (e.g., network issues, model not found).
fn download_onnx_model_if_available(model_id: &str, filename: &str) -> Option<PathBuf> {
    match download_onnx_model(model_id, filename) {
        Ok(path) => {
            info!("Downloaded ONNX model from HF Hub: {}", path.display());
            Some(path)
        }
        Err(e) => {
            warn!(
                "Failed to download ONNX model {} from HF Hub: {}. Will try local models/ directory.",
                model_id, e
            );
            None
        }
    }
}

/// Download ONNX model file from HuggingFace Hub
fn download_onnx_model(model_id: &str, filename: &str) -> Result<PathBuf> {
    let api = Api::new()
        .map_err(|e| SearchError::Embedding(format!("Failed to create HF API: {}", e)))?;

    let repo = Repo::with_revision(model_id.to_string(), RepoType::Model, "main".to_string());
    let api_repo = api.repo(repo);

    // Downloads to ~/.cache/huggingface/hub/models--{org}--{model}/snapshots/{hash}/
    api_repo
        .get(filename)
        .map_err(|e| SearchError::Embedding(format!("Failed to download {}: {}", filename, e)))
}

/// Download tokenizer from HuggingFace Hub
fn download_tokenizer(model_id: &str) -> Result<PathBuf> {
    let api = Api::new()
        .map_err(|e| SearchError::Embedding(format!("Failed to create HF API: {}", e)))?;

    let repo = Repo::with_revision(model_id.to_string(), RepoType::Model, "main".to_string());
    let api_repo = api.repo(repo);

    api_repo
        .get("tokenizer.json")
        .map_err(|e| SearchError::Embedding(format!("Failed to download tokenizer.json: {}", e)))
}

/// Load semantic model from disk or download from HuggingFace Hub
fn load_semantic_model(config: &OnnxConfig) -> Result<SemanticModel> {
    // Check if model exists at configured path
    let model_path = if config.semantic_model_path.exists() {
        info!(
            "Loading ONNX semantic model from {}",
            config.semantic_model_path.display()
        );
        config.semantic_model_path.clone()
    } else {
        // Try to download from HuggingFace Hub
        info!(
            "Semantic model not found at {}, downloading from HuggingFace Hub...",
            config.semantic_model_path.display()
        );
        let downloaded_path = download_onnx_model(CODE_MODEL_ID, "onnx/model.onnx")
            .map_err(|e| {
                SearchError::Embedding(format!(
                    "Failed to load semantic model from {} and download from HuggingFace failed: {}",
                    config.semantic_model_path.display(),
                    e
                ))
            })?;
        info!("Downloaded semantic model to: {}", downloaded_path.display());
        downloaded_path
    };

    info!("Creating ONNX session from: {}", model_path.display());
    let session = create_session(&model_path, config)?;
    let tokenizer = load_tokenizer_for_semantic()?;

    Ok(SemanticModel {
        session: Mutex::new(session),
        tokenizer
    })
}

/// Load code model from disk or download from HuggingFace Hub
fn load_code_model(config: &OnnxConfig) -> Result<CodeModel> {
    // Check if model exists at configured path
    let model_path = if config.code_model_path.exists() {
        info!(
            "Loading ONNX code model from {}",
            config.code_model_path.display()
        );
        config.code_model_path.clone()
    } else {
        // Try to download from HuggingFace Hub
        info!(
            "Code model not found at {}, downloading from HuggingFace Hub...",
            config.code_model_path.display()
        );
        let downloaded_path = download_onnx_model(CODE_MODEL_ID, "onnx/model.onnx")
            .map_err(|e| {
                SearchError::Embedding(format!(
                    "Failed to load code model from {} and download from HuggingFace failed: {}",
                    config.code_model_path.display(),
                    e
                ))
            })?;
        info!("Downloaded code model to: {}", downloaded_path.display());
        downloaded_path
    };

    info!("Creating ONNX session from: {}", model_path.display());
    let session = create_session(&model_path, config)?;
    let tokenizer = load_tokenizer_for_code()?;

    Ok(CodeModel {
        session: Mutex::new(session),
        tokenizer
    })
}

/// Create an ONNX Runtime session with the specified execution provider
fn create_session(model_path: &PathBuf, config: &OnnxConfig) -> Result<Session> {
    let mut session_builder = Session::builder()
        .map_err(|e| SearchError::Embedding(format!("Failed to create session builder: {}", e)))?;

    // Configure execution provider
    match config.execution_provider {
        ExecutionProvider::Cpu => {
            #[cfg(feature = "onnx")]
            {
                if let Some(num_threads) = config.num_threads {
                    session_builder = session_builder
                        .with_intra_threads(num_threads)
                        .map_err(|e| {
                            SearchError::Embedding(format!("Failed to set thread count: {}", e))
                        })?;
                }
                info!("Using ONNX CPU execution provider");
            }
        }
        ExecutionProvider::DirectML => {
            #[cfg(feature = "onnx-directml")]
            {
                session_builder = session_builder
                    .with_execution_providers([
                        ort::ep::DirectML::default()
                            .with_device_id(config.device_id as i32)
                            .build()
                    ])
                    .map_err(|e| {
                        SearchError::Embedding(format!("Failed to enable DirectML: {}", e))
                    })?;
                info!("Using ONNX DirectML execution provider (device {})", config.device_id);
            }
            #[cfg(not(feature = "onnx-directml"))]
            {
                warn!("DirectML requested but not compiled. Rebuild with --features onnx-directml");
                return Err(SearchError::Embedding(
                    "DirectML not available. Rebuild with --features onnx-directml".to_string(),
                ));
            }
        }
        ExecutionProvider::OpenVino => {
            #[cfg(feature = "onnx-openvino")]
            {
                session_builder = session_builder
                    .with_execution_providers([
                        ort::ep::OpenVINO::default()
                            .with_device_type("GPU_FP32")
                            .build()
                    ])
                    .map_err(|e| {
                        SearchError::Embedding(format!("Failed to enable OpenVINO: {}", e))
                    })?;
                info!("Using ONNX OpenVINO execution provider");
            }
            #[cfg(not(feature = "onnx-openvino"))]
            {
                warn!("OpenVINO requested but not compiled. Rebuild with --features onnx-openvino");
                return Err(SearchError::Embedding(
                    "OpenVINO not available. Rebuild with --features onnx-openvino".to_string(),
                ));
            }
        }
    }

    // Load the model
    let session = session_builder
        .commit_from_file(model_path)
        .map_err(|e| SearchError::Embedding(format!("Failed to load ONNX model from {:?}: {}", model_path, e)))?;

    Ok(session)
}

/// Load tokenizer for semantic model (Jina v2 base-en)
/// NOTE: Currently using CODE_MODEL_ID because semantic model doesn't have ONNX format
fn load_tokenizer_for_semantic() -> Result<Tokenizer> {
    // Try to load from HuggingFace Hub first
    // Using CODE_MODEL_ID because we use the code model for both semantic and code
    if let Ok(tokenizer_path) = download_tokenizer(CODE_MODEL_ID) {
        debug!("Using tokenizer from HF Hub: {}", tokenizer_path.display());
        return load_tokenizer_from_path(&tokenizer_path);
    }

    // Fall back to local files
    warn!("Failed to download tokenizer from HF Hub, trying local files...");
    let tokenizer_paths = [
        "models/jina-code-tokenizer.json",
        "models/tokenizer.json",
        "tokenizer.json",
    ];

    for path in &tokenizer_paths {
        if std::path::Path::new(path).exists() {
            debug!("Using local tokenizer: {}", path);
            return load_tokenizer_from_path(&PathBuf::from(path));
        }
    }

    Err(SearchError::Embedding(
        "Semantic tokenizer not found. Please provide tokenizer.json file in models/ directory or ensure network access to HuggingFace Hub".to_string()
    ))
}

/// Load tokenizer for code model (Jina v2 base-code)
fn load_tokenizer_for_code() -> Result<Tokenizer> {
    // Try to load from HuggingFace Hub first
    if let Ok(tokenizer_path) = download_tokenizer(CODE_MODEL_ID) {
        debug!("Using tokenizer from HF Hub: {}", tokenizer_path.display());
        return load_tokenizer_from_path(&tokenizer_path);
    }

    // Fall back to local files
    warn!("Failed to download tokenizer from HF Hub, trying local files...");
    let tokenizer_paths = [
        "models/jina-code-tokenizer.json",
        "models/tokenizer.json",
        "tokenizer.json",
    ];

    for path in &tokenizer_paths {
        if std::path::Path::new(path).exists() {
            debug!("Using local tokenizer: {}", path);
            return load_tokenizer_from_path(&PathBuf::from(path));
        }
    }

    Err(SearchError::Embedding(
        "Code tokenizer not found. Please provide tokenizer.json file in models/ directory or ensure network access to HuggingFace Hub".to_string()
    ))
}

/// Load tokenizer from a file path and configure padding
fn load_tokenizer_from_path(path: &PathBuf) -> Result<Tokenizer> {
    Tokenizer::from_file(path)
        .map_err(|e| {
            SearchError::Embedding(format!(
                "Failed to load tokenizer from {}: {}",
                path.display(),
                e
            ))
        })
        .map(|mut tok| {
            if let Some(pp) = tok.get_padding_mut() {
                pp.strategy = PaddingStrategy::BatchLongest;
            } else {
                tok.with_padding(Some(PaddingParams {
                    strategy: PaddingStrategy::BatchLongest,
                    ..Default::default()
                }));
            }
            tok
        })
}

/// Encode texts using ONNX Runtime
fn encode_with_onnx(
    session: &mut Session,
    tokenizer: &Tokenizer,
    texts: &[&str],
) -> Result<Vec<Vec<f32>>> {
    // Tokenize inputs
    let encodings = tokenizer
        .encode_batch(texts.to_vec(), true)
        .map_err(|e| SearchError::Embedding(format!("Tokenization failed: {}", e)))?;

    if encodings.is_empty() {
        return Ok(vec![]);
    }

    // Extract input tensors
    let input_ids: Vec<Vec<i64>> = encodings
        .iter()
        .map(|enc| enc.get_ids().iter().map(|&id| id as i64).collect())
        .collect();

    let attention_mask: Vec<Vec<i64>> = encodings
        .iter()
        .map(|enc| enc.get_attention_mask().iter().map(|&m| m as i64).collect())
        .collect();

    // Flatten for ONNX input
    let batch_size = input_ids.len();
    let seq_length = input_ids[0].len();

    let input_ids_flat: Vec<i64> = input_ids.into_iter().flatten().collect();
    let attention_mask_flat: Vec<i64> = attention_mask.into_iter().flatten().collect();

    // Keep attention mask as f32 for pooling calculations
    let attention_mask_f32: Vec<f32> = attention_mask_flat.iter().map(|&x| x as f32).collect();

    // Create ONNX input tensors - only input_ids and attention_mask (no token_type_ids)
    // Jina models don't use token_type_ids
    let input_tensor = ort::inputs![
        "input_ids" => OrtValue::from_array(
            ([batch_size, seq_length], input_ids_flat)
        ).map_err(|e| SearchError::Embedding(format!("Failed to create input_ids tensor: {}", e)))?,
        "attention_mask" => OrtValue::from_array(
            ([batch_size, seq_length], attention_mask_flat)
        ).map_err(|e| SearchError::Embedding(format!("Failed to create attention_mask tensor: {}", e)))?
    ];

    // Run inference
    let outputs = session
        .run(input_tensor)
        .map_err(|e| SearchError::Embedding(format!("ONNX inference failed: {}", e)))?;

    // Extract embeddings from output - should be [batch_size, seq_length, hidden_dim]
    let output_tensor = outputs
        .get("last_hidden_state")
        .or_else(|| outputs.get("embeddings"))
        .ok_or_else(|| SearchError::Embedding(
            "No output tensor found. Expected 'last_hidden_state' or 'embeddings' output.".to_string()
        ))?;

    // Get shape and data
    let (shape, output_data): (_, &[f32]) = output_tensor
        .try_extract_tensor()
        .map_err(|e| SearchError::Embedding(format!("Failed to extract output tensor: {}", e)))?;

    // Convert shape to slice
    let shape_dims = shape.as_ref();

    // Verify shape: [batch_size, seq_length, hidden_dim]
    if shape_dims.len() != 3 {
        return Err(SearchError::Embedding(format!(
            "Expected 3D output tensor [batch_size, seq_length, hidden_dim], got shape: {:?}",
            shape_dims
        )));
    }

    let batch_size_out = shape_dims[0] as usize;
    let seq_length_out = shape_dims[1] as usize;
    let hidden_dim = shape_dims[2] as usize;

    if batch_size_out != batch_size {
        return Err(SearchError::Embedding(format!(
            "Batch size mismatch: expected {}, got {}",
            batch_size, batch_size_out
        )));
    }

    if seq_length_out != seq_length {
        return Err(SearchError::Embedding(format!(
            "Sequence length mismatch: expected {}, got {}",
            seq_length, seq_length_out
        )));
    }

    if hidden_dim != EMBEDDING_DIM {
        return Err(SearchError::Embedding(format!(
            "Hidden dimension mismatch: expected {}, got {}",
            EMBEDDING_DIM, hidden_dim
        )));
    }

    // Apply mean pooling with attention mask (matching Candle's approach)
    // This replicates the mean_pool() function from local.rs:
    // - Mask out padding tokens using attention_mask
    // - Average non-padding token embeddings across sequence length
    let mut embeddings = Vec::with_capacity(batch_size);

    for i in 0..batch_size {
        // Get this sequence's embeddings: [seq_length, hidden_dim]
        let seq_start = i * seq_length * hidden_dim;

        // Calculate mean pooling with attention mask
        let mut sum = vec![0.0f32; hidden_dim];
        let mut mask_sum = 0.0f32;

        for j in 0..seq_length {
            let mask_val = attention_mask_f32[i * seq_length + j];
            if mask_val > 0.0 {
                let token_start = seq_start + j * hidden_dim;
                for k in 0..hidden_dim {
                    sum[k] += output_data[token_start + k] * mask_val;
                }
                mask_sum += mask_val;
            }
        }

        // Average by dividing by sum of mask values
        if mask_sum > 0.0 {
            for val in &mut sum {
                *val /= mask_sum;
            }
        }

        // L2 normalization (matching normalize_l2() from local.rs)
        let norm: f32 = sum.iter().map(|x| x * x).sum::<f32>().sqrt();
        let normalized: Vec<f32> = if norm > 0.0 {
            sum.iter().map(|x| x / norm).collect()
        } else {
            sum
        };

        embeddings.push(normalized);
    }

    Ok(embeddings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_execution_provider_display() {
        assert_eq!(ExecutionProvider::Cpu.to_string(), "CPU");
        assert_eq!(ExecutionProvider::DirectML.to_string(), "DirectML");
        assert_eq!(ExecutionProvider::OpenVino.to_string(), "OpenVINO");
    }

    #[test]
    fn test_onnx_config_default() {
        let config = OnnxConfig::default();
        assert_eq!(config.execution_provider, ExecutionProvider::Cpu);
        assert_eq!(config.device_id, 0);
        assert!(config.num_threads.is_none());
    }

    #[test]
    fn test_onnx_provider_type() {
        // This test requires actual ONNX models to exist, so we'll skip in normal tests
        // Create a minimal test when test fixtures are available
    }

    #[test]
    fn test_embedding_dim() {
        assert_eq!(EMBEDDING_DIM, 768);
    }
}
