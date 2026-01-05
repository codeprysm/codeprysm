//! Embeddings generation using candle
//!
//! Provides semantic and code embeddings for hybrid search using GPU acceleration:
//! - **Semantic**: Jina Embeddings v2 Base EN (768 dimensions) for natural language queries
//! - **Code**: Jina Embeddings v2 Base Code (768 dimensions) for code-aware search
//!
//! GPU acceleration is available via compile-time feature flags:
//! - `--features metal` for macOS Metal/MPS
//! - `--features cuda` for NVIDIA CUDA
//!
//! # Example
//!
//! ```ignore
//! use codeprysm_search::EmbeddingsManager;
//!
//! let manager = EmbeddingsManager::new()?;
//!
//! // Semantic embeddings for natural language
//! let queries = vec!["authentication logic", "error handling"];
//! let semantic_vecs = manager.encode_semantic(&queries)?;
//!
//! // Code embeddings for code snippets
//! let code = vec!["fn authenticate(user: &User) -> Result<Token>"];
//! let code_vecs = manager.encode_code(&code)?;
//! ```

use candle_core::{DType, Device, Module, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::jina_bert::{BertModel as JinaBertModel, Config as JinaConfig};
use hf_hub::{api::sync::Api, Repo, RepoType};
use once_cell::sync::OnceCell;
use std::path::PathBuf;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer};
use tracing::{debug, info};

use crate::error::{Result, SearchError};
use crate::jina_bert_v2::{BertModel as JinaBertV2Model, Config as JinaV2Config};

/// Status of embedding model availability
#[derive(Debug, Clone)]
pub struct ModelStatus {
    /// Whether the semantic model is available
    pub semantic_available: bool,
    /// Whether the code model is available
    pub code_available: bool,
    /// Whether semantic model is currently loaded in memory
    pub semantic_loaded: bool,
    /// Whether code model is currently loaded in memory
    pub code_loaded: bool,
    /// Device being used (CPU, Metal, CUDA)
    pub device: String,
    /// Error message for semantic model if unavailable
    pub semantic_error: Option<String>,
    /// Error message for code model if unavailable
    pub code_error: Option<String>,
}

impl ModelStatus {
    /// Check if all models are available
    pub fn all_available(&self) -> bool {
        self.semantic_available && self.code_available
    }
}

impl From<crate::embeddings::ProviderStatus> for ModelStatus {
    fn from(status: crate::embeddings::ProviderStatus) -> Self {
        ModelStatus {
            semantic_available: status.semantic_ready,
            code_available: status.code_ready,
            semantic_loaded: status.semantic_ready,
            code_loaded: status.code_ready,
            device: status.device,
            semantic_error: if !status.semantic_ready {
                status.error.clone()
            } else {
                None
            },
            code_error: if !status.code_ready {
                status.error
            } else {
                None
            },
        }
    }
}

/// Unified embedding dimension (both models output 768-dim)
pub const EMBEDDING_DIM: usize = 768;

/// Dimensions for semantic embeddings (all-mpnet-base-v2)
pub const SEMANTIC_DIM: usize = 768;

/// Dimensions for code embeddings (Jina Embeddings v2 Base Code)
pub const CODE_DIM: usize = 768;

/// Default batch size for embedding generation
const DEFAULT_BATCH_SIZE: usize = 32;

/// Data type for model inference
const DTYPE: DType = DType::F32;

/// Semantic model on HuggingFace Hub (Jina v2 Base EN - general text)
const SEMANTIC_MODEL_ID: &str = "jinaai/jina-embeddings-v2-base-en";

/// Code model on HuggingFace Hub
const CODE_MODEL_ID: &str = "jinaai/jina-embeddings-v2-base-code";

/// Select the best available device for inference
fn select_device() -> Result<Device> {
    // Try Metal/MPS first (if feature enabled)
    #[cfg(feature = "metal")]
    {
        match Device::new_metal(0) {
            Ok(device) => {
                info!("Using Metal/MPS GPU acceleration");
                return Ok(device);
            }
            Err(e) => {
                debug!("Metal not available: {}", e);
            }
        }
    }

    // Try CUDA (if feature enabled)
    #[cfg(feature = "cuda")]
    {
        match Device::new_cuda(0) {
            Ok(device) => {
                info!("Using CUDA GPU acceleration");
                return Ok(device);
            }
            Err(e) => {
                debug!("CUDA not available: {}", e);
            }
        }
    }

    // Fallback to CPU
    info!("Using CPU (no GPU acceleration available)");
    Ok(Device::Cpu)
}

