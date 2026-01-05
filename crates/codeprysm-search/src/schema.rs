//! Collection schemas and point types for Qdrant
//!
//! Defines the structure of collections and points used for semantic code search.

use qdrant_client::qdrant::Distance;
use serde::{Deserialize, Serialize};

/// Collection names used by codeprysm-search
pub mod collections {
    /// Semantic search collection (text/description embeddings)
    pub const SEMANTIC: &str = "semantic_search";
    /// Code search collection (code embeddings)
    pub const CODE: &str = "code_search";
}

/// Configuration for a vector collection
#[derive(Debug, Clone)]
pub struct CollectionConfig {
    /// Collection name
    pub name: &'static str,
    /// Vector dimension
    pub dimension: u64,
    /// Distance metric
    pub distance: Distance,
    /// Description for documentation
    pub description: &'static str,
}

/// Predefined collection configurations
impl CollectionConfig {
    /// Semantic search collection (all-mpnet-base-v2: 768 dimensions)
    pub const SEMANTIC: CollectionConfig = CollectionConfig {
        name: collections::SEMANTIC,
        dimension: 768,
        distance: Distance::Cosine,
        description: "Semantic embeddings from all-mpnet-base-v2 for natural language queries",
    };

    /// Code search collection (Jina Embeddings v2 Base Code: 768 dimensions)
    pub const CODE: CollectionConfig = CollectionConfig {
        name: collections::CODE,
        dimension: 768,
        distance: Distance::Cosine,
        description: "Code embeddings from Jina v2 Base Code for code-aware search",
    };
}

/// Payload field names for indexed filtering
pub mod fields {
    /// Repository identifier for multi-tenant filtering
    pub const REPO_ID: &str = "repo_id";
    /// Entity type (Container, Callable, Data). Files are Container with kind="file".
    pub const ENTITY_TYPE: &str = "type";
    /// Entity kind (v2 schema: type, function, method, field, etc.)
    pub const KIND: &str = "kind";
    /// Entity subtype (v2 schema: class, struct, interface, async, etc.)
    pub const SUBTYPE: &str = "subtype";
    /// File path
    pub const FILE_PATH: &str = "file_path";
    /// Entity name
    pub const NAME: &str = "name";
    /// Start line number
    pub const START_LINE: &str = "start_line";
    /// End line number
    pub const END_LINE: &str = "end_line";
}

/// Metadata payload for a code entity point
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityPayload {
    /// Repository identifier for multi-tenant filtering
    pub repo_id: String,
    /// Entity ID (e.g., "src/lib.rs:MyStruct:new")
    pub entity_id: String,
    /// Entity name
    pub name: String,
    /// Entity type (Container, Callable, Data). Files are Container with kind="file".
    #[serde(rename = "type")]
    pub entity_type: String,
    /// Entity kind (v2 schema)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    /// Entity subtype (v2 schema)
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub subtype: String,
    /// File path relative to repo root
    pub file_path: String,
    /// Start line number (1-indexed)
    pub start_line: u32,
    /// End line number (1-indexed)
    pub end_line: u32,
}

/// A point to upsert into a collection
#[derive(Debug, Clone)]
pub struct CodePoint {
    /// Unique point ID (hash of entity_id + repo_id)
    pub id: u64,
    /// Vector embedding
    pub vector: Vec<f32>,
    /// Metadata payload
    pub payload: EntityPayload,
    /// Text content (code snippet or semantic description)
    pub content: String,
}

impl CodePoint {
    /// Generate a unique point ID from entity_id and repo_id
    pub fn generate_id(entity_id: &str, repo_id: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        entity_id.hash(&mut hasher);
        repo_id.hash(&mut hasher);
        hasher.finish()
    }
}

/// Search result from a vector query
#[derive(Debug, Clone)]
pub struct SearchHit {
    /// Point ID
    pub id: u64,
    /// Similarity score (0.0 to 1.0 for cosine)
    pub score: f32,
    /// Entity payload
    pub payload: EntityPayload,
    /// Content (code snippet or description)
    pub content: String,
}
