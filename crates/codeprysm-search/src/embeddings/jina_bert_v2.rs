//! JinaBERT v2 QK-Post-Norm implementation
//!
//! This is a custom implementation for jina-embeddings-v2-base-code which uses
//! the `jinaai/jina-bert-v2-qk-post-norm` architecture, different from the standard
//! jina-bert-implementation used by jina-embeddings-v2-base-en.
//!
//! Key differences from standard JinaBERT:
//! - MLP uses `up_gated_layer` and `down_layer` instead of `gated_layers` and `wo`
//! - No layernorm inside MLP (uses layer_norm_q/k in attention instead)
//! - Attention has QK post-normalization via `layer_norm_q` and `layer_norm_k`

use candle_core::{DType, Device, IndexOp, Module, Result, Tensor, D};
use candle_nn::{layer_norm, Embedding, LayerNorm, Linear, VarBuilder};
use serde::Deserialize;

/// Data type for model inference
pub const DTYPE: DType = DType::F32;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PositionEmbeddingType {
    Absolute,
    Alibi,
}

/// Configuration for JinaBERT v2 QK-Post-Norm model
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Config {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub intermediate_size: usize,
    pub hidden_act: candle_nn::Activation,
    pub max_position_embeddings: usize,
    pub type_vocab_size: usize,
    pub initializer_range: f64,
    pub layer_norm_eps: f64,
    pub pad_token_id: usize,
    pub position_embedding_type: PositionEmbeddingType,
}

impl Config {
    /// Default config for jina-embeddings-v2-base-code
    pub fn v2_base_code() -> Self {
        Self {
            vocab_size: 49152,
            hidden_size: 768,
            num_hidden_layers: 12,
            num_attention_heads: 12,
            intermediate_size: 3072,
            hidden_act: candle_nn::Activation::Gelu,
            max_position_embeddings: 8192,
            type_vocab_size: 2,
            initializer_range: 0.02,
            layer_norm_eps: 1e-12,
            pad_token_id: 0,
            position_embedding_type: PositionEmbeddingType::Alibi,
        }
    }
}

/// Helper to create linear layer with bias
fn linear(in_dim: usize, out_dim: usize, vb: VarBuilder) -> Result<Linear> {
    let weight = vb.get((out_dim, in_dim), "weight")?;
    let bias = vb.get(out_dim, "bias")?;
    Ok(Linear::new(weight, Some(bias)))
}

/// Helper to create linear layer without bias
fn linear_no_bias(in_dim: usize, out_dim: usize, vb: VarBuilder) -> Result<Linear> {
    let weight = vb.get((out_dim, in_dim), "weight")?;
    Ok(Linear::new(weight, None))
}

/// Embedding layer for JinaBERT v2
#[derive(Clone, Debug)]
struct BertEmbeddings {
    word_embeddings: Embedding,
    token_type_embeddings: Embedding,
    layer_norm: LayerNorm,
}

impl BertEmbeddings {
    fn new(vb: VarBuilder, cfg: &Config) -> Result<Self> {
        let word_embeddings = Embedding::new(
            vb.pp("word_embeddings")
                .get((cfg.vocab_size, cfg.hidden_size), "weight")?,
            cfg.hidden_size,
        );
        let token_type_embeddings = Embedding::new(
            vb.pp("token_type_embeddings")
                .get((cfg.type_vocab_size, cfg.hidden_size), "weight")?,
            cfg.hidden_size,
        );
        let layer_norm = layer_norm(cfg.hidden_size, cfg.layer_norm_eps, vb.pp("LayerNorm"))?;
        Ok(Self {
            word_embeddings,
            token_type_embeddings,
            layer_norm,
        })
    }
}

impl Module for BertEmbeddings {
    fn forward(&self, input_ids: &Tensor) -> Result<Tensor> {
        let (b_size, seq_len) = input_ids.dims2()?;
        let input_embeddings = self.word_embeddings.forward(input_ids)?;
        let token_type_ids = Tensor::zeros((b_size, seq_len), DType::U32, input_ids.device())?;
        let token_type_embeddings = self.token_type_embeddings.forward(&token_type_ids)?;
        let embeddings = (&input_embeddings + token_type_embeddings)?;
        self.layer_norm.forward(&embeddings)
    }
}