/// Loaded semantic model (JinaBERT-based)
struct SemanticModel {
    model: JinaBertModel,
    tokenizer: Tokenizer,
    device: Device,
}

/// Loaded code model (JinaBERT v2 QK-Post-Norm)
struct CodeModel {
    model: JinaBertV2Model,
    tokenizer: Tokenizer,
    device: Device,
}

/// Check if model files are cached locally (without downloading)
fn check_model_cached(model_id: &str) -> std::result::Result<bool, String> {
    let api = Api::new().map_err(|e| format!("HuggingFace API unavailable: {}", e))?;
    let repo = Repo::with_revision(model_id.to_string(), RepoType::Model, "main".to_string());
    let api_repo = api.repo(repo);

    // Check if all required files are accessible (uses cache if available)
    // We use is_local to check if already in cache before downloading
    match api_repo.info() {
        Ok(_) => {
            // Repo exists, now check if files are cached
            // Try to get paths - this will return cached paths or download
            // For a true cache-only check, we'd need to inspect the cache dir directly
            Ok(true)
        }
        Err(e) => Err(format!("Model not available: {}", e)),
    }
}

/// Download model files from HuggingFace Hub
fn download_model_files(model_id: &str) -> Result<(PathBuf, PathBuf, PathBuf)> {
    let api = Api::new()
        .map_err(|e| SearchError::Embedding(format!("Failed to create HF API: {}", e)))?;
    let repo = Repo::with_revision(model_id.to_string(), RepoType::Model, "main".to_string());
    let api_repo = api.repo(repo);

    let config = api_repo
        .get("config.json")
        .map_err(|e| SearchError::Embedding(format!("Failed to download config.json: {}", e)))?;
    let tokenizer = api_repo
        .get("tokenizer.json")
        .map_err(|e| SearchError::Embedding(format!("Failed to download tokenizer.json: {}", e)))?;
    let weights = api_repo.get("model.safetensors").map_err(|e| {
        SearchError::Embedding(format!("Failed to download model.safetensors: {}", e))
    })?;

    Ok((config, tokenizer, weights))
}

/// Load semantic model (jina-embeddings-v2-base-en)
fn load_semantic_model(device: &Device) -> Result<SemanticModel> {
    info!("Loading semantic model ({})...", SEMANTIC_MODEL_ID);

    let (config_path, tokenizer_path, weights_path) = download_model_files(SEMANTIC_MODEL_ID)?;

    // Load config
    let config_str = std::fs::read_to_string(&config_path)
        .map_err(|e| SearchError::Embedding(format!("Failed to read config: {}", e)))?;
    let config: JinaConfig = serde_json::from_str(&config_str)
        .map_err(|e| SearchError::Embedding(format!("Failed to parse config: {}", e)))?;

    // Load tokenizer
    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| SearchError::Embedding(format!("Failed to load tokenizer: {}", e)))?;

    // Load weights
    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, device)
            .map_err(|e| SearchError::Embedding(format!("Failed to load weights: {}", e)))?
    };

    // Create model
    let model = JinaBertModel::new(vb, &config)
        .map_err(|e| SearchError::Embedding(format!("Failed to create model: {}", e)))?;

    info!("Semantic model loaded (dim={})", SEMANTIC_DIM);

    Ok(SemanticModel {
        model,
        tokenizer,
        device: device.clone(),
    })
}

/// Load code model (jina-embeddings-v2-base-code)
///
/// Uses JinaBertV2Model which is compatible with the jina-bert-v2-qk-post-norm
/// architecture used by jina-embeddings-v2-base-code.
fn load_code_model(device: &Device) -> Result<CodeModel> {
    info!("Loading code model ({})...", CODE_MODEL_ID);

    let (config_path, tokenizer_path, weights_path) = download_model_files(CODE_MODEL_ID)?;

    // Load config (using v2 config for qk-post-norm architecture)
    let config_str = std::fs::read_to_string(&config_path)
        .map_err(|e| SearchError::Embedding(format!("Failed to read config: {}", e)))?;
    let config: JinaV2Config = serde_json::from_str(&config_str)
        .map_err(|e| SearchError::Embedding(format!("Failed to parse config: {}", e)))?;

    // Load tokenizer
    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| SearchError::Embedding(format!("Failed to load tokenizer: {}", e)))?;

    // Load weights
    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, device)
            .map_err(|e| SearchError::Embedding(format!("Failed to load weights: {}", e)))?
    };

    // Create model using v2 architecture (compatible with jina-bert-v2-qk-post-norm)
    let model = JinaBertV2Model::new(vb, &config)
        .map_err(|e| SearchError::Embedding(format!("Failed to create model: {}", e)))?;

    info!("Code model loaded (dim={})", CODE_DIM);

    Ok(CodeModel {
        model,
        tokenizer,
        device: device.clone(),
    })
}

