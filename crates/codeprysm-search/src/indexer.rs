//! Code graph indexer for Qdrant
//!
//! Indexes code graph nodes into Qdrant collections for semantic search.
//!
//! # Example
//!
//! ```ignore
//! use codeprysm_search::{GraphIndexer, QdrantConfig};
//! use codeprysm_core::PetCodeGraph;
//! use std::path::Path;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Load graph from partitions or build it
//!     let graph = PetCodeGraph::new(); // ... populate graph
//!     let mut indexer = GraphIndexer::new(
//!         QdrantConfig::local(),
//!         "my-repo",
//!         Path::new("/path/to/repo"),
//!     ).await?;
//!
//!     let stats = indexer.index_graph(&graph).await?;
//!     println!("Indexed {} nodes", stats.total_indexed);
//!     Ok(())
//! }
//! ```

use std::path::Path;
use std::sync::Arc;

use codeprysm_core::{Node, PetCodeGraph};
use tracing::{debug, info};

use crate::client::{QdrantConfig, QdrantStore};
use crate::embeddings::{EmbeddingConfig, EmbeddingProvider, EmbeddingProviderType};
use crate::error::Result;
use crate::schema::{collections, CodePoint, EntityPayload};
use crate::semantic_text::SemanticTextBuilder;
use crate::EmbeddingsManager;

/// Statistics from indexing operation
#[derive(Debug, Clone, Default)]
pub struct IndexStats {
    /// Total nodes processed
    pub total_processed: usize,
    /// Nodes successfully indexed
    pub total_indexed: usize,
    /// Nodes skipped (e.g., FILE nodes without content)
    pub total_skipped: usize,
    /// Nodes that failed to index
    pub total_failed: usize,
    /// Nodes indexed to semantic collection
    pub semantic_indexed: usize,
    /// Nodes indexed to code collection
    pub code_indexed: usize,
}

/// Embedding source - either legacy EmbeddingsManager or new provider
#[allow(clippy::large_enum_variant)]
enum EmbeddingSource {
    /// Legacy sync embedding manager
    Legacy(EmbeddingsManager),
    /// New async provider (wrapped in Arc for Send + Sync)
    Provider(Arc<dyn EmbeddingProvider>),
}

/// Graph indexer for populating Qdrant with code graph data
pub struct GraphIndexer {
    store: QdrantStore,
    embedding_source: EmbeddingSource,
    repo_id: String,
    repo_path: std::path::PathBuf,
    /// Batch size for upserting points to Qdrant
    batch_size: usize,
    /// Batch size for embedding API calls (optimizes remote provider performance)
    embedding_batch_size: usize,
}

impl GraphIndexer {
    /// Create a new graph indexer with default local provider
    pub async fn new(
        config: QdrantConfig,
        repo_id: impl Into<String>,
        repo_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let repo_id = repo_id.into();
        let store = QdrantStore::connect(config, &repo_id).await?;
        let embeddings = EmbeddingsManager::new()?;

        Ok(Self {
            store,
            embedding_source: EmbeddingSource::Legacy(embeddings),
            repo_id,
            repo_path: repo_path.as_ref().to_path_buf(),
            batch_size: 100,
            embedding_batch_size: 200,
        })
    }

    /// Create a new graph indexer with a specific provider
    pub async fn with_provider(
        config: QdrantConfig,
        repo_id: impl Into<String>,
        repo_path: impl AsRef<Path>,
        provider: Arc<dyn EmbeddingProvider>,
    ) -> Result<Self> {
        let repo_id = repo_id.into();
        let store = QdrantStore::connect(config, &repo_id).await?;

        Ok(Self {
            store,
            embedding_source: EmbeddingSource::Provider(provider),
            repo_id,
            repo_path: repo_path.as_ref().to_path_buf(),
            batch_size: 100,
            embedding_batch_size: 200,
        })
    }

