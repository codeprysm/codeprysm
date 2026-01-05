//! CodePrysm Search - Semantic code search using Qdrant and embeddings
//!
//! This crate provides vector search capabilities for code entities,
//! supporting both semantic (natural language) and code-aware search.
//!
//! # Features
//!
//! - **Multi-tenant**: Each repository has isolated search results via `repo_id`
//! - **Hybrid search**: Combines semantic and code embeddings for better results
//! - **Qdrant backend**: Production-ready vector database with filtering support
//!
//! # Example
//!
//! ```ignore
//! use codeprysm_search::{QdrantStore, QdrantConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Connect to Qdrant
//!     let config = QdrantConfig::local();
//!     let store = QdrantStore::connect(config, "my-repo").await?;
//!
//!     // Ensure collections exist
//!     store.ensure_collections().await?;
//!
//!     // Search (with embeddings from candle)
//!     let results = store.search("semantic_search", query_vector, 10, None).await?;
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod embeddings;
pub mod error;
pub mod hybrid;
pub mod indexer;
pub mod schema;
pub mod semantic_text;

// Re-export jina_bert_v2 from embeddings for backward compatibility
pub use embeddings::jina_bert_v2;

// Legacy embeddings implementation (will be migrated to embeddings/local.rs)
mod embeddings_legacy;

// Re-exports for convenience
pub use client::{QdrantConfig, QdrantStore};
pub use error::{Result, SearchError};
pub use hybrid::{HybridSearchHit, HybridSearcher, QueryType, ScoringConfig, WeightPreset};
pub use indexer::{GraphIndexer, IndexStats};
pub use schema::{CodePoint, CollectionConfig, EntityPayload, SearchHit};
pub use semantic_text::SemanticTextBuilder;

// Re-export legacy embeddings types for backward compatibility
pub use embeddings_legacy::{
    EmbeddingsManager, ModelStatus, CODE_DIM, EMBEDDING_DIM, SEMANTIC_DIM,
};

// Re-export new provider abstraction types
pub use embeddings::{
    create_provider, validate_dimension, AzureMLAuth, AzureMLConfig, AzureMLProvider,
    EmbeddingConfig, EmbeddingProvider, EmbeddingProviderType, LocalProvider, OpenAIConfig,
    OpenAIProvider, ProviderStatus, EXPECTED_DIM,
};