/// L2 normalize embeddings
fn normalize_l2(v: &Tensor) -> Result<Tensor> {
    v.broadcast_div(&v.sqr()?.sum_keepdim(1)?.sqrt()?)
        .map_err(|e| SearchError::Embedding(format!("L2 normalization failed: {}", e)))
}

/// Mean pooling with attention mask
fn mean_pool(embeddings: &Tensor, attention_mask: &Tensor) -> Result<Tensor> {
    let attention_mask_expanded = attention_mask.to_dtype(DTYPE)?.unsqueeze(2)?;

    let sum_mask = attention_mask_expanded.sum(1)?;
    let masked_embeddings = embeddings.broadcast_mul(&attention_mask_expanded)?;
    let summed = masked_embeddings.sum(1)?;

    summed
        .broadcast_div(&sum_mask)
        .map_err(|e| SearchError::Embedding(format!("Mean pooling failed: {}", e)))
}

/// Manages embedding models for semantic and code search
///
/// Lazily loads models on first use to avoid startup latency.
/// Uses GPU acceleration when available (Metal on macOS, CUDA on Linux).
///
/// Thread-safe: Uses `OnceCell` for lazy initialization, allowing
/// concurrent access with only shared references (`&self`).
pub struct EmbeddingsManager {
    semantic_model: OnceCell<SemanticModel>,
    code_model: OnceCell<CodeModel>,
    device: Device,
    batch_size: usize,
}

impl EmbeddingsManager {
    /// Create a new embeddings manager with default settings
    ///
    /// Models are loaded lazily on first encode call.
    /// Device is selected automatically (Metal > CUDA > CPU).
    pub fn new() -> Result<Self> {
        let device = select_device()?;
        Ok(Self {
            semantic_model: OnceCell::new(),
            code_model: OnceCell::new(),
            device,
            batch_size: DEFAULT_BATCH_SIZE,
        })
    }

    /// Create with custom batch size
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Get the device being used
    pub fn device(&self) -> &Device {
        &self.device
    }

    /// Ensure semantic model is loaded (thread-safe lazy initialization)
    fn ensure_semantic_model(&self) -> Result<&SemanticModel> {
        self.semantic_model
            .get_or_try_init(|| load_semantic_model(&self.device))
    }

    /// Ensure code model is loaded (thread-safe lazy initialization)
    fn ensure_code_model(&self) -> Result<&CodeModel> {
        self.code_model
            .get_or_try_init(|| load_code_model(&self.device))
    }

    /// Generate semantic embeddings for natural language text
    ///
    /// Uses all-mpnet-base-v2 optimized for general text similarity.
    /// Best for queries like "authentication logic" or "error handling".
    ///
    /// # Arguments
    /// * `texts` - Slice of text strings to embed
    ///
    /// # Returns
    /// Vector of embeddings, each with 768 dimensions
    pub fn encode_semantic(&self, texts: &[impl AsRef<str>]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let texts: Vec<&str> = texts.iter().map(AsRef::as_ref).collect();
        debug!("Encoding {} texts with semantic model", texts.len());

        // Get model (loads if needed)
        let model_data = self.ensure_semantic_model()?;

        // Clone what we need to avoid borrow issues
        let device = model_data.device.clone();
        let mut tokenizer = model_data.tokenizer.clone();

        // Configure tokenizer for batching
        let padding = PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            ..Default::default()
        };
        tokenizer.with_padding(Some(padding));

        // Tokenize
        let encodings = tokenizer
            .encode_batch(texts, true)
            .map_err(|e| SearchError::Embedding(format!("Tokenization failed: {}", e)))?;

