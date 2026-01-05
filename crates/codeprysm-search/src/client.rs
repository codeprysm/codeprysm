//! Qdrant client wrapper for codeprysm-search
//!
//! Provides a high-level interface for connecting to Qdrant and managing collections.
//!
//! # Migration Note (v0.2.0)
//!
//! The `semantic_search` collection dimension changed from 384 to 768 due to the
//! candle migration (all-MiniLM-L6-v2 → all-mpnet-base-v2). Existing collections
//! must be recreated with `just qdrant-stop && just qdrant-start && just init`.

use qdrant_client::qdrant::{
    point_id::PointIdOptions, vectors_config::Config, CreateCollectionBuilder,
    CreateFieldIndexCollectionBuilder, FieldType, PointStruct, SearchPointsBuilder,
    UpsertPointsBuilder, VectorParams, VectorsConfig,
};
use qdrant_client::{Payload, Qdrant};
use serde_json::json;
use tracing::{debug, info};

use crate::error::{Result, SearchError};
use crate::schema::{fields, CodePoint, CollectionConfig, EntityPayload, SearchHit};

/// Configuration for connecting to Qdrant
#[derive(Debug, Clone)]
pub struct QdrantConfig {
    /// Qdrant server URL (e.g., "http://localhost:6334")
    pub url: String,
    /// Optional API key for authentication
    pub api_key: Option<String>,
    /// Connection timeout in seconds
    pub timeout_secs: u64,
}

impl Default for QdrantConfig {
    fn default() -> Self {
        Self {
            url: "http://localhost:6334".to_string(),
            api_key: None,
            timeout_secs: 30,
        }
    }
}

impl QdrantConfig {
    /// Create config for local development
    pub fn local() -> Self {
        Self::default()
    }

    /// Create config with custom URL
    pub fn with_url(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Default::default()
        }
    }

    /// Set API key
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
        self
    }
}

/// Qdrant client wrapper for code search operations
pub struct QdrantStore {
    client: Qdrant,
    /// Repository ID for multi-tenant filtering
    repo_id: String,
}

impl QdrantStore {
    /// Connect to Qdrant server
    pub async fn connect(config: QdrantConfig, repo_id: impl Into<String>) -> Result<Self> {
        info!("Connecting to Qdrant at {}", config.url);

        let mut builder = Qdrant::from_url(&config.url);

        if let Some(api_key) = config.api_key {
            builder = builder.api_key(api_key);
        }

        let client = builder.build().map_err(|e| {
            SearchError::Connection(format!("Failed to build Qdrant client: {}", e))
        })?;

        // Test connection by listing collections
        client
            .list_collections()
            .await
            .map_err(|e| SearchError::Connection(format!("Failed to connect to Qdrant: {}", e)))?;

        info!("Successfully connected to Qdrant");

        Ok(Self {
            client,
            repo_id: repo_id.into(),
        })
    }

    /// Get the repository ID
    pub fn repo_id(&self) -> &str {
        &self.repo_id
    }

    /// Check if a collection exists
    pub async fn collection_exists(&self, name: &str) -> Result<bool> {
        let exists = self.client.collection_exists(name).await?;
        Ok(exists)
    }

    /// Create a collection with the given configuration
    pub async fn create_collection(&self, config: &CollectionConfig) -> Result<()> {
        if self.collection_exists(config.name).await? {
            debug!("Collection '{}' already exists", config.name);
            return Ok(());
        }

        info!(
            "Creating collection '{}' (dim={}, distance={:?})",
            config.name, config.dimension, config.distance
        );

        let vectors_config = VectorsConfig {
            config: Some(Config::Params(VectorParams {
                size: config.dimension,
                distance: config.distance.into(),
                ..Default::default()
            })),
        };

        self.client
            .create_collection(
                CreateCollectionBuilder::new(config.name).vectors_config(vectors_config),
            )
            .await?;

        // Create payload indexes for efficient filtering
        self.create_payload_indexes(config.name).await?;

        info!("Collection '{}' created successfully", config.name);
        Ok(())
    }