/// Self-attention with QK post-normalization
#[derive(Clone, Debug)]
struct BertSelfAttentionV2 {
    query: Linear,
    key: Linear,
    value: Linear,
    layer_norm_q: LayerNorm,
    layer_norm_k: LayerNorm,
    num_attention_heads: usize,
    attention_head_size: usize,
}

impl BertSelfAttentionV2 {
    fn new(vb: VarBuilder, cfg: &Config) -> Result<Self> {
        let attention_head_size = cfg.hidden_size / cfg.num_attention_heads;
        let all_head_size = cfg.num_attention_heads * attention_head_size;
        let hidden_size = cfg.hidden_size;

        let query = linear(hidden_size, all_head_size, vb.pp("query"))?;
        let key = linear(hidden_size, all_head_size, vb.pp("key"))?;
        let value = linear(hidden_size, all_head_size, vb.pp("value"))?;

        // QK post-normalization layers (v2 specific)
        let layer_norm_q = layer_norm(all_head_size, cfg.layer_norm_eps, vb.pp("layer_norm_q"))?;
        let layer_norm_k = layer_norm(all_head_size, cfg.layer_norm_eps, vb.pp("layer_norm_k"))?;

        Ok(Self {
            query,
            key,
            value,
            layer_norm_q,
            layer_norm_k,
            num_attention_heads: cfg.num_attention_heads,
            attention_head_size,
        })
    }

    fn transpose_for_scores(&self, xs: &Tensor) -> Result<Tensor> {
        let mut x_shape = xs.dims().to_vec();
        x_shape.pop();
        x_shape.push(self.num_attention_heads);
        x_shape.push(self.attention_head_size);
        xs.reshape(x_shape)?.transpose(1, 2)?.contiguous()
    }

    fn forward(
        &self,
        xs: &Tensor,
        alibi_bias: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        // Linear projections
        let query_layer = self.query.forward(xs)?;
        let key_layer = self.key.forward(xs)?;
        let value_layer = self.value.forward(xs)?;

        // QK post-normalization (v2 specific)
        let query_layer = self.layer_norm_q.forward(&query_layer)?;
        let key_layer = self.layer_norm_k.forward(&key_layer)?;

        // Reshape for multi-head attention
        let query_layer = self.transpose_for_scores(&query_layer)?;
        let key_layer = self.transpose_for_scores(&key_layer)?;
        let value_layer = self.transpose_for_scores(&value_layer)?;

        // Attention scores
        let attention_scores = query_layer.matmul(&key_layer.t()?)?;
        let attention_scores = (attention_scores / (self.attention_head_size as f64).sqrt())?;
        let attention_scores = attention_scores.broadcast_add(alibi_bias)?;

        // Apply attention mask if provided (mask out padding tokens)
        let attention_scores = if let Some(mask) = attention_mask {
            // mask is (batch, seq_len), need to reshape to (batch, 1, 1, seq_len)
            // and convert 0s to large negative values
            let mask = mask.to_dtype(DType::F32)?;
            let mask = mask.unsqueeze(1)?.unsqueeze(1)?; // (batch, 1, 1, seq_len)
                                                         // Where mask is 0, add large negative; where mask is 1, add 0
            let mask = ((mask.neg()? + 1.0)? * -10000.0)?;
            attention_scores.broadcast_add(&mask)?
        } else {
            attention_scores
        };

        let attention_probs = candle_nn::ops::softmax_last_dim(&attention_scores)?;

        // Apply attention to values
        let context_layer = attention_probs.matmul(&value_layer)?;
        let context_layer = context_layer.transpose(1, 2)?.contiguous()?;
        let context_layer = context_layer.flatten_from(D::Minus2)?;
        Ok(context_layer)
    }
}

/// Self-output layer
#[derive(Clone, Debug)]
struct BertSelfOutput {
    dense: Linear,
    layer_norm: LayerNorm,
}

impl BertSelfOutput {
    fn new(vb: VarBuilder, cfg: &Config) -> Result<Self> {
        let dense = linear(cfg.hidden_size, cfg.hidden_size, vb.pp("dense"))?;
        let layer_norm = layer_norm(cfg.hidden_size, cfg.layer_norm_eps, vb.pp("LayerNorm"))?;
        Ok(Self { dense, layer_norm })
    }