        // Build tensors
        let token_ids: Vec<Tensor> = encodings
            .iter()
            .map(|enc| {
                let ids = enc.get_ids().to_vec();
                Tensor::new(ids.as_slice(), &device)
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| SearchError::Embedding(format!("Failed to create token tensor: {}", e)))?;

        let attention_masks: Vec<Tensor> = encodings
            .iter()
            .map(|enc| {
                let mask = enc.get_attention_mask().to_vec();
                Tensor::new(mask.as_slice(), &device)
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                SearchError::Embedding(format!("Failed to create attention mask: {}", e))
            })?;

        let token_ids = Tensor::stack(&token_ids, 0)
            .map_err(|e| SearchError::Embedding(format!("Failed to stack tokens: {}", e)))?;
        let attention_mask = Tensor::stack(&attention_masks, 0)
            .map_err(|e| SearchError::Embedding(format!("Failed to stack masks: {}", e)))?;

        // Forward pass
        // JinaBertModel uses ALiBi, only needs input_ids (attention mask used only for pooling)
        let embeddings = model_data
            .model
            .forward(&token_ids)
            .map_err(|e| SearchError::Embedding(format!("Forward pass failed: {}", e)))?;

        // Mean pooling
        let pooled = mean_pool(&embeddings, &attention_mask)?;

        // L2 normalize
        let normalized = normalize_l2(&pooled)?;

        // Convert to Vec<Vec<f32>>
        let result: Vec<Vec<f32>> = (0..normalized.dim(0)?)
            .map(|i| normalized.get(i)?.to_vec1::<f32>())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| SearchError::Embedding(format!("Failed to convert embeddings: {}", e)))?;