    /// Create a new graph indexer from embedding config
    pub async fn from_config(
        qdrant_config: QdrantConfig,
        embedding_config: &EmbeddingConfig,
        repo_id: impl Into<String>,
        repo_path: impl AsRef<Path>,
    ) -> Result<Self> {
        let provider = crate::embeddings::create_provider(embedding_config)?;
        tracing::info!(
            "Using embedding provider: {:?} (dim={})",
            provider.provider_type(),
            provider.embedding_dim()
        );
        Self::with_provider(qdrant_config, repo_id, repo_path, provider).await
    }

    /// Create from existing store and embeddings (legacy)
    pub fn from_components(
        store: QdrantStore,
        embeddings: EmbeddingsManager,
        repo_id: impl Into<String>,
        repo_path: impl AsRef<Path>,
    ) -> Self {
        Self {
            store,
            embedding_source: EmbeddingSource::Legacy(embeddings),
            repo_id: repo_id.into(),
            repo_path: repo_path.as_ref().to_path_buf(),
            batch_size: 100,
            embedding_batch_size: 200,
        }
    }

    /// Set batch size for upserts
    pub fn with_batch_size(mut self, batch_size: usize) -> Self {
        self.batch_size = batch_size;
        self
    }

    /// Set batch size for embedding API calls
    ///
    /// Higher values reduce API round-trips but increase memory usage.
    /// Default is 50, which balances latency and throughput for remote providers.
    pub fn with_embedding_batch_size(mut self, size: usize) -> Self {
        self.embedding_batch_size = size;
        self
    }

    /// Get reference to the underlying store
    pub fn store(&self) -> &QdrantStore {
        &self.store
    }

    /// Get the embedding dimension for the current provider
    pub fn embedding_dim(&self) -> usize {
        match &self.embedding_source {
            EmbeddingSource::Legacy(_) => 768, // Jina models always 768
            EmbeddingSource::Provider(p) => p.embedding_dim(),
        }
    }

    /// Encode a single semantic query
    #[allow(dead_code)]
    async fn encode_semantic(&self, text: &str) -> Result<Vec<f32>> {
        match &self.embedding_source {
            EmbeddingSource::Legacy(mgr) => mgr.encode_semantic_query(text),
            EmbeddingSource::Provider(provider) => {
                let results = provider.encode_semantic(vec![text.to_string()]).await?;
                results.into_iter().next().ok_or_else(|| {
                    crate::error::SearchError::Embedding("No embedding returned".into())
                })
            }
        }
    }

    /// Encode a single code query
    #[allow(dead_code)]
    async fn encode_code(&self, text: &str) -> Result<Vec<f32>> {
        match &self.embedding_source {
            EmbeddingSource::Legacy(mgr) => mgr.encode_code_query(text),
            EmbeddingSource::Provider(provider) => {
                let results = provider.encode_code(vec![text.to_string()]).await?;
                results.into_iter().next().ok_or_else(|| {
                    crate::error::SearchError::Embedding("No embedding returned".into())
                })
            }
        }
    }