    fn forward(&self, xs: &Tensor, input_tensor: &Tensor) -> Result<Tensor> {
        let xs = self.dense.forward(xs)?;
        self.layer_norm.forward(&(xs + input_tensor)?)
    }
}

/// Full attention block
#[derive(Clone, Debug)]
struct BertAttention {
    self_attention: BertSelfAttentionV2,
    self_output: BertSelfOutput,
}

impl BertAttention {
    fn new(vb: VarBuilder, cfg: &Config) -> Result<Self> {
        let self_attention = BertSelfAttentionV2::new(vb.pp("self"), cfg)?;
        let self_output = BertSelfOutput::new(vb.pp("output"), cfg)?;
        Ok(Self {
            self_attention,
            self_output,
        })
    }

    fn forward(
        &self,
        xs: &Tensor,
        alibi_bias: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let self_outputs = self
            .self_attention
            .forward(xs, alibi_bias, attention_mask)?;
        self.self_output.forward(&self_outputs, xs)
    }
}

/// GLU MLP for v2 QK-Post-Norm (different weight names from standard JinaBERT)
///
/// Uses `up_gated_layer` and `down_layer` instead of `gated_layers` and `wo`
/// Note: Residual connections are handled at the layer level, not here
#[derive(Clone, Debug)]
struct BertGLUMLPV2 {
    up_gated_layer: Linear,
    down_layer: Linear,
    act: candle_nn::Activation,
    intermediate_size: usize,
}

impl BertGLUMLPV2 {
    fn new(vb: VarBuilder, cfg: &Config) -> Result<Self> {
        // v2 uses different weight names: up_gated_layer instead of gated_layers
        let up_gated_layer = linear_no_bias(
            cfg.hidden_size,
            cfg.intermediate_size * 2,
            vb.pp("up_gated_layer"),
        )?;
        // v2 uses down_layer instead of wo
        let down_layer = linear(cfg.intermediate_size, cfg.hidden_size, vb.pp("down_layer"))?;
        let act = candle_nn::Activation::Gelu;

        Ok(Self {
            up_gated_layer,
            down_layer,
            act,
            intermediate_size: cfg.intermediate_size,
        })
    }
}

impl Module for BertGLUMLPV2 {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        // No residual here - handled at layer level
        let xs = self.up_gated_layer.forward(xs)?;
        // Split into up and gated parts
        // Python: up = first half, gated = second half
        // Result = up * GELU(gated)
        let up = xs.narrow(D::Minus1, 0, self.intermediate_size)?;
        let gated = xs.narrow(D::Minus1, self.intermediate_size, self.intermediate_size)?;
        let xs = (up * gated.apply(&self.act))?;
        self.down_layer.forward(&xs)
    }
}

/// A single transformer layer
///
/// Architecture (per official Jina implementation):
/// 1. attention_output = Attention(hidden_states) [includes internal residual + LayerNorm]
/// 2. x = layer_norm_1(hidden_states + attention_output)
/// 3. mlp_output = MLP(x)
/// 4. output = layer_norm_2(x + mlp_output)
#[derive(Clone, Debug)]
struct BertLayer {
    attention: BertAttention,
    layer_norm_1: LayerNorm,
    mlp: BertGLUMLPV2,
    layer_norm_2: LayerNorm,
}

impl BertLayer {
    fn new(vb: VarBuilder, cfg: &Config) -> Result<Self> {
        let attention = BertAttention::new(vb.pp("attention"), cfg)?;
        let layer_norm_1 = layer_norm(cfg.hidden_size, cfg.layer_norm_eps, vb.pp("layer_norm_1"))?;
        let mlp = BertGLUMLPV2::new(vb.pp("mlp"), cfg)?;
        let layer_norm_2 = layer_norm(cfg.hidden_size, cfg.layer_norm_eps, vb.pp("layer_norm_2"))?;
        Ok(Self {
            attention,
            layer_norm_1,
            mlp,
            layer_norm_2,
        })
    }

    fn forward(
        &self,
        xs: &Tensor,
        alibi_bias: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        // Step 1: Self-attention (includes internal residual + LayerNorm in output)
        let attention_output = self.attention.forward(xs, alibi_bias, attention_mask)?;

        // Step 2: Add original hidden_states + layer_norm_1
        let after_attn = self.layer_norm_1.forward(&xs.add(&attention_output)?)?;

        // Step 3: MLP (no internal residual)
        let mlp_output = self.mlp.forward(&after_attn)?;

        // Step 4: Add + layer_norm_2
        self.layer_norm_2.forward(&after_attn.add(&mlp_output)?)
    }
}