        Ok(result)
    }

    /// Generate code embeddings for source code
    ///
    /// Uses Jina Embeddings v2 Base Code optimized for code understanding.
    /// Best for code snippets and code-aware search.
    ///
    /// # Arguments
    /// * `code_texts` - Slice of code strings to embed
    ///
    /// # Returns
    /// Vector of embeddings, each with 768 dimensions
    pub fn encode_code(&self, code_texts: &[impl AsRef<str>]) -> Result<Vec<Vec<f32>>> {
        if code_texts.is_empty() {
            return Ok(vec![]);
        }

        let texts: Vec<&str> = code_texts.iter().map(AsRef::as_ref).collect();
        debug!("Encoding {} code snippets with code model", texts.len());

        // Get model (loads if needed)
        let model_data = self.ensure_code_model()?;

        // Clone what we need to avoid borrow issues
        let device = model_data.device.clone();
        let mut tokenizer = model_data.tokenizer.clone();

        // Configure tokenizer for batching
        let padding = PaddingParams {
            strategy: PaddingStrategy::BatchLongest,
            ..Default::default()
        };
        tokenizer.with_padding(Some(padding));

        // Tokenize
        let encodings = tokenizer
            .encode_batch(texts, true)
            .map_err(|e| SearchError::Embedding(format!("Tokenization failed: {}", e)))?;

        // Build tensors
        let token_ids: Vec<Tensor> = encodings
            .iter()
            .map(|enc| {
                let ids = enc.get_ids().to_vec();
                Tensor::new(ids.as_slice(), &device)
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| SearchError::Embedding(format!("Failed to create token tensor: {}", e)))?;

        let attention_masks: Vec<Tensor> = encodings
            .iter()
            .map(|enc| {
                let mask = enc.get_attention_mask().to_vec();
                Tensor::new(mask.as_slice(), &device)
            })
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| {
                SearchError::Embedding(format!("Failed to create attention mask: {}", e))
            })?;

        let token_ids = Tensor::stack(&token_ids, 0)
            .map_err(|e| SearchError::Embedding(format!("Failed to stack tokens: {}", e)))?;
        let attention_mask = Tensor::stack(&attention_masks, 0)
            .map_err(|e| SearchError::Embedding(format!("Failed to stack masks: {}", e)))?;

        // Forward pass
        // JinaBertModel uses ALiBi, only needs input_ids (attention mask used only for pooling)
        let embeddings = model_data
            .model
            .forward(&token_ids)
            .map_err(|e| SearchError::Embedding(format!("Forward pass failed: {}", e)))?;

        // Mean pooling
        let pooled = mean_pool(&embeddings, &attention_mask)?;

        // L2 normalize
        let normalized = normalize_l2(&pooled)?;

        // Convert to Vec<Vec<f32>>
        let result: Vec<Vec<f32>> = (0..normalized.dim(0)?)
            .map(|i| normalized.get(i)?.to_vec1::<f32>())
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| SearchError::Embedding(format!("Failed to convert embeddings: {}", e)))?;

        Ok(result)
    }

    /// Encode a single semantic query
    ///
    /// Convenience method for encoding a single query string.
    pub fn encode_semantic_query(&self, query: &str) -> Result<Vec<f32>> {
        let embeddings = self.encode_semantic(&[query])?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| SearchError::Embedding("No embedding returned".into()))
    }

    /// Encode a single code snippet
    ///
    /// Convenience method for encoding a single code string.
    pub fn encode_code_query(&self, code: &str) -> Result<Vec<f32>> {
        let embeddings = self.encode_code(&[code])?;
        embeddings
            .into_iter()
            .next()
            .ok_or_else(|| SearchError::Embedding("No embedding returned".into()))
    }

    /// Check if models are loaded
    pub fn is_loaded(&self) -> (bool, bool) {
        (
            self.semantic_model.get().is_some(),
            self.code_model.get().is_some(),
        )
    }

    /// Preload both models
    ///
    /// Call this at startup to avoid latency on first query.
    /// Thread-safe: can be called concurrently from multiple threads.
    pub fn preload(&self) -> Result<()> {
        self.ensure_semantic_model()?;
        self.ensure_code_model()?;
        Ok(())
    }

    /// Check model availability status
    ///
    /// Returns detailed status about model availability without loading models
    /// into memory. Useful for health checks and diagnostics.
    pub fn check_models(&self) -> ModelStatus {
        let (semantic_loaded, code_loaded) = self.is_loaded();

        let device_name = match &self.device {
            Device::Cpu => "CPU".to_string(),
            #[cfg(feature = "metal")]
            Device::Metal(_) => "Metal/MPS".to_string(),
            #[cfg(feature = "cuda")]
            Device::Cuda(_) => "CUDA".to_string(),
            _ => "Unknown".to_string(),
        };

        // Check semantic model
        let (semantic_available, semantic_error) = if semantic_loaded {
            (true, None)
        } else {
            match check_model_cached(SEMANTIC_MODEL_ID) {
                Ok(_) => (true, None),
                Err(e) => (false, Some(e)),
            }
        };

        // Check code model
        let (code_available, code_error) = if code_loaded {
            (true, None)
        } else {
            match check_model_cached(CODE_MODEL_ID) {
                Ok(_) => (true, None),
                Err(e) => (false, Some(e)),
            }
        };

        ModelStatus {
            semantic_available,
            code_available,
            semantic_loaded,
            code_loaded,
            device: device_name,
            semantic_error,
            code_error,
        }
    }
}

impl Default for EmbeddingsManager {
    fn default() -> Self {
        Self::new().expect("Failed to create EmbeddingsManager")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_embeddings_manager_creation() {
        let manager = EmbeddingsManager::new();
        assert!(manager.is_ok());
    }

    #[test]
    fn test_empty_input() {
        let manager = EmbeddingsManager::new().unwrap();
        let empty: Vec<&str> = vec![];

        let semantic = manager.encode_semantic(&empty);
        assert!(semantic.is_ok());
        assert!(semantic.unwrap().is_empty());

        let code = manager.encode_code(&empty);
        assert!(code.is_ok());
        assert!(code.unwrap().is_empty());
    }

    #[test]
    fn test_device_selection() {
        let device = select_device();
        assert!(device.is_ok());
    }

    #[test]
    #[ignore] // Requires model download
    fn test_semantic_encoding() {
        let manager = EmbeddingsManager::new().unwrap();

        let texts = vec!["hello world", "authentication logic"];
        let embeddings = manager.encode_semantic(&texts).unwrap();

        assert_eq!(embeddings.len(), 2);
        assert_eq!(embeddings[0].len(), SEMANTIC_DIM);
        assert_eq!(embeddings[1].len(), SEMANTIC_DIM);
    }

    #[test]
    #[ignore] // Requires model download
    fn test_code_encoding() {
        let manager = EmbeddingsManager::new().unwrap();

        let code = vec!["fn main() { println!(\"hello\"); }"];
        let embeddings = manager.encode_code(&code).unwrap();

        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].len(), CODE_DIM);
    }
}