    /// Create payload indexes for efficient filtering
    async fn create_payload_indexes(&self, collection_name: &str) -> Result<()> {
        // Index repo_id for multi-tenant filtering (keyword)
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                collection_name,
                fields::REPO_ID,
                FieldType::Keyword,
            ))
            .await?;

        // Index entity_type for filtering by type (keyword)
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                collection_name,
                fields::ENTITY_TYPE,
                FieldType::Keyword,
            ))
            .await?;

        // Index kind for v2 schema filtering (keyword)
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                collection_name,
                fields::KIND,
                FieldType::Keyword,
            ))
            .await?;

        // Index file_path for file filtering (keyword for exact prefix matching)
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                collection_name,
                fields::FILE_PATH,
                FieldType::Keyword,
            ))
            .await?;

        // Index name for entity name search (keyword)
        self.client
            .create_field_index(CreateFieldIndexCollectionBuilder::new(
                collection_name,
                fields::NAME,
                FieldType::Keyword,
            ))
            .await?;

        debug!("Payload indexes created for '{}'", collection_name);
        Ok(())
    }

    /// Ensure both semantic and code collections exist
    pub async fn ensure_collections(&self) -> Result<()> {
        self.create_collection(&CollectionConfig::SEMANTIC).await?;
        self.create_collection(&CollectionConfig::CODE).await?;
        Ok(())
    }

    /// Delete a collection
    pub async fn delete_collection(&self, name: &str) -> Result<()> {
        if !self.collection_exists(name).await? {
            return Ok(());
        }

        info!("Deleting collection '{}'", name);
        self.client.delete_collection(name).await?;
        Ok(())
    }

    /// Upsert points to a collection
    pub async fn upsert_points(&self, collection_name: &str, points: Vec<CodePoint>) -> Result<()> {
        if points.is_empty() {
            return Ok(());
        }

        debug!("Upserting {} points to '{}'", points.len(), collection_name);

        let qdrant_points: Vec<PointStruct> = points
            .into_iter()
            .map(|p| {
                let payload = Payload::try_from(json!({
                    fields::REPO_ID: p.payload.repo_id,
                    "entity_id": p.payload.entity_id,
                    fields::NAME: p.payload.name,
                    fields::ENTITY_TYPE: p.payload.entity_type,
                    fields::KIND: p.payload.kind,
                    fields::SUBTYPE: p.payload.subtype,
                    fields::FILE_PATH: p.payload.file_path,
                    fields::START_LINE: p.payload.start_line,
                    fields::END_LINE: p.payload.end_line,
                    "content": p.content,
                }))
                .expect("Failed to create payload");

                PointStruct::new(p.id, p.vector, payload)
            })
            .collect();

        self.client
            .upsert_points(UpsertPointsBuilder::new(collection_name, qdrant_points).wait(true))
            .await?;

        Ok(())
    }

    /// Upsert points in batches to avoid timeouts
    pub async fn upsert_points_batched(
        &self,
        collection_name: &str,
        points: Vec<CodePoint>,
        batch_size: usize,
    ) -> Result<()> {
        let total = points.len();
        if total == 0 {
            return Ok(());
        }

        info!(
            "Upserting {} points to '{}' in batches of {}",
            total, collection_name, batch_size
        );

        for (i, batch) in points.chunks(batch_size).enumerate() {
            let batch_num = i + 1;
            let batch_total = total.div_ceil(batch_size);
            debug!(
                "Processing batch {}/{} ({} points)",
                batch_num,
                batch_total,
                batch.len()
            );

            self.upsert_points(collection_name, batch.to_vec()).await?;
        }

        info!("Successfully upserted {} points", total);
        Ok(())
    }

    /// Search for similar vectors
    pub async fn search(
        &self,
        collection_name: &str,
        query_vector: Vec<f32>,
        limit: u64,
        filter_types: Option<Vec<&str>>,
    ) -> Result<Vec<SearchHit>> {
        use qdrant_client::qdrant::{Condition, Filter};

        // Build filter - always filter by repo_id for multi-tenancy
        let mut filter = Filter::must([Condition::matches(fields::REPO_ID, self.repo_id.clone())]);

        // Add type filter if specified (OR condition for multiple types)
        if let Some(types) = filter_types {
            if !types.is_empty() {
                let type_conditions: Vec<Condition> = types
                    .into_iter()
                    .map(|t| Condition::matches(fields::ENTITY_TYPE, t.to_string()))
                    .collect();
                // Add as a should clause (OR) that must have at least one match
                filter.should = type_conditions;
                filter.min_should = Some(qdrant_client::qdrant::MinShould {
                    conditions: vec![],
                    min_count: 1,
                });
            }
        }

        let response = self
            .client
            .search_points(
                SearchPointsBuilder::new(collection_name, query_vector, limit)
                    .filter(filter)
                    .with_payload(true),
            )
            .await?;

        let hits = response
            .result
            .into_iter()
            .filter_map(|point| {
                let payload = point.payload;
                let point_id = point.id?;
                let id = match point_id.point_id_options? {
                    PointIdOptions::Num(n) => n,
                    PointIdOptions::Uuid(u) => {
                        // Hash UUID to u64 for compatibility
                        use std::hash::{Hash, Hasher};
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        u.hash(&mut hasher);
                        hasher.finish()
                    }
                };

                let get_string = |key: &str| -> String {
                    payload
                        .get(key)
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                };

                let get_u32 = |key: &str| -> u32 {
                    payload
                        .get(key)
                        .and_then(|v| v.as_integer())
                        .map(|i| i as u32)
                        .unwrap_or(0)
                };

                Some(SearchHit {
                    id,
                    score: point.score,
                    payload: EntityPayload {
                        repo_id: get_string(fields::REPO_ID),
                        entity_id: get_string("entity_id"),
                        name: get_string(fields::NAME),
                        entity_type: get_string(fields::ENTITY_TYPE),
                        kind: get_string(fields::KIND),
                        subtype: get_string(fields::SUBTYPE),
                        file_path: get_string(fields::FILE_PATH),
                        start_line: get_u32(fields::START_LINE),
                        end_line: get_u32(fields::END_LINE),
                    },
                    content: get_string("content"),
                })
            })
            .collect();

        Ok(hits)
    }

    /// Delete all points for the current repo_id from a collection
    pub async fn delete_repo_points(&self, collection_name: &str) -> Result<()> {
        use qdrant_client::qdrant::{Condition, DeletePointsBuilder, Filter};

        info!(
            "Deleting all points for repo '{}' from '{}'",
            self.repo_id, collection_name
        );

        let filter = Filter::must([Condition::matches(fields::REPO_ID, self.repo_id.clone())]);

        self.client
            .delete_points(
                DeletePointsBuilder::new(collection_name)
                    .points(filter)
                    .wait(true),
            )
            .await?;

        Ok(())
    }

    /// Delete all points for a specific file within the current repo
    ///
    /// Used for incremental indexing when a file is modified or deleted.
    pub async fn delete_points_by_file(
        &self,
        collection_name: &str,
        file_path: &str,
    ) -> Result<()> {
        use qdrant_client::qdrant::{Condition, DeletePointsBuilder, Filter};

        debug!(
            "Deleting points for file '{}' in repo '{}' from '{}'",
            file_path, self.repo_id, collection_name
        );

        // Must match both repo_id (for multi-tenancy) AND file_path
        let filter = Filter::must([
            Condition::matches(fields::REPO_ID, self.repo_id.clone()),
            Condition::matches(fields::FILE_PATH, file_path.to_string()),
        ]);

        self.client
            .delete_points(
                DeletePointsBuilder::new(collection_name)
                    .points(filter)
                    .wait(true),
            )
            .await?;

        Ok(())
    }

    /// Get collection info
    pub async fn collection_info(
        &self,
        name: &str,
    ) -> Result<Option<qdrant_client::qdrant::CollectionInfo>> {
        if !self.collection_exists(name).await? {
            return Ok(None);
        }

        let info = self.client.collection_info(name).await?;
        Ok(Some(info.result.expect("Collection info should exist")))
    }

    /// Scroll entities by exact name match (case-sensitive)
    ///
    /// Uses Qdrant's keyword filter on the indexed `name` field.
    /// Returns entities where name exactly matches the query.
    pub async fn scroll_by_name(
        &self,
        collection_name: &str,
        name: &str,
        limit: u32,
    ) -> Result<Vec<SearchHit>> {
        use qdrant_client::qdrant::{Condition, Filter, ScrollPointsBuilder};

        // Filter by repo_id (multi-tenancy) AND exact name match
        let filter = Filter::must([
            Condition::matches(fields::REPO_ID, self.repo_id.clone()),
            Condition::matches(fields::NAME, name.to_string()),
        ]);

        let response = self
            .client
            .scroll(
                ScrollPointsBuilder::new(collection_name)
                    .filter(filter)
                    .limit(limit)
                    .with_payload(true),
            )
            .await?;

        let hits = response
            .result
            .into_iter()
            .filter_map(|point| {
                let payload = point.payload;
                let point_id = point.id?;
                let id = match point_id.point_id_options? {
                    qdrant_client::qdrant::point_id::PointIdOptions::Num(n) => n,
                    qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u) => {
                        use std::hash::{Hash, Hasher};
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        u.hash(&mut hasher);
                        hasher.finish()
                    }
                };

                let get_string = |key: &str| -> String {
                    payload
                        .get(key)
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                };

                let get_u32 = |key: &str| -> u32 {
                    payload
                        .get(key)
                        .and_then(|v| v.as_integer())
                        .map(|i| i as u32)
                        .unwrap_or(0)
                };

                Some(SearchHit {
                    id,
                    score: 1.0, // Name match gets perfect score
                    payload: EntityPayload {
                        repo_id: get_string(fields::REPO_ID),
                        entity_id: get_string("entity_id"),
                        name: get_string(fields::NAME),
                        entity_type: get_string(fields::ENTITY_TYPE),
                        kind: get_string(fields::KIND),
                        subtype: get_string(fields::SUBTYPE),
                        file_path: get_string(fields::FILE_PATH),
                        start_line: get_u32(fields::START_LINE),
                        end_line: get_u32(fields::END_LINE),
                    },
                    content: get_string("content"),
                })
            })
            .collect();

        Ok(hits)
    }

    /// Scroll all entities for this repo (for small repos / debugging)
    ///
    /// WARNING: Only use for small repos. For large repos, use filtered scroll.
    pub async fn scroll_all(&self, collection_name: &str, limit: u32) -> Result<Vec<SearchHit>> {
        use qdrant_client::qdrant::{Condition, Filter, ScrollPointsBuilder};

        let filter = Filter::must([Condition::matches(fields::REPO_ID, self.repo_id.clone())]);

        let response = self
            .client
            .scroll(
                ScrollPointsBuilder::new(collection_name)
                    .filter(filter)
                    .limit(limit)
                    .with_payload(true),
            )
            .await?;

        let hits = response
            .result
            .into_iter()
            .filter_map(|point| {
                let payload = point.payload;
                let point_id = point.id?;
                let id = match point_id.point_id_options? {
                    qdrant_client::qdrant::point_id::PointIdOptions::Num(n) => n,
                    qdrant_client::qdrant::point_id::PointIdOptions::Uuid(u) => {
                        use std::hash::{Hash, Hasher};
                        let mut hasher = std::collections::hash_map::DefaultHasher::new();
                        u.hash(&mut hasher);
                        hasher.finish()
                    }
                };

                let get_string = |key: &str| -> String {
                    payload
                        .get(key)
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                        .unwrap_or_default()
                };

                let get_u32 = |key: &str| -> u32 {
                    payload
                        .get(key)
                        .and_then(|v| v.as_integer())
                        .map(|i| i as u32)
                        .unwrap_or(0)
                };

                Some(SearchHit {
                    id,
                    score: 0.0, // No score for scroll
                    payload: EntityPayload {
                        repo_id: get_string(fields::REPO_ID),
                        entity_id: get_string("entity_id"),
                        name: get_string(fields::NAME),
                        entity_type: get_string(fields::ENTITY_TYPE),
                        kind: get_string(fields::KIND),
                        subtype: get_string(fields::SUBTYPE),
                        file_path: get_string(fields::FILE_PATH),
                        start_line: get_u32(fields::START_LINE),
                        end_line: get_u32(fields::END_LINE),
                    },
                    content: get_string("content"),
                })
            })
            .collect();

        Ok(hits)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_defaults() {
        let config = QdrantConfig::default();
        assert_eq!(config.url, "http://localhost:6334");
        assert!(config.api_key.is_none());
        assert_eq!(config.timeout_secs, 30);
    }

    #[test]
    fn test_config_builder() {
        let config = QdrantConfig::with_url("http://qdrant:6334").api_key("test-key");
        assert_eq!(config.url, "http://qdrant:6334");
        assert_eq!(config.api_key, Some("test-key".to_string()));
    }

    #[test]
    fn test_point_id_generation() {
        let id1 = CodePoint::generate_id("src/lib.rs:MyStruct", "repo1");
        let id2 = CodePoint::generate_id("src/lib.rs:MyStruct", "repo1");
        let id3 = CodePoint::generate_id("src/lib.rs:MyStruct", "repo2");

        assert_eq!(id1, id2); // Same inputs = same ID
        assert_ne!(id1, id3); // Different repo = different ID
    }

    #[test]
    fn test_collection_configs() {
        // Both collections now use 768 dimensions (candle migration)
        assert_eq!(CollectionConfig::SEMANTIC.dimension, 768);
        assert_eq!(CollectionConfig::CODE.dimension, 768);
    }
}