    /// Encode a batch of semantic texts
    async fn encode_semantic_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        match &self.embedding_source {
            EmbeddingSource::Legacy(mgr) => {
                // Legacy manager doesn't support batching, encode one at a time
                texts.iter().map(|t| mgr.encode_semantic_query(t)).collect()
            }
            EmbeddingSource::Provider(provider) => provider.encode_semantic(texts).await,
        }
    }

    /// Encode a batch of code texts
    async fn encode_code_batch(&self, texts: Vec<String>) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        match &self.embedding_source {
            EmbeddingSource::Legacy(mgr) => {
                // Legacy manager doesn't support batching, encode one at a time
                texts.iter().map(|t| mgr.encode_code_query(t)).collect()
            }
            EmbeddingSource::Provider(provider) => provider.encode_code(texts).await,
        }
    }

    /// Encode semantic and code texts, using parallelism when appropriate
    ///
    /// This is the most efficient way to generate embeddings for indexing:
    /// - Batches texts to reduce API round-trips
    /// - For remote providers (Azure ML, OpenAI): runs semantic and code encoding in parallel
    /// - For local provider: runs sequentially to avoid GPU command buffer conflicts
    async fn encode_batch_parallel(
        &self,
        semantic_texts: Vec<String>,
        code_texts: Vec<String>,
    ) -> Result<(Vec<Vec<f32>>, Vec<Vec<f32>>)> {
        // Check if we can use parallel execution
        // Local provider uses GPU which can't handle concurrent model execution
        let use_parallel = match &self.embedding_source {
            EmbeddingSource::Legacy(_) => false,
            EmbeddingSource::Provider(provider) => {
                provider.provider_type() != EmbeddingProviderType::Local
            }
        };

        if use_parallel {
            // Remote providers: run in parallel for maximum throughput
            let (semantic_result, code_result) = tokio::join!(
                self.encode_semantic_batch(semantic_texts),
                self.encode_code_batch(code_texts)
            );
            Ok((semantic_result?, code_result?))
        } else {
            // Local provider: run sequentially to avoid GPU conflicts
            let semantic_embeddings = self.encode_semantic_batch(semantic_texts).await?;
            let code_embeddings = self.encode_code_batch(code_texts).await?;
            Ok((semantic_embeddings, code_embeddings))
        }
    }

    /// Ensure collections exist and index the graph
    ///
    /// Uses batched parallel encoding for optimal performance with remote providers.
    pub async fn index_graph(&mut self, graph: &PetCodeGraph) -> Result<IndexStats> {
        info!("Starting graph indexing for repo '{}'", self.repo_id);

        // Ensure collections exist
        self.store.ensure_collections().await?;
        info!("Collections ensured");

        // Clear existing points for this repo (for clean reindex)
        self.store.delete_repo_points(collections::SEMANTIC).await?;
        self.store.delete_repo_points(collections::CODE).await?;
        info!("Cleared existing points for repo");

        // Create semantic text builder with access to full graph for context
        let semantic_builder = SemanticTextBuilder::new(graph);

        let mut stats = IndexStats::default();

        // Phase 1: Collect all nodes and their content
        // This separates I/O (file reading) from embedding generation
        struct NodeData {
            point_id: u64,
            semantic_text: String,
            code_text: String,
            payload: EntityPayload,
        }

        let mut pending_nodes: Vec<NodeData> = Vec::new();

        for node in graph.iter_nodes() {
            stats.total_processed += 1;

            // Skip file and repository nodes (they don't have meaningful content for search)
            if node.is_file() || node.is_repository() {
                stats.total_skipped += 1;
                continue;
            }

            // Read source code for the node
            let content = match self.read_node_content(node) {
                Ok(c) => c,
                Err(e) => {
                    debug!("Failed to read content for {}: {}", node.id, e);
                    stats.total_failed += 1;
                    continue;
                }
            };

            // Skip empty content
            if content.trim().is_empty() {
                stats.total_skipped += 1;
                continue;
            }

            // Create rich semantic text using graph traversal for context
            let semantic_text = semantic_builder.build(node, &content);

            // Create payload
            let payload = EntityPayload {
                repo_id: self.repo_id.clone(),
                entity_id: node.id.clone(),
                name: node.name.clone(),
                entity_type: node.node_type.as_str().to_string(),
                kind: node.kind.clone().unwrap_or_default(),
                subtype: node.subtype.clone().unwrap_or_default(),
                file_path: node.file.clone(),
                start_line: node.line as u32,
                end_line: node.end_line as u32,
            };

            let point_id = CodePoint::generate_id(&node.id, &self.repo_id);

            pending_nodes.push(NodeData {
                point_id,
                semantic_text,
                code_text: content,
                payload,
            });
        }

        info!(
            "Collected {} nodes for embedding (batch size: {})",
            pending_nodes.len(),
            self.embedding_batch_size
        );

        // Phase 2: Generate embeddings in batches with parallel semantic+code encoding
        let mut semantic_points = Vec::with_capacity(pending_nodes.len());
        let mut code_points = Vec::with_capacity(pending_nodes.len());

        for (batch_idx, batch) in pending_nodes.chunks(self.embedding_batch_size).enumerate() {
            let batch_start = batch_idx * self.embedding_batch_size;

            // Extract texts for this batch
            let semantic_texts: Vec<String> =
                batch.iter().map(|n| n.semantic_text.clone()).collect();
            let code_texts: Vec<String> = batch.iter().map(|n| n.code_text.clone()).collect();

            // Generate embeddings in parallel (semantic and code simultaneously)
            let (semantic_vecs, code_vecs) =
                match self.encode_batch_parallel(semantic_texts, code_texts).await {
                    Ok(vecs) => vecs,
                    Err(e) => {
                        // On batch failure, mark all nodes in batch as failed
                        debug!("Batch {} failed: {}", batch_idx, e);
                        stats.total_failed += batch.len();
                        continue;
                    }
                };

            // Verify we got the expected number of embeddings
            if semantic_vecs.len() != batch.len() || code_vecs.len() != batch.len() {
                debug!(
                    "Batch {} size mismatch: expected {}, got semantic={}, code={}",
                    batch_idx,
                    batch.len(),
                    semantic_vecs.len(),
                    code_vecs.len()
                );
                stats.total_failed += batch.len();
                continue;
            }

            // Build points from embeddings
            for (i, node_data) in batch.iter().enumerate() {
                semantic_points.push(CodePoint {
                    id: node_data.point_id,
                    vector: semantic_vecs[i].clone(),
                    payload: node_data.payload.clone(),
                    content: node_data.semantic_text.clone(),
                });

                code_points.push(CodePoint {
                    id: node_data.point_id,
                    vector: code_vecs[i].clone(),
                    payload: node_data.payload.clone(),
                    content: node_data.code_text.clone(),
                });

                stats.total_indexed += 1;
            }

            // Log progress
            let processed = batch_start + batch.len();
            if processed % 500 == 0 || processed == pending_nodes.len() {
                info!(
                    "Embedding progress: {}/{} ({:.1}%)",
                    processed,
                    pending_nodes.len(),
                    (processed as f64 / pending_nodes.len() as f64) * 100.0
                );
            }
        }

        // Phase 3: Upsert points in batches
        info!(
            "Upserting {} semantic points and {} code points",
            semantic_points.len(),
            code_points.len()
        );

        self.store
            .upsert_points_batched(
                collections::SEMANTIC,
                semantic_points.clone(),
                self.batch_size,
            )
            .await?;
        stats.semantic_indexed = semantic_points.len();

        self.store
            .upsert_points_batched(collections::CODE, code_points.clone(), self.batch_size)
            .await?;
        stats.code_indexed = code_points.len();

        info!(
            "Indexing complete: {} processed, {} indexed, {} skipped, {} failed",
            stats.total_processed, stats.total_indexed, stats.total_skipped, stats.total_failed
        );

        Ok(stats)
    }

    /// Index a batch of nodes and upsert immediately
    ///
    /// This is used for partition-by-partition indexing to avoid loading
    /// the entire graph into memory. Each partition's nodes are indexed
    /// and upserted before moving to the next partition.
    ///
    /// Unlike `index_graph`, this does NOT clear existing points first.
    /// Call `clear_repo_points()` before starting if doing a full reindex.
    ///
    /// The `graph` parameter is needed for SemanticTextBuilder context.
    ///
    /// Uses batched parallel encoding for optimal performance with remote providers.
    pub async fn index_nodes(
        &mut self,
        nodes: &[Node],
        graph: &PetCodeGraph,
    ) -> Result<IndexStats> {
        let semantic_builder = SemanticTextBuilder::new(graph);
        let mut stats = IndexStats::default();

        // Phase 1: Collect all valid nodes and their content
        struct NodeData {
            point_id: u64,
            semantic_text: String,
            code_text: String,
            payload: EntityPayload,
        }

        let mut pending_nodes: Vec<NodeData> = Vec::new();

        for node in nodes {
            stats.total_processed += 1;

            // Skip file and repository nodes
            if node.is_file() || node.is_repository() {
                stats.total_skipped += 1;
                continue;
            }

            // Read source code for the node
            let content = match self.read_node_content(node) {
                Ok(c) => c,
                Err(e) => {
                    debug!("Failed to read content for {}: {}", node.id, e);
                    stats.total_failed += 1;
                    continue;
                }
            };

            // Skip empty content
            if content.trim().is_empty() {
                stats.total_skipped += 1;
                continue;
            }

            // Create rich semantic text
            let semantic_text = semantic_builder.build(node, &content);

            // Create payload
            let payload = EntityPayload {
                repo_id: self.repo_id.clone(),
                entity_id: node.id.clone(),
                name: node.name.clone(),
                entity_type: node.node_type.as_str().to_string(),
                kind: node.kind.clone().unwrap_or_default(),
                subtype: node.subtype.clone().unwrap_or_default(),
                file_path: node.file.clone(),
                start_line: node.line as u32,
                end_line: node.end_line as u32,
            };

            let point_id = CodePoint::generate_id(&node.id, &self.repo_id);

            pending_nodes.push(NodeData {
                point_id,
                semantic_text,
                code_text: content,
                payload,
            });
        }

        if pending_nodes.is_empty() {
            return Ok(stats);
        }

        // Phase 2: Generate embeddings in batches with parallel semantic+code encoding
        let mut semantic_points = Vec::with_capacity(pending_nodes.len());
        let mut code_points = Vec::with_capacity(pending_nodes.len());

        for (batch_idx, batch) in pending_nodes.chunks(self.embedding_batch_size).enumerate() {
            // Extract texts for this batch
            let semantic_texts: Vec<String> =
                batch.iter().map(|n| n.semantic_text.clone()).collect();
            let code_texts: Vec<String> = batch.iter().map(|n| n.code_text.clone()).collect();

            // Generate embeddings in parallel
            let (semantic_vecs, code_vecs) =
                match self.encode_batch_parallel(semantic_texts, code_texts).await {
                    Ok(vecs) => vecs,
                    Err(e) => {
                        debug!("Batch {} failed: {}", batch_idx, e);
                        stats.total_failed += batch.len();
                        continue;
                    }
                };

            // Verify we got the expected number of embeddings
            if semantic_vecs.len() != batch.len() || code_vecs.len() != batch.len() {
                debug!(
                    "Batch {} size mismatch: expected {}, got semantic={}, code={}",
                    batch_idx,
                    batch.len(),
                    semantic_vecs.len(),
                    code_vecs.len()
                );
                stats.total_failed += batch.len();
                continue;
            }

            // Build points from embeddings
            for (i, node_data) in batch.iter().enumerate() {
                semantic_points.push(CodePoint {
                    id: node_data.point_id,
                    vector: semantic_vecs[i].clone(),
                    payload: node_data.payload.clone(),
                    content: node_data.semantic_text.clone(),
                });

                code_points.push(CodePoint {
                    id: node_data.point_id,
                    vector: code_vecs[i].clone(),
                    payload: node_data.payload.clone(),
                    content: node_data.code_text.clone(),
                });

                stats.total_indexed += 1;
            }
        }

        // Phase 3: Upsert immediately (don't accumulate across partitions)
        if !semantic_points.is_empty() {
            self.store
                .upsert_points_batched(
                    collections::SEMANTIC,
                    semantic_points.clone(),
                    self.batch_size,
                )
                .await?;
            stats.semantic_indexed = semantic_points.len();
        }

        if !code_points.is_empty() {
            self.store
                .upsert_points_batched(collections::CODE, code_points.clone(), self.batch_size)
                .await?;
            stats.code_indexed = code_points.len();
        }

        Ok(stats)
    }

    /// Clear all points for this repo from both collections
    ///
    /// Call this before partition-by-partition indexing for a full reindex.
    pub async fn clear_repo_points(&self) -> Result<()> {
        self.store.delete_repo_points(collections::SEMANTIC).await?;
        self.store.delete_repo_points(collections::CODE).await?;
        Ok(())
    }

    /// Read source code content for a node
    fn read_node_content(&self, node: &Node) -> std::io::Result<String> {
        let file_path = self.repo_path.join(&node.file);
        let content = std::fs::read_to_string(&file_path)?;

        let lines: Vec<&str> = content.lines().collect();
        let start = node.line.saturating_sub(1);
        let end = node.end_line.min(lines.len());

        if start >= lines.len() {
            return Ok(String::new());
        }

        let selected: Vec<&str> = lines[start..end].to_vec();
        Ok(selected.join("\n"))
    }

    /// Check if collections exist and have data for this repo
    pub async fn needs_indexing(&self) -> Result<bool> {
        // Check if semantic collection exists
        if !self.store.collection_exists(collections::SEMANTIC).await? {
            return Ok(true);
        }

        // Check if code collection exists
        if !self.store.collection_exists(collections::CODE).await? {
            return Ok(true);
        }

        // Check if there are any points for this repo
        // We'd need to add a count method to QdrantStore for this
        // For now, assume if collections exist, we're good
        Ok(false)
    }

    /// Incrementally index only changed files
    ///
    /// This is more efficient than `index_graph` when only a few files have changed.
    /// It deletes points for modified/deleted files, then indexes nodes from
    /// modified/added files.
    pub async fn index_changes(
        &mut self,
        graph: &PetCodeGraph,
        changes: &codeprysm_core::merkle::ChangeSet,
    ) -> Result<IndexStats> {
        use std::collections::HashSet;

        info!(
            "Starting incremental indexing: {} added, {} modified, {} deleted",
            changes.added.len(),
            changes.modified.len(),
            changes.deleted.len()
        );

        // Ensure collections exist
        self.store.ensure_collections().await?;

        // 1. Delete points for deleted files
        for file_path in &changes.deleted {
            debug!("Deleting points for deleted file: {}", file_path);
            self.store
                .delete_points_by_file(collections::SEMANTIC, file_path)
                .await?;
            self.store
                .delete_points_by_file(collections::CODE, file_path)
                .await?;
        }

        // 2. Delete points for modified files (will be re-indexed)
        for file_path in &changes.modified {
            debug!("Deleting points for modified file: {}", file_path);
            self.store
                .delete_points_by_file(collections::SEMANTIC, file_path)
                .await?;
            self.store
                .delete_points_by_file(collections::CODE, file_path)
                .await?;
        }

        // 3. Build set of files to index (added + modified)
        let files_to_index: HashSet<&str> = changes
            .added
            .iter()
            .chain(changes.modified.iter())
            .map(|s| s.as_str())
            .collect();

        if files_to_index.is_empty() {
            info!("No files to index (only deletions)");
            return Ok(IndexStats::default());
        }

        // 4. Index nodes from affected files using batched parallel encoding
        let mut stats = IndexStats::default();

        // Create semantic text builder with access to full graph for context
        let semantic_builder = SemanticTextBuilder::new(graph);

        // Phase 1: Collect all valid nodes and their content
        struct NodeData {
            point_id: u64,
            semantic_text: String,
            code_text: String,
            payload: EntityPayload,
        }

        let mut pending_nodes: Vec<NodeData> = Vec::new();

        for node in graph.iter_nodes() {
            // Only process nodes from changed files
            if !files_to_index.contains(node.file.as_str()) {
                continue;
            }

            stats.total_processed += 1;

            // Skip file and repository nodes
            if node.is_file() || node.is_repository() {
                stats.total_skipped += 1;
                continue;
            }

            // Read source code for the node
            let content = match self.read_node_content(node) {
                Ok(c) => c,
                Err(e) => {
                    debug!("Failed to read content for {}: {}", node.id, e);
                    stats.total_failed += 1;
                    continue;
                }
            };

            // Skip empty content
            if content.trim().is_empty() {
                stats.total_skipped += 1;
                continue;
            }

            // Create rich semantic text using graph traversal for context
            let semantic_text = semantic_builder.build(node, &content);

            // Create payload
            let payload = EntityPayload {
                repo_id: self.repo_id.clone(),
                entity_id: node.id.clone(),
                name: node.name.clone(),
                entity_type: node.node_type.as_str().to_string(),
                kind: node.kind.clone().unwrap_or_default(),
                subtype: node.subtype.clone().unwrap_or_default(),
                file_path: node.file.clone(),
                start_line: node.line as u32,
                end_line: node.end_line as u32,
            };

            let point_id = CodePoint::generate_id(&node.id, &self.repo_id);

            pending_nodes.push(NodeData {
                point_id,
                semantic_text,
                code_text: content,
                payload,
            });
        }

        if pending_nodes.is_empty() {
            info!("No nodes to index from changed files");
            return Ok(stats);
        }

        // Phase 2: Generate embeddings in batches with parallel semantic+code encoding
        let mut semantic_points = Vec::with_capacity(pending_nodes.len());
        let mut code_points = Vec::with_capacity(pending_nodes.len());

        for (batch_idx, batch) in pending_nodes.chunks(self.embedding_batch_size).enumerate() {
            // Extract texts for this batch
            let semantic_texts: Vec<String> =
                batch.iter().map(|n| n.semantic_text.clone()).collect();
            let code_texts: Vec<String> = batch.iter().map(|n| n.code_text.clone()).collect();

            // Generate embeddings in parallel
            let (semantic_vecs, code_vecs) =
                match self.encode_batch_parallel(semantic_texts, code_texts).await {
                    Ok(vecs) => vecs,
                    Err(e) => {
                        debug!("Batch {} failed: {}", batch_idx, e);
                        stats.total_failed += batch.len();
                        continue;
                    }
                };

            // Verify we got the expected number of embeddings
            if semantic_vecs.len() != batch.len() || code_vecs.len() != batch.len() {
                debug!(
                    "Batch {} size mismatch: expected {}, got semantic={}, code={}",
                    batch_idx,
                    batch.len(),
                    semantic_vecs.len(),
                    code_vecs.len()
                );
                stats.total_failed += batch.len();
                continue;
            }

            // Build points from embeddings
            for (i, node_data) in batch.iter().enumerate() {
                semantic_points.push(CodePoint {
                    id: node_data.point_id,
                    vector: semantic_vecs[i].clone(),
                    payload: node_data.payload.clone(),
                    content: node_data.semantic_text.clone(),
                });

                code_points.push(CodePoint {
                    id: node_data.point_id,
                    vector: code_vecs[i].clone(),
                    payload: node_data.payload.clone(),
                    content: node_data.code_text.clone(),
                });

                stats.total_indexed += 1;
            }
        }

        // Phase 3: Upsert points in batches
        if !semantic_points.is_empty() {
            info!(
                "Upserting {} semantic points and {} code points",
                semantic_points.len(),
                code_points.len()
            );

            self.store
                .upsert_points_batched(
                    collections::SEMANTIC,
                    semantic_points.clone(),
                    self.batch_size,
                )
                .await?;
            stats.semantic_indexed = semantic_points.len();

            self.store
                .upsert_points_batched(collections::CODE, code_points.clone(), self.batch_size)
                .await?;
            stats.code_indexed = code_points.len();
        }

        info!(
            "Incremental indexing complete: {} processed, {} indexed, {} skipped, {} failed",
            stats.total_processed, stats.total_indexed, stats.total_skipped, stats.total_failed
        );

        Ok(stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_stats_default() {
        let stats = IndexStats::default();
        assert_eq!(stats.total_processed, 0);
        assert_eq!(stats.total_indexed, 0);
    }
}
