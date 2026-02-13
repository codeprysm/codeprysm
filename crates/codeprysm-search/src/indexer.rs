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

use codeprysm_core::lazy::manager::LazyGraphManager;
use codeprysm_core::{Node, PetCodeGraph};
use tracing::{debug, info, warn};

use crate::graph_context::GraphContext;

use crate::client::{QdrantConfig, QdrantStore};
use crate::embeddings::{EmbeddingConfig, EmbeddingProvider, EmbeddingProviderType};
use crate::error::Result;
use crate::schema::{collections, CodePoint, EntityPayload};
use crate::semantic_text::{SemanticTextBuilder, SemanticTextConfig};
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
    /// Number of partitions processed (streaming mode only)
    pub partitions_processed: usize,
    /// Average nodes per partition (streaming mode only)
    pub nodes_per_partition_avg: f64,
    /// Estimated peak memory usage in bytes (based on 2KB/node heuristic)
    pub estimated_peak_memory_bytes: usize,
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

    /// Ensure collections exist and index the graph.
    ///
    /// This method loads all nodes into memory before processing, which provides
    /// optimal performance for small to medium repositories.
    ///
    /// # Memory Usage
    ///
    /// **Warning**: For large repositories (>10,000 nodes), this method may consume
    /// significant memory (potentially 50GB+ for 50K nodes). Consider using
    /// [`index_graph_lazy()`](Self::index_graph_lazy) with a `LazyGraphManager` instead,
    /// which processes partition-by-partition with bounded memory usage.
    ///
    /// # When to Use
    ///
    /// - Repositories with <10,000 nodes
    /// - Environments with ample RAM
    /// - When maximum indexing speed is required
    ///
    /// For large repositories or memory-constrained environments, use
    /// `index_graph_lazy()` instead.
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
                        warn!("Batch {} failed to encode embeddings: {}", batch_idx, e);
                        stats.total_failed += batch.len();
                        continue;
                    }
                };

            // Verify we got the expected number of embeddings
            if semantic_vecs.len() != batch.len() || code_vecs.len() != batch.len() {
                warn!(
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

    /// Index a graph using streaming partition-by-partition approach.
    ///
    /// This method provides **memory-bounded indexing** for large repositories.
    /// Unlike `index_graph()` which loads all nodes into memory before processing,
    /// this method:
    ///
    /// 1. Iterates partitions one at a time using `LazyGraphManager`
    /// 2. Loads nodes only for the current partition
    /// 3. Generates embeddings in batches
    /// 4. Upserts to Qdrant immediately
    /// 5. Releases partition memory before moving to the next
    ///
    /// # Memory Bounds
    ///
    /// Memory usage is bounded by:
    /// - Single partition's nodes (~1-5MB typically)
    /// - One embedding batch worth of vectors (~embedding_batch_size × 768 × 4 bytes)
    /// - Qdrant upsert batch (~batch_size points)
    ///
    /// A 50K-node repo and a 500K-node repo will have similar peak memory usage.
    ///
    /// # When to Use
    ///
    /// - Repositories with >10,000 nodes
    /// - Memory-constrained environments
    /// - When the default `index_graph()` causes memory pressure
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use codeprysm_core::lazy::manager::LazyGraphManager;
    ///
    /// let manager = LazyGraphManager::open(&prism_dir)?;
    /// let stats = indexer.index_graph_lazy(&manager).await?;
    /// ```
    pub async fn index_graph_lazy(&mut self, manager: &LazyGraphManager) -> Result<IndexStats> {
        info!(repo_id = %self.repo_id, "Starting streaming graph indexing");

        // Ensure collections exist
        self.store.ensure_collections().await?;
        info!("Collections ensured");

        // Clear existing points for this repo (for clean reindex)
        self.store.delete_repo_points(collections::SEMANTIC).await?;
        self.store.delete_repo_points(collections::CODE).await?;
        info!("Cleared existing points for repo");

        // Get total count for progress reporting (without loading nodes)
        let total_nodes = manager.get_total_indexable_node_count().map_err(|e| {
            crate::error::SearchError::Graph(format!("Failed to count nodes: {}", e))
        })?;
        let total_partitions = manager.partition_count();

        info!(
            "Found {} indexable nodes across {} partitions",
            total_nodes, total_partitions
        );

        let mut stats = IndexStats::default();

        // Iterate partitions manually since we need async operations
        let partition_ids: Vec<String> = manager.manifest().partitions.keys().cloned().collect();

        // Track partition statistics for memory estimation
        let mut max_partition_nodes = 0usize;
        let mut total_partition_nodes = 0usize;
        let mut partitions_with_nodes = 0usize;

        // Estimated memory per node (content + semantic text + embeddings buffer)
        // Based on empirical observation: ~2KB per node average
        const BYTES_PER_NODE_ESTIMATE: usize = 2048;

        for (idx, partition_id) in partition_ids.iter().enumerate() {
            tracing::debug!(
                partition = %partition_id,
                index = idx,
                total = partition_ids.len(),
                "Starting partition indexing"
            );

            // Load partition nodes directly from database
            let db_path = manager.manifest().get_partition_file(partition_id);
            if db_path.is_none() {
                tracing::warn!("Partition {} has no database file", partition_id);
                continue;
            }

            // Use the manager to get partition nodes
            // We need to load the partition to access its nodes
            if let Err(e) = manager.load_partition(partition_id) {
                tracing::warn!("Failed to load partition {}: {}", partition_id, e);
                continue;
            }

            // Get nodes from this partition
            let node_ids = match manager.node_ids_in_partition(partition_id) {
                Some(ids) => ids,
                None => {
                    manager.unload_partition(partition_id);
                    continue;
                }
            };

            // Get the actual nodes
            let nodes: Vec<Node> = node_ids
                .iter()
                .filter_map(|id| manager.get_node_if_loaded(id))
                .filter(|n| !n.is_file() && !n.is_repository())
                .collect();

            if nodes.is_empty() {
                manager.unload_partition(partition_id);
                continue;
            }

            let partition_node_count = nodes.len();
            max_partition_nodes = max_partition_nodes.max(partition_node_count);
            total_partition_nodes += partition_node_count;
            partitions_with_nodes += 1;

            // Estimate memory for this partition
            let estimated_partition_mb =
                (partition_node_count * BYTES_PER_NODE_ESTIMATE) as f64 / (1024.0 * 1024.0);

            info!(
                "Processing partition {}/{}: {} ({} nodes, ~{:.1}MB estimated)",
                idx + 1,
                partition_ids.len(),
                partition_id,
                partition_node_count,
                estimated_partition_mb
            );

            // Index this partition's nodes using the LazyGraphManager for context
            let partition_stats = self.index_nodes_with_context(&nodes, manager).await?;

            // Accumulate stats
            stats.total_processed += partition_stats.total_processed;
            stats.total_indexed += partition_stats.total_indexed;
            stats.total_skipped += partition_stats.total_skipped;
            stats.total_failed += partition_stats.total_failed;
            stats.semantic_indexed += partition_stats.semantic_indexed;
            stats.code_indexed += partition_stats.code_indexed;

            // Unload partition to free memory
            manager.unload_partition(partition_id);

            // Log progress with memory estimate
            let progress_pct = if total_nodes > 0 {
                (stats.total_indexed as f64 / total_nodes as f64) * 100.0
            } else {
                0.0
            };

            info!(
                "Progress: {}/{} partitions, {}/{} nodes indexed ({:.1}%)",
                idx + 1,
                partition_ids.len(),
                stats.total_indexed,
                total_nodes,
                progress_pct
            );
        }

        // Compute final partition statistics
        stats.partitions_processed = partitions_with_nodes;
        stats.nodes_per_partition_avg = if partitions_with_nodes > 0 {
            total_partition_nodes as f64 / partitions_with_nodes as f64
        } else {
            0.0
        };
        // Peak memory = largest partition + embedding batch buffer
        let embedding_batch_buffer = self.embedding_batch_size * 4096; // ~4KB per embedding
        stats.estimated_peak_memory_bytes =
            (max_partition_nodes * BYTES_PER_NODE_ESTIMATE) + embedding_batch_buffer;

        let peak_mb = stats.estimated_peak_memory_bytes as f64 / (1024.0 * 1024.0);
        info!(
            "Streaming indexing complete: {} processed, {} indexed, {} skipped, {} failed",
            stats.total_processed, stats.total_indexed, stats.total_skipped, stats.total_failed
        );
        info!(
            "Partition stats: {} partitions, {:.1} nodes/partition avg, ~{:.1}MB estimated peak memory",
            stats.partitions_processed, stats.nodes_per_partition_avg, peak_mb
        );

        Ok(stats)
    }

    /// Index a graph with checkpoint/resume support.
    ///
    /// This method extends [`index_graph_lazy()`](Self::index_graph_lazy) with:
    /// - Checkpoint file read/write for resume capability
    /// - Conditional clearing of Qdrant points (skip if resuming)
    /// - Skip completed partitions
    /// - Re-index interrupted partition
    /// - Progress reporting with resume context
    ///
    /// # Resume Behavior
    ///
    /// - If checkpoint exists and manifest unchanged: resumes from last completed partition
    /// - If checkpoint exists but manifest changed: starts fresh (checkpoint invalidated)
    /// - If checkpoint exists but Qdrant URL changed: starts fresh (checkpoint invalidated)
    /// - If `force=true`: ignores checkpoint and starts fresh
    ///
    /// # Checkpoint Location
    ///
    /// Checkpoint is stored at `{prism_dir}/index_checkpoint.json` alongside manifest.json.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use codeprysm_core::lazy::manager::LazyGraphManager;
    ///
    /// let manager = LazyGraphManager::open(&prism_dir)?;
    /// let stats = indexer.index_graph_lazy_resumable(&manager, &prism_dir, false).await?;
    /// ```
    pub async fn index_graph_lazy_resumable(
        &mut self,
        manager: &LazyGraphManager,
        prism_dir: &Path,
        force: bool,
    ) -> Result<IndexStats> {
        use crate::checkpoint::{compute_manifest_hash, IndexCheckpoint, ResumeValidation};

        let checkpoint_path = prism_dir.join("index_checkpoint.json");
        let manifest_path = prism_dir.join("manifest.json");

        // Compute manifest hash for validation
        let manifest_hash = compute_manifest_hash(&manifest_path)?;
        let qdrant_url = self.store.url().to_string();

        // Try to load existing checkpoint (unless force is set)
        let existing_checkpoint = if !force {
            match IndexCheckpoint::load(&checkpoint_path)? {
                Some(cp) => match cp.is_resumable(&manifest_hash, &qdrant_url) {
                    ResumeValidation::Valid => {
                        info!(
                            "Resuming from checkpoint: {}/{} partitions completed",
                            cp.completed_partitions.len(),
                            manager.partition_count()
                        );
                        Some(cp)
                    }
                    ResumeValidation::ManifestChanged { .. } => {
                        info!("Manifest changed since checkpoint, starting fresh");
                        None
                    }
                    ResumeValidation::QdrantUrlMismatch { old_url, new_url } => {
                        info!(
                            "Qdrant URL changed ({} -> {}), starting fresh",
                            old_url, new_url
                        );
                        None
                    }
                    ResumeValidation::RepoMismatch { old_repo, new_repo } => {
                        info!(
                            "Repository changed ({} -> {}), starting fresh",
                            old_repo, new_repo
                        );
                        None
                    }
                    ResumeValidation::AlreadyCompleted => {
                        info!("Previous indexing completed, starting fresh");
                        None
                    }
                    ResumeValidation::PreviousFailed { error } => {
                        info!(
                            "Previous indexing failed ({}), resuming from last completed partition",
                            error
                        );
                        Some(cp)
                    }
                },
                None => None,
            }
        } else {
            info!("Force flag set, ignoring any existing checkpoint");
            None
        };

        let is_resuming = existing_checkpoint.is_some();

        // Create new checkpoint or use existing one
        let mut checkpoint = existing_checkpoint.unwrap_or_else(|| {
            IndexCheckpoint::new(self.repo_id.clone(), manifest_hash, qdrant_url)
        });

        info!(repo_id = %self.repo_id, is_resuming = is_resuming, "Starting streaming graph indexing");

        // Ensure collections exist
        self.store.ensure_collections().await?;
        info!("Collections ensured");

        // CONDITIONAL: Only clear points if NOT resuming
        if !is_resuming {
            info!("Starting fresh indexing, clearing existing points");
            self.store.delete_repo_points(collections::SEMANTIC).await?;
            self.store.delete_repo_points(collections::CODE).await?;
            info!("Cleared existing points for repo");
        } else {
            info!(
                "Resuming indexing, keeping {} existing partitions' points",
                checkpoint.completed_partitions.len()
            );
        }

        // Get total count for progress reporting (without loading nodes)
        let total_nodes = manager.get_total_indexable_node_count().map_err(|e| {
            crate::error::SearchError::Graph(format!("Failed to count nodes: {}", e))
        })?;
        let total_partitions = manager.partition_count();

        info!(
            "Found {} indexable nodes across {} partitions",
            total_nodes, total_partitions
        );

        let mut stats = IndexStats::default();

        // Iterate partitions manually since we need async operations
        let partition_ids: Vec<String> = manager.manifest().partitions.keys().cloned().collect();

        // Build list of partitions to process (skip completed ones)
        let partitions_to_process: Vec<&str> = partition_ids
            .iter()
            .filter(|id| !checkpoint.is_partition_completed(id))
            .map(|s| s.as_str())
            .collect();

        let skipped_count = partition_ids.len() - partitions_to_process.len();
        if skipped_count > 0 {
            info!(
                "Skipping {} already-completed partitions, processing {}",
                skipped_count,
                partitions_to_process.len()
            );
        }

        // Save initial checkpoint
        checkpoint.save(&checkpoint_path)?;

        // Track partition statistics for memory estimation
        let mut max_partition_nodes = 0usize;
        let mut total_partition_nodes = 0usize;
        let mut partitions_with_nodes = 0usize;

        // Estimated memory per node (content + semantic text + embeddings buffer)
        // Based on empirical observation: ~2KB per node average
        const BYTES_PER_NODE_ESTIMATE: usize = 2048;

        for (idx, partition_id) in partitions_to_process.iter().enumerate() {
            tracing::debug!(
                partition = %partition_id,
                index = idx,
                total = partitions_to_process.len(),
                "Starting partition indexing"
            );

            // Mark partition as starting (for crash recovery)
            checkpoint.start_partition(partition_id);
            checkpoint.save(&checkpoint_path)?;

            // Load partition nodes directly from database
            let db_path = manager.manifest().get_partition_file(partition_id);
            if db_path.is_none() {
                tracing::warn!("Partition {} has no database file", partition_id);
                continue;
            }

            // Use the manager to get partition nodes
            // We need to load the partition to access its nodes
            if let Err(e) = manager.load_partition(partition_id) {
                tracing::warn!("Failed to load partition {}: {}", partition_id, e);
                continue;
            }

            // Get nodes from this partition
            let node_ids = match manager.node_ids_in_partition(partition_id) {
                Some(ids) => ids,
                None => {
                    manager.unload_partition(partition_id);
                    continue;
                }
            };

            // Get the actual nodes
            let nodes: Vec<Node> = node_ids
                .iter()
                .filter_map(|id| manager.get_node_if_loaded(id))
                .filter(|n| !n.is_file() && !n.is_repository())
                .collect();

            if nodes.is_empty() {
                manager.unload_partition(partition_id);
                // Mark as completed even if empty (still processed successfully)
                checkpoint.complete_partition(partition_id, &IndexStats::default());
                checkpoint.save(&checkpoint_path)?;
                continue;
            }

            let partition_node_count = nodes.len();
            max_partition_nodes = max_partition_nodes.max(partition_node_count);
            total_partition_nodes += partition_node_count;
            partitions_with_nodes += 1;

            // Estimate memory for this partition
            let estimated_partition_mb =
                (partition_node_count * BYTES_PER_NODE_ESTIMATE) as f64 / (1024.0 * 1024.0);

            info!(
                "Processing partition {}/{}: {} ({} nodes, ~{:.1}MB estimated)",
                idx + 1,
                partitions_to_process.len(),
                partition_id,
                partition_node_count,
                estimated_partition_mb
            );

            // Index this partition's nodes using the LazyGraphManager for context
            let partition_stats = self.index_nodes_with_context(&nodes, manager).await?;

            // Accumulate stats
            stats.total_processed += partition_stats.total_processed;
            stats.total_indexed += partition_stats.total_indexed;
            stats.total_skipped += partition_stats.total_skipped;
            stats.total_failed += partition_stats.total_failed;
            stats.semantic_indexed += partition_stats.semantic_indexed;
            stats.code_indexed += partition_stats.code_indexed;

            // Unload partition to free memory
            manager.unload_partition(partition_id);

            // Mark partition as completed and save checkpoint
            checkpoint.complete_partition(partition_id, &partition_stats);
            checkpoint.save(&checkpoint_path)?;

            // Log progress with memory estimate
            let total_completed = checkpoint.completed_count();
            let progress_pct = (total_completed as f64 / total_partitions as f64) * 100.0;

            info!(
                "Progress: {}/{} partitions ({:.1}%), {} nodes indexed",
                total_completed, total_partitions, progress_pct, stats.total_indexed
            );
        }

        // Compute final partition statistics
        stats.partitions_processed = partitions_with_nodes + skipped_count;
        stats.nodes_per_partition_avg = if partitions_with_nodes > 0 {
            total_partition_nodes as f64 / partitions_with_nodes as f64
        } else {
            0.0
        };
        // Peak memory = largest partition + embedding batch buffer
        let embedding_batch_buffer = self.embedding_batch_size * 4096; // ~4KB per embedding
        stats.estimated_peak_memory_bytes =
            (max_partition_nodes * BYTES_PER_NODE_ESTIMATE) + embedding_batch_buffer;

        // Add stats from resumed partitions
        stats.total_indexed += checkpoint.stats.total_indexed;
        stats.total_processed += checkpoint.stats.total_processed;
        stats.total_skipped += checkpoint.stats.total_skipped;
        stats.total_failed += checkpoint.stats.total_failed;
        stats.semantic_indexed += checkpoint.stats.semantic_indexed;
        stats.code_indexed += checkpoint.stats.code_indexed;

        let peak_mb = stats.estimated_peak_memory_bytes as f64 / (1024.0 * 1024.0);
        info!(
            "Streaming indexing complete: {} processed, {} indexed, {} skipped, {} failed",
            stats.total_processed, stats.total_indexed, stats.total_skipped, stats.total_failed
        );
        info!(
            "Partition stats: {} partitions, {:.1} nodes/partition avg, ~{:.1}MB estimated peak memory",
            stats.partitions_processed, stats.nodes_per_partition_avg, peak_mb
        );

        // Mark completed and delete checkpoint
        checkpoint.mark_completed();
        IndexCheckpoint::delete(&checkpoint_path)?;

        Ok(stats)
    }

    /// Index nodes using a generic graph context for semantic text building.
    ///
    /// This is the core indexing logic extracted to work with any GraphContext
    /// implementation (PetCodeGraph or LazyGraphManager).
    ///
    /// Uses `SemanticTextConfig::streaming()` to limit cross-partition context
    /// lookups and bound memory usage in streaming mode.
    async fn index_nodes_with_context<G>(&mut self, nodes: &[Node], graph: G) -> Result<IndexStats>
    where
        G: GraphContext,
    {
        // Use streaming config to limit cross-partition lookups
        let semantic_builder =
            SemanticTextBuilder::new_with_config(graph, SemanticTextConfig::streaming());
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

        // Phase 2: Generate embeddings in batches
        let mut semantic_points = Vec::with_capacity(pending_nodes.len());
        let mut code_points = Vec::with_capacity(pending_nodes.len());

        for (batch_idx, batch) in pending_nodes.chunks(self.embedding_batch_size).enumerate() {
            // Extract texts for this batch
            let semantic_texts: Vec<String> =
                batch.iter().map(|n| n.semantic_text.clone()).collect();
            let code_texts: Vec<String> = batch.iter().map(|n| n.code_text.clone()).collect();

            // Generate embeddings
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
    ///
    /// # Deprecation
    ///
    /// For new code, prefer using [`index_graph_lazy()`](Self::index_graph_lazy) with
    /// `LazyGraphManager` for memory-bounded indexing. This method requires holding
    /// the full graph in memory, which can cause memory exhaustion for large repos.
    #[deprecated(
        since = "0.2.0",
        note = "Use index_graph_lazy() with LazyGraphManager for memory-bounded indexing"
    )]
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

    /// Incrementally index only changed files using streaming partition-by-partition approach.
    ///
    /// This is a memory-bounded alternative to [`index_changes()`](Self::index_changes) that
    /// processes changed files by partition, loading and unloading partitions as needed.
    ///
    /// # Memory Bounds
    ///
    /// Memory usage is bounded by:
    /// - Single partition's nodes for changed files
    /// - One embedding batch worth of vectors
    /// - Qdrant upsert batch
    ///
    /// # Algorithm
    ///
    /// 1. Delete points for deleted files
    /// 2. Delete points for modified files (will be re-indexed)
    /// 3. Group added+modified files by partition ID using manifest
    /// 4. For each affected partition:
    ///    - Load partition
    ///    - Find nodes in changed files
    ///    - Index nodes using streaming config
    ///    - Upsert to Qdrant
    ///    - Unload partition
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// use codeprysm_core::lazy::manager::LazyGraphManager;
    /// use codeprysm_core::merkle::ChangeSet;
    ///
    /// let manager = LazyGraphManager::open(&prism_dir)?;
    /// let changes = ChangeSet { added: vec!["new.rs".into()], ..Default::default() };
    /// let stats = indexer.index_changes_lazy(&manager, &changes).await?;
    /// ```
    pub async fn index_changes_lazy(
        &mut self,
        manager: &LazyGraphManager,
        changes: &codeprysm_core::merkle::ChangeSet,
    ) -> Result<IndexStats> {
        use std::collections::{HashMap, HashSet};

        info!(
            "Starting streaming incremental indexing: {} added, {} modified, {} deleted",
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

        // 3. Group files to index by partition ID
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

        // Group files by partition
        let manifest = manager.manifest();
        let mut files_by_partition: HashMap<String, Vec<&str>> = HashMap::new();

        for file in &files_to_index {
            if let Some(partition_id) = manifest.get_partition_for_file(file) {
                files_by_partition
                    .entry(partition_id.to_string())
                    .or_default()
                    .push(file);
            } else {
                debug!("File {} not found in manifest, skipping", file);
            }
        }

        info!(
            "Found {} files across {} partitions",
            files_to_index.len(),
            files_by_partition.len()
        );

        let mut stats = IndexStats::default();

        // 4. Process each partition
        for (partition_id, partition_files) in files_by_partition {
            debug!(
                "Processing partition {} ({} files)",
                partition_id,
                partition_files.len()
            );

            // Load partition
            if let Err(e) = manager.load_partition(&partition_id) {
                tracing::warn!("Failed to load partition {}: {}", partition_id, e);
                continue;
            }

            // Get node IDs in this partition
            let node_ids = match manager.node_ids_in_partition(&partition_id) {
                Some(ids) => ids,
                None => {
                    manager.unload_partition(&partition_id);
                    continue;
                }
            };

            // Create set of files to index in this partition
            let partition_files_set: HashSet<&str> = partition_files.into_iter().collect();

            // Get nodes that belong to the changed files
            let nodes: Vec<Node> = node_ids
                .iter()
                .filter_map(|id| manager.get_node_if_loaded(id))
                .filter(|n| {
                    !n.is_file()
                        && !n.is_repository()
                        && partition_files_set.contains(n.file.as_str())
                })
                .collect();

            if !nodes.is_empty() {
                // Index nodes using streaming config
                let partition_stats = self.index_nodes_with_context(&nodes, manager).await?;

                // Accumulate stats
                stats.total_processed += partition_stats.total_processed;
                stats.total_indexed += partition_stats.total_indexed;
                stats.total_skipped += partition_stats.total_skipped;
                stats.total_failed += partition_stats.total_failed;
                stats.semantic_indexed += partition_stats.semantic_indexed;
                stats.code_indexed += partition_stats.code_indexed;
            }

            // Unload partition to free memory
            manager.unload_partition(&partition_id);
        }

        info!(
            "Streaming incremental indexing complete: {} processed, {} indexed, {} skipped, {} failed",
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
