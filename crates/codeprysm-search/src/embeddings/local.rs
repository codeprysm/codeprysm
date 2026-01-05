//! Local embedding provider using Candle and Jina models
//!
//! Provides local inference for embedding generation with GPU acceleration:
//! - **Semantic**: Jina Embeddings v2 Base EN (768 dimensions)
//! - **Code**: Jina Embeddings v2 Base Code (768 dimensions)
//!
//! GPU acceleration via compile-time features:
//! - `--features metal` for macOS Metal/MPS
//! - `--features cuda` for NVIDIA CUDA

use std::sync::Arc;

use async_trait::async_trait;
use candle_core::{DType, Device, Module, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::jina_bert::{BertModel as JinaBertModel, Config as JinaConfig};
use hf_hub::{api::sync::Api, Repo, RepoType};
use once_cell::sync::OnceCell;
use std::path::PathBuf;
use std::time::Instant;
use tokenizers::{PaddingParams, PaddingStrategy, Tokenizer};
use tracing::{debug, info};

use super::jina_bert_v2::{BertModel as JinaBertV2Model, Config as JinaV2Config};
use crate::error::{Result, SearchError};

use super::provider::{EmbeddingProvider, EmbeddingProviderType, ProviderStatus};

/// Unified embedding dimension (both models output 768-dim)
pub const EMBEDDING_DIM: usize = 768;

/// Dimensions for semantic embeddings
pub const SEMANTIC_DIM: usize = 768;

/// Dimensions for code embeddings
pub const CODE_DIM: usize = 768;

/// Default batch size for embedding generation
const DEFAULT_BATCH_SIZE: usize = 32;

/// Data type for model inference
const DTYPE: DType = DType::F32;

/// Semantic model on HuggingFace Hub
const SEMANTIC_MODEL_ID: &str = "jinaai/jina-embeddings-v2-base-en";

/// Code model on HuggingFace Hub
const CODE_MODEL_ID: &str = "jinaai/jina-embeddings-v2-base-code";

/// Local embedding provider using Candle for inference
///
/// Uses `Arc<LocalProviderInner>` for interior clonability, which is required
/// for `spawn_blocking` to move the provider into the blocking task.
///
/// Thread-safe: Uses `OnceCell` for lazy model initialization.
#[derive(Clone)]
pub struct LocalProvider {
    inner: Arc<LocalProviderInner>,
}

/// Inner state for LocalProvider (not Clone due to OnceCell)
struct LocalProviderInner {
    semantic_model: OnceCell<SemanticModel>,
    code_model: OnceCell<CodeModel>,
    device: Device,
    #[allow(dead_code)]
    batch_size: usize,
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

impl LocalProvider {
    /// Create a new local provider with default settings
    ///
    /// Device is selected automatically: Metal > CUDA > CPU
    pub fn new() -> Result<Self> {
        let device = select_device()?;
        Ok(Self {
            inner: Arc::new(LocalProviderInner {
                semantic_model: OnceCell::new(),
                code_model: OnceCell::new(),
                device,
                batch_size: DEFAULT_BATCH_SIZE,
            }),
        })
    }

    /// Create with a specific device
    pub fn with_device(device: Device) -> Self {
        Self {
            inner: Arc::new(LocalProviderInner {
                semantic_model: OnceCell::new(),
                code_model: OnceCell::new(),
                device,
                batch_size: DEFAULT_BATCH_SIZE,
            }),
        }
    }

    /// Get the device being used
    pub fn device(&self) -> &Device {
        &self.inner.device
    }

    /// Get device name as string
    fn device_name(&self) -> String {
        match &self.inner.device {
            Device::Cpu => "CPU".to_string(),
            #[cfg(feature = "metal")]
            Device::Metal(_) => "Metal".to_string(),
            #[cfg(feature = "cuda")]
            Device::Cuda(_) => "CUDA".to_string(),
            #[allow(unreachable_patterns)]
            _ => "Unknown".to_string(),
        }
    }

    /// Ensure semantic model is loaded (thread-safe lazy initialization)
    fn ensure_semantic_model(&self) -> Result<&SemanticModel> {
        self.inner
            .semantic_model
            .get_or_try_init(|| load_semantic_model(&self.inner.device))
    }

    /// Ensure code model is loaded (thread-safe lazy initialization)
    fn ensure_code_model(&self) -> Result<&CodeModel> {
        self.inner
            .code_model
            .get_or_try_init(|| load_code_model(&self.inner.device))
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
        debug!("Encoding {} texts with semantic model", texts.len());

        let model_data = self.ensure_semantic_model()?;
        encode_with_model(
            &model_data.model,
            &model_data.tokenizer,
            &model_data.device,
            &texts,
        )
    }

    /// Synchronous code encoding (internal)
    fn encode_code_sync(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let texts: Vec<&str> = texts.iter().map(String::as_str).collect();
        debug!("Encoding {} code snippets with code model", texts.len());

        let model_data = self.ensure_code_model()?;
        encode_with_model_v2(
            &model_data.model,
            &model_data.tokenizer,
            &model_data.device,
            &texts,
        )
    }
}

#[async_trait]
impl EmbeddingProvider for LocalProvider {
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
        let device = self.device_name();

        // Check model availability
        let semantic_available = semantic_loaded || check_model_cached(SEMANTIC_MODEL_ID).is_ok();
        let code_available = code_loaded || check_model_cached(CODE_MODEL_ID).is_ok();

        let error = if !semantic_available || !code_available {
            Some("Models not available - download required".to_string())
        } else {
            None
        };

        Ok(ProviderStatus {
            available: semantic_available && code_available,
            provider_type: EmbeddingProviderType::Local,
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

        info!("LocalProvider warmup complete in {:?}", start.elapsed());
        Ok(())
    }

    fn embedding_dim(&self) -> usize {
        EMBEDDING_DIM
    }

    fn provider_type(&self) -> EmbeddingProviderType {
        EmbeddingProviderType::Local
    }
}

// ============================================================================
// Helper functions (moved from embeddings_legacy.rs)
// ============================================================================

/// Select the best available device for inference
fn select_device() -> Result<Device> {
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

    info!("Using CPU (no GPU acceleration available)");
    Ok(Device::Cpu)
}

/// Check if model files are cached locally
fn check_model_cached(model_id: &str) -> std::result::Result<bool, String> {
    let api = Api::new().map_err(|e| format!("HuggingFace API unavailable: {}", e))?;
    let repo = Repo::with_revision(model_id.to_string(), RepoType::Model, "main".to_string());
    let api_repo = api.repo(repo);

    match api_repo.info() {
        Ok(_) => Ok(true),
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

    let config_str = std::fs::read_to_string(&config_path)
        .map_err(|e| SearchError::Embedding(format!("Failed to read config: {}", e)))?;
    let config: JinaConfig = serde_json::from_str(&config_str)
        .map_err(|e| SearchError::Embedding(format!("Failed to parse config: {}", e)))?;

    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| SearchError::Embedding(format!("Failed to load tokenizer: {}", e)))?;

    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, device)
            .map_err(|e| SearchError::Embedding(format!("Failed to load weights: {}", e)))?
    };

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
fn load_code_model(device: &Device) -> Result<CodeModel> {
    info!("Loading code model ({})...", CODE_MODEL_ID);

    let (config_path, tokenizer_path, weights_path) = download_model_files(CODE_MODEL_ID)?;

    let config_str = std::fs::read_to_string(&config_path)
        .map_err(|e| SearchError::Embedding(format!("Failed to read config: {}", e)))?;
    let config: JinaV2Config = serde_json::from_str(&config_str)
        .map_err(|e| SearchError::Embedding(format!("Failed to parse config: {}", e)))?;

    let tokenizer = Tokenizer::from_file(&tokenizer_path)
        .map_err(|e| SearchError::Embedding(format!("Failed to load tokenizer: {}", e)))?;

    let vb = unsafe {
        VarBuilder::from_mmaped_safetensors(&[weights_path], DTYPE, device)
            .map_err(|e| SearchError::Embedding(format!("Failed to load weights: {}", e)))?
    };

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

/// Encode texts using JinaBertModel (semantic model)
fn encode_with_model(
    model: &JinaBertModel,
    tokenizer: &Tokenizer,
    device: &Device,
    texts: &[&str],
) -> Result<Vec<Vec<f32>>> {
    let mut tokenizer = tokenizer.clone();
    let padding = PaddingParams {
        strategy: PaddingStrategy::BatchLongest,
        ..Default::default()
    };
    tokenizer.with_padding(Some(padding));

    let encodings = tokenizer
        .encode_batch(texts.to_vec(), true)
        .map_err(|e| SearchError::Embedding(format!("Tokenization failed: {}", e)))?;

    let token_ids: Vec<Tensor> = encodings
        .iter()
        .map(|enc| {
            let ids = enc.get_ids().to_vec();
            Tensor::new(ids.as_slice(), device)
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| SearchError::Embedding(format!("Failed to create token tensor: {}", e)))?;

    let attention_masks: Vec<Tensor> = encodings
        .iter()
        .map(|enc| {
            let mask = enc.get_attention_mask().to_vec();
            Tensor::new(mask.as_slice(), device)
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| SearchError::Embedding(format!("Failed to create attention mask: {}", e)))?;

    let token_ids = Tensor::stack(&token_ids, 0)
        .map_err(|e| SearchError::Embedding(format!("Failed to stack tokens: {}", e)))?;
    let attention_mask = Tensor::stack(&attention_masks, 0)
        .map_err(|e| SearchError::Embedding(format!("Failed to stack masks: {}", e)))?;

    let embeddings = model
        .forward(&token_ids)
        .map_err(|e| SearchError::Embedding(format!("Forward pass failed: {}", e)))?;

    let pooled = mean_pool(&embeddings, &attention_mask)?;
    let normalized = normalize_l2(&pooled)?;

    let result: Vec<Vec<f32>> = (0..normalized.dim(0)?)
        .map(|i| normalized.get(i)?.to_vec1::<f32>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| SearchError::Embedding(format!("Failed to convert embeddings: {}", e)))?;

    Ok(result)
}

/// Encode texts using JinaBertV2Model (code model)
fn encode_with_model_v2(
    model: &JinaBertV2Model,
    tokenizer: &Tokenizer,
    device: &Device,
    texts: &[&str],
) -> Result<Vec<Vec<f32>>> {
    let mut tokenizer = tokenizer.clone();
    // IMPORTANT: The code model uses pad_token_id=1 (<pad>), not 0 (<s>)
    // The tokenizer.json doesn't specify this, so we must set it explicitly
    let padding = PaddingParams {
        strategy: PaddingStrategy::BatchLongest,
        pad_id: 1,                      // <pad> token ID for jina-embeddings-v2-base-code
        pad_token: "<pad>".to_string(), // Actual pad token
        ..Default::default()
    };
    tokenizer.with_padding(Some(padding));

    let encodings = tokenizer
        .encode_batch(texts.to_vec(), true)
        .map_err(|e| SearchError::Embedding(format!("Tokenization failed: {}", e)))?;

    let token_ids: Vec<Tensor> = encodings
        .iter()
        .map(|enc| {
            let ids = enc.get_ids().to_vec();
            Tensor::new(ids.as_slice(), device)
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| SearchError::Embedding(format!("Failed to create token tensor: {}", e)))?;

    let attention_masks: Vec<Tensor> = encodings
        .iter()
        .map(|enc| {
            let mask = enc.get_attention_mask().to_vec();
            Tensor::new(mask.as_slice(), device)
        })
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| SearchError::Embedding(format!("Failed to create attention mask: {}", e)))?;

    let token_ids = Tensor::stack(&token_ids, 0)
        .map_err(|e| SearchError::Embedding(format!("Failed to stack tokens: {}", e)))?;
    let attention_mask = Tensor::stack(&attention_masks, 0)
        .map_err(|e| SearchError::Embedding(format!("Failed to stack masks: {}", e)))?;

    // Use forward_with_mask to properly handle padding tokens in attention
    let embeddings = model
        .forward_with_mask(&token_ids, Some(&attention_mask))
        .map_err(|e| SearchError::Embedding(format!("Forward pass failed: {}", e)))?;

    let pooled = mean_pool(&embeddings, &attention_mask)?;
    let normalized = normalize_l2(&pooled)?;

    let result: Vec<Vec<f32>> = (0..normalized.dim(0)?)
        .map(|i| normalized.get(i)?.to_vec1::<f32>())
        .collect::<std::result::Result<Vec<_>, _>>()
        .map_err(|e| SearchError::Embedding(format!("Failed to convert embeddings: {}", e)))?;

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_provider_creation() {
        let provider = LocalProvider::new();
        assert!(provider.is_ok());
    }

    #[test]
    fn test_device_selection() {
        let device = select_device();
        assert!(device.is_ok());
    }

    #[test]
    fn test_provider_type() {
        let provider = LocalProvider::new().unwrap();
        assert_eq!(provider.provider_type(), EmbeddingProviderType::Local);
    }

    #[test]
    fn test_embedding_dim() {
        let provider = LocalProvider::new().unwrap();
        assert_eq!(provider.embedding_dim(), 768);
    }

    #[test]
    fn test_empty_input_semantic() {
        let provider = LocalProvider::new().unwrap();
        let result = provider.encode_semantic_sync(&[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_empty_input_code() {
        let provider = LocalProvider::new().unwrap();
        let result = provider.encode_code_sync(&[]);
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_check_status() {
        let provider = LocalProvider::new().unwrap();
        let status = provider.check_status().await;
        assert!(status.is_ok());
        let status = status.unwrap();
        assert_eq!(status.provider_type, EmbeddingProviderType::Local);
    }

    #[tokio::test]
    #[ignore] // Requires model download
    async fn test_semantic_encoding_async() {
        let provider = LocalProvider::new().unwrap();
        let texts = vec![
            "hello world".to_string(),
            "authentication logic".to_string(),
        ];
        let embeddings = provider.encode_semantic(texts).await.unwrap();

        assert_eq!(embeddings.len(), 2);
        assert_eq!(embeddings[0].len(), SEMANTIC_DIM);
        assert_eq!(embeddings[1].len(), SEMANTIC_DIM);
    }

    #[tokio::test]
    #[ignore] // Requires model download
    async fn test_code_encoding_async() {
        let provider = LocalProvider::new().unwrap();
        let code = vec!["fn main() { println!(\"hello\"); }".to_string()];
        let embeddings = provider.encode_code(code).await.unwrap();

        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].len(), CODE_DIM);
    }

    #[tokio::test]
    #[ignore] // Requires model download
    async fn test_warmup() {
        let provider = LocalProvider::new().unwrap();
        let result = provider.warmup().await;
        assert!(result.is_ok());

        let (semantic_loaded, code_loaded) = provider.is_loaded();
        assert!(semantic_loaded);
        assert!(code_loaded);
    }
}
