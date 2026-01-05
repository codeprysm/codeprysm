//! Embedding generation for semantic code search
//!
//! This module provides embedding generation with multiple provider backends:
//!
//! - **Local** - Candle-based inference with Jina models (CPU/Metal/CUDA)
//! - **Azure ML** - Azure ML Online Endpoints with managed Jina deployments
//! - **OpenAI** - OpenAI-compatible APIs (OpenAI, Azure OpenAI, Ollama, Prism SaaS)
//!
//! # Architecture
//!
//! The module uses a trait-based design for provider abstraction:
//!
//! ```text
//! EmbeddingProvider (trait)
//!     ├── LocalProvider     - Candle + Jina models
//!     ├── AzureMLProvider   - HTTP client for /score API
//!     └── OpenAIProvider    - HTTP client for /v1/embeddings API
//! ```
//!
//! `EmbeddingsManager` serves as a facade for backward compatibility,
//! delegating to the configured provider.
//!
//! # Example
//!
//! ```ignore
//! use codeprysm_search::embeddings::{EmbeddingsManager, EmbeddingProvider};
//!
//! // Default: uses LocalProvider
//! let manager = EmbeddingsManager::new()?;
//!
//! // Semantic embeddings for natural language
//! let queries = vec!["authentication logic", "error handling"];
//! let semantic_vecs = manager.encode_semantic(&queries)?;
//!
//! // Code embeddings for source code
//! let code = vec!["fn authenticate(user: &User) -> Result<Token>"];
//! let code_vecs = manager.encode_code(&code)?;
//! ```

pub mod azure_ml;
pub mod factory;
pub mod jina_bert_v2;
mod local;
pub mod openai;
mod provider;

// Re-export provider types
pub use provider::{EmbeddingProvider, EmbeddingProviderType, ProviderStatus};

// Re-export factory types and function
pub use factory::{create as create_provider, validate_dimension, EmbeddingConfig, EXPECTED_DIM};

// Re-export LocalProvider
pub use local::LocalProvider;

// Re-export AzureMLProvider
pub use azure_ml::{AzureMLAuth, AzureMLConfig, AzureMLProvider};

// Re-export OpenAIProvider
pub use openai::{OpenAIConfig, OpenAIProvider};

// Re-export embedding constants from local module
pub use local::{CODE_DIM, EMBEDDING_DIM, SEMANTIC_DIM};