/// Build ALiBi attention bias
fn build_alibi_bias(cfg: &Config) -> Result<Tensor> {
    let n_heads = cfg.num_attention_heads;
    let seq_len = cfg.max_position_embeddings;
    let alibi_bias = Tensor::arange(0, seq_len as i64, &Device::Cpu)?.to_dtype(DType::F32)?;
    let alibi_bias = {
        let a1 = alibi_bias.reshape((1, seq_len))?;
        let a2 = alibi_bias.reshape((seq_len, 1))?;
        a1.broadcast_sub(&a2)?.abs()?.broadcast_left(n_heads)?
    };
    let mut n_heads2 = 1;
    while n_heads2 < n_heads {
        n_heads2 *= 2
    }
    let slopes = (1..=n_heads2)
        .map(|v| -1f32 / 2f32.powf((v * 8) as f32 / n_heads2 as f32))
        .collect::<Vec<_>>();
    let slopes = if n_heads2 == n_heads {
        slopes
    } else {
        slopes
            .iter()
            .skip(1)
            .step_by(2)
            .chain(slopes.iter().step_by(2))
            .take(n_heads)
            .cloned()
            .collect::<Vec<f32>>()
    };
    let slopes = Tensor::new(slopes, &Device::Cpu)?.reshape((1, (), 1, 1))?;
    alibi_bias.to_dtype(DType::F32)?.broadcast_mul(&slopes)
}

/// The encoder stack
#[derive(Clone, Debug)]
struct BertEncoder {
    alibi: Tensor,
    layers: Vec<BertLayer>,
}

impl BertEncoder {
    fn new(vb: VarBuilder, cfg: &Config) -> Result<Self> {
        if cfg.position_embedding_type != PositionEmbeddingType::Alibi {
            candle_core::bail!("only alibi is supported as a position-embedding-type")
        }
        let layers = (0..cfg.num_hidden_layers)
            .map(|index| BertLayer::new(vb.pp(format!("layer.{index}")), cfg))
            .collect::<Result<Vec<_>>>()?;
        let alibi = build_alibi_bias(cfg)?.to_device(vb.device())?;
        Ok(Self { alibi, layers })
    }
}

impl BertEncoder {
    fn forward_with_mask(&self, xs: &Tensor, attention_mask: Option<&Tensor>) -> Result<Tensor> {
        let seq_len = xs.dim(1)?;
        let alibi_bias = self.alibi.i((.., .., ..seq_len, ..seq_len))?;
        let mut xs = xs.clone();
        for layer in self.layers.iter() {
            xs = layer.forward(&xs, &alibi_bias, attention_mask)?
        }
        Ok(xs)
    }
}

impl Module for BertEncoder {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        self.forward_with_mask(xs, None)
    }
}

/// JinaBERT v2 QK-Post-Norm Model
///
/// This model is compatible with jina-embeddings-v2-base-code which uses
/// the jina-bert-v2-qk-post-norm architecture.
#[derive(Clone, Debug)]
pub struct BertModel {
    embeddings: BertEmbeddings,
    encoder: BertEncoder,
    pub device: Device,
}

impl BertModel {
    pub fn new(vb: VarBuilder, cfg: &Config) -> Result<Self> {
        let embeddings = BertEmbeddings::new(vb.pp("embeddings"), cfg)?;
        let encoder = BertEncoder::new(vb.pp("encoder"), cfg)?;
        Ok(Self {
            embeddings,
            encoder,
            device: vb.device().clone(),
        })
    }

    /// Forward pass with optional attention mask for padding
    pub fn forward_with_mask(
        &self,
        input_ids: &Tensor,
        attention_mask: Option<&Tensor>,
    ) -> Result<Tensor> {
        let embedding_output = self.embeddings.forward(input_ids)?;
        self.encoder
            .forward_with_mask(&embedding_output, attention_mask)
    }
}

impl Module for BertModel {
    fn forward(&self, input_ids: &Tensor) -> Result<Tensor> {
        self.forward_with_mask(input_ids, None)
    }
}
