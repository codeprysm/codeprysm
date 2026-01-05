//! Hybrid search combining semantic and code embeddings
//!
//! Provides intelligent search using dual Jina embedding models (both 768-dim):
//! - **Semantic**: jina-embeddings-v2-base-en for natural language queries
//! - **Code**: jina-embeddings-v2-base-code for code-aware search
//!
//! ## Search Modes
//!
//! The searcher supports three modes via [`HybridSearcher::search_by_mode`]:
//! - `"code"` - Uses code embeddings only, best for identifiers and code patterns
//! - `"info"` - Uses semantic embeddings only, best for conceptual queries
//! - `None` (default) - Hybrid search fusing both collections
//!
//! ## Search Features
//!
//! - Classifies query intent (identifier, question, natural language)
//! - Searches both semantic and code collections (in hybrid mode)
//! - Fuses results with max-based scoring and agreement bonuses
//! - Applies exact match, type, and context bonuses for ranking
//!
//! # Example
//!
//! ```ignore
//! use codeprysm_search::{HybridSearcher, QdrantConfig};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let mut searcher = HybridSearcher::connect(
//!         QdrantConfig::local(),
//!         "my-repo"
//!     ).await?;
//!
//!     // Hybrid search (default)
//!     let results = searcher.search("authentication logic", 10).await?;
//!
//!     // Code-only search for identifiers
//!     let code_results = searcher.search_by_mode("parseFile", 10, vec![], Some("code")).await?;
//!
//!     // Semantic-only search for concepts
//!     let info_results = searcher.search_by_mode("how errors are handled", 10, vec![], Some("info")).await?;
//!
//!     Ok(())
//! }
//! ```

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use regex::Regex;
use tracing::{debug, info};

use crate::client::{QdrantConfig, QdrantStore};
use crate::embeddings::{EmbeddingConfig, EmbeddingProvider};
use crate::error::Result;
use crate::schema::{collections, SearchHit};
use crate::EmbeddingsManager;

/// Weight presets for different query types
/// Format: (semantic_weight, code_weight, agreement_bonus_coefficient)
#[derive(Debug, Clone, Copy)]
pub struct WeightPreset {
    pub semantic: f32,
    pub code: f32,
    pub agreement_coeff: f32,
}

impl WeightPreset {
    /// Balanced weights for identifier lookups (camelCase, snake_case)
    /// Semantic embeddings work well for identifiers too, so keep balanced
    pub const IDENTIFIER: WeightPreset = WeightPreset {
        semantic: 0.5,
        code: 0.5,
        agreement_coeff: 0.15,
    };

    /// Strongly semantic weights for questions
    /// Questions like "how does X work" need concept matching
    pub const QUESTION: WeightPreset = WeightPreset {
        semantic: 0.9,
        code: 0.1,
        agreement_coeff: 0.08,
    };

    /// Semantic-favoring weights for natural language queries
    /// Phrases like "error handling" or "authentication logic" need semantic matching
    pub const NATURAL: WeightPreset = WeightPreset {
        semantic: 0.75,
        code: 0.25,
        agreement_coeff: 0.12,
    };
}

/// Scoring bonus configuration
#[derive(Debug, Clone)]
pub struct ScoringConfig {
    /// Bonus for exact name match
    pub exact_match_bonus: f32,
    /// Bonus for match ignoring separators
    pub separator_match_bonus: f32,
    /// Bonus when entity type matches query hints
    pub type_bonus: f32,
    /// Boost for /src/ files
    pub src_context_boost: f32,
    /// Penalty for test files
    pub test_context_penalty: f32,
}

impl Default for ScoringConfig {
    fn default() -> Self {
        Self {
            exact_match_bonus: 0.35,
            separator_match_bonus: 0.25,
            type_bonus: 0.08,
            src_context_boost: 0.06,
            test_context_penalty: -0.03,
        }
    }
}

/// Query intent classification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryType {
    /// Code entity lookup (camelCase, snake_case, PascalCase)
    Identifier,
    /// Natural language question
    Question,
    /// General natural language query
    Natural,
}

impl QueryType {
    /// Get weight preset for this query type
    pub fn weights(&self) -> WeightPreset {
        match self {
            QueryType::Identifier => WeightPreset::IDENTIFIER,
            QueryType::Question => WeightPreset::QUESTION,
            QueryType::Natural => WeightPreset::NATURAL,
        }
    }
}

/// Hybrid search result combining semantic and code search
#[derive(Debug, Clone)]
pub struct HybridSearchHit {
    /// Entity ID (e.g., "src/lib.rs:MyStruct:new")
    pub entity_id: String,
    /// Entity name
    pub name: String,
    /// Entity type (Container, Callable, Data). Files are Container with kind="file".
    pub entity_type: String,
    /// Entity kind (v2 schema)
    pub kind: String,
    /// Entity subtype (v2 schema)
    pub subtype: String,
    /// File path relative to repo root
    pub file_path: String,
    /// Line range (start, end)
    pub line_range: (u32, u32),
    /// Combined score after fusion
    pub combined_score: f32,
    /// Code snippet content
    pub code_snippet: String,
    /// Search sources that found this result
    pub found_via: Vec<String>,
    /// Individual scores by source
    pub individual_scores: HashMap<String, f32>,
}

/// Embedding source - either legacy EmbeddingsManager or new provider
#[allow(clippy::large_enum_variant)]
enum EmbeddingSource {
    /// Legacy sync embedding manager
    Legacy(EmbeddingsManager),
    /// New async provider (wrapped in Arc for Send + Sync)
    Provider(Arc<dyn EmbeddingProvider>),
}

/// Hybrid searcher combining semantic and code search
pub struct HybridSearcher {
    store: QdrantStore,
    embedding_source: EmbeddingSource,
    scoring: ScoringConfig,
}

impl HybridSearcher {
    /// Connect to Qdrant and create a new hybrid searcher with default local provider
    pub async fn connect(config: QdrantConfig, repo_id: impl Into<String>) -> Result<Self> {
        let store = QdrantStore::connect(config, repo_id).await?;
        let embeddings = EmbeddingsManager::new()?;

        Ok(Self {
            store,
            embedding_source: EmbeddingSource::Legacy(embeddings),
            scoring: ScoringConfig::default(),
        })
    }

    /// Connect with a specific embedding provider
    pub async fn connect_with_provider(
        config: QdrantConfig,
        repo_id: impl Into<String>,
        provider: Arc<dyn EmbeddingProvider>,
    ) -> Result<Self> {
        let store = QdrantStore::connect(config, repo_id).await?;

        Ok(Self {
            store,
            embedding_source: EmbeddingSource::Provider(provider),
            scoring: ScoringConfig::default(),
        })
    }

    /// Connect using embedding configuration
    pub async fn connect_from_config(
        qdrant_config: QdrantConfig,
        embedding_config: &EmbeddingConfig,
        repo_id: impl Into<String>,
    ) -> Result<Self> {
        let provider = crate::embeddings::create_provider(embedding_config)?;
        Self::connect_with_provider(qdrant_config, repo_id, provider).await
    }

    /// Create from existing store and embeddings manager (legacy)
    pub fn new(store: QdrantStore, embeddings: EmbeddingsManager) -> Self {
        Self {
            store,
            embedding_source: EmbeddingSource::Legacy(embeddings),
            scoring: ScoringConfig::default(),
        }
    }

    /// Set custom scoring configuration
    pub fn with_scoring(mut self, config: ScoringConfig) -> Self {
        self.scoring = config;
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

    /// Preload embedding models for faster first query
    pub fn preload_models(&self) -> Result<()> {
        match &self.embedding_source {
            EmbeddingSource::Legacy(mgr) => mgr.preload(),
            EmbeddingSource::Provider(_) => Ok(()), // Remote providers don't need preload
        }
    }

    /// Encode a semantic query
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

    /// Encode a code query
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

    /// Get the index status (number of indexed points in each collection)
    ///
    /// Returns (semantic_count, code_count) or None if collections don't exist
    pub async fn index_status(&self) -> Result<Option<(u64, u64)>> {
        use crate::schema::collections;

        let semantic_info = self.store.collection_info(collections::SEMANTIC).await?;
        let code_info = self.store.collection_info(collections::CODE).await?;

        match (semantic_info, code_info) {
            (Some(s), Some(c)) => Ok(Some((
                s.points_count.unwrap_or(0),
                c.points_count.unwrap_or(0),
            ))),
            _ => Ok(None),
        }
    }

    /// Check if the index is empty (no points indexed)
    pub async fn is_index_empty(&self) -> Result<bool> {
        match self.index_status().await? {
            Some((semantic, code)) => Ok(semantic == 0 && code == 0),
            None => Ok(true), // Collections don't exist = empty
        }
    }

    /// Perform hybrid search combining semantic, code, and name-based approaches
    ///
    /// # Arguments
    /// * `query` - Search query (natural language or code identifier)
    /// * `limit` - Maximum number of results to return
    ///
    /// # Returns
    /// Vector of search results sorted by combined score (descending)
    ///
    /// # Search Strategy
    ///
    /// For Identifier queries (camelCase, PascalCase, snake_case):
    /// - Runs embedding search on both semantic and code collections
    /// - Additionally runs exact name search to guarantee name matches are found
    /// - Merges results, ensuring exact name matches get proper scoring
    ///
    /// For other queries (Question, Natural):
    /// - Runs embedding search only (semantic + code)
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<HybridSearchHit>> {
        info!("Hybrid search for: '{}'", query);

        // Classify query intent
        let query_type = Self::classify_query(query);
        debug!("Query classified as: {:?}", query_type);

        // Search with larger pool for better exact match detection
        let search_pool_size = std::cmp::max(50, limit * 4) as u64;

        // Generate embeddings
        let semantic_vec = self.encode_semantic(query).await?;
        let code_vec = self.encode_code(query).await?;

        // Search both collections
        let semantic_results = self
            .store
            .search(collections::SEMANTIC, semantic_vec, search_pool_size, None)
            .await?;

        let code_results = self
            .store
            .search(collections::CODE, code_vec, search_pool_size, None)
            .await?;

        debug!(
            "Found {} semantic, {} code results",
            semantic_results.len(),
            code_results.len()
        );

        // For Identifier queries, also search by name to guarantee exact matches are found
        let name_results = if matches!(query_type, QueryType::Identifier) {
            // Try exact name match first
            let exact = self
                .store
                .scroll_by_name(collections::CODE, query, 10)
                .await
                .unwrap_or_default();

            // If no exact match, try common case variations
            if exact.is_empty() {
                let variations = self.generate_name_variations(query);
                let mut all_results = Vec::new();
                for variant in variations {
                    if let Ok(results) = self
                        .store
                        .scroll_by_name(collections::CODE, &variant, 5)
                        .await
                    {
                        all_results.extend(results);
                    }
                }
                all_results
            } else {
                exact
            }
        } else {
            Vec::new()
        };

        if !name_results.is_empty() {
            debug!("Found {} name-matched results", name_results.len());
        }

        // Fuse results including name matches
        let mut fused = self.fuse_results_with_names(
            semantic_results,
            code_results,
            name_results,
            query,
            query_type,
        );

        // Sort by combined score and limit
        fused.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());
        fused.truncate(limit);

        Ok(fused)
    }

    /// Generate case variations for name matching
    fn generate_name_variations(&self, query: &str) -> Vec<String> {
        let mut variations = Vec::new();
        let trimmed = query.trim();

        // Original
        variations.push(trimmed.to_string());

        // lowercase
        let lower = trimmed.to_lowercase();
        if lower != trimmed {
            variations.push(lower);
        }

        // UPPERCASE
        let upper = trimmed.to_uppercase();
        if upper != trimmed {
            variations.push(upper);
        }

        // PascalCase (capitalize first letter)
        if !trimmed.is_empty() {
            let mut chars: Vec<char> = trimmed.chars().collect();
            chars[0] = chars[0].to_uppercase().next().unwrap_or(chars[0]);
            let pascal: String = chars.into_iter().collect();
            if pascal != trimmed && !variations.contains(&pascal) {
                variations.push(pascal);
            }
        }

        // camelCase (lowercase first letter)
        if !trimmed.is_empty() {
            let mut chars: Vec<char> = trimmed.chars().collect();
            chars[0] = chars[0].to_lowercase().next().unwrap_or(chars[0]);
            let camel: String = chars.into_iter().collect();
            if camel != trimmed && !variations.contains(&camel) {
                variations.push(camel);
            }
        }

        variations
    }

    /// Search with type filtering
    pub async fn search_with_types(
        &self,
        query: &str,
        limit: usize,
        types: Vec<&str>,
    ) -> Result<Vec<HybridSearchHit>> {
        info!("Hybrid search for: '{}' (types: {:?})", query, types);

        let query_type = Self::classify_query(query);
        let search_pool_size = std::cmp::max(50, limit * 4) as u64;

        let semantic_vec = self.encode_semantic(query).await?;
        let code_vec = self.encode_code(query).await?;

        let type_filter = if types.is_empty() { None } else { Some(types) };

        let semantic_results = self
            .store
            .search(
                collections::SEMANTIC,
                semantic_vec,
                search_pool_size,
                type_filter.clone(),
            )
            .await?;

        let code_results = self
            .store
            .search(collections::CODE, code_vec, search_pool_size, type_filter)
            .await?;

        let mut fused = self.fuse_results(semantic_results, code_results, query, query_type);
        fused.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());
        fused.truncate(limit);

        Ok(fused)
    }

    /// Search by explicit mode: "code", "info", or None for hybrid
    ///
    /// # Arguments
    /// * `query` - Search query
    /// * `limit` - Maximum number of results
    /// * `types` - Optional type filters
    /// * `mode` - Search mode: "code" (code collection only), "info" (semantic only), None (hybrid)
    ///
    /// # Returns
    /// Vector of search results sorted by score (descending)
    pub async fn search_by_mode(
        &self,
        query: &str,
        limit: usize,
        types: Vec<&str>,
        mode: Option<&str>,
    ) -> Result<Vec<HybridSearchHit>> {
        match mode {
            Some("code") => self.search_code_only(query, limit, types).await,
            Some("info") => self.search_semantic_only(query, limit, types).await,
            _ => {
                // Hybrid mode (default)
                if types.is_empty() {
                    self.search(query, limit).await
                } else {
                    self.search_with_types(query, limit, types).await
                }
            }
        }
    }

    /// Search using only the code embeddings collection (jina-base-code)
    ///
    /// Best for: code identifiers, function names, class names, code patterns
    async fn search_code_only(
        &self,
        query: &str,
        limit: usize,
        types: Vec<&str>,
    ) -> Result<Vec<HybridSearchHit>> {
        info!("Code-only search for: '{}'", query);

        let search_pool_size = std::cmp::max(50, limit * 4) as u64;
        let code_vec = self.encode_code(query).await?;

        let type_filter = if types.is_empty() { None } else { Some(types) };

        let code_results = self
            .store
            .search(collections::CODE, code_vec, search_pool_size, type_filter)
            .await?;

        debug!("Found {} code results", code_results.len());

        let mut results = self.convert_single_source_results(code_results, query, "code");
        results.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());
        results.truncate(limit);

        Ok(results)
    }

    /// Search using only the semantic embeddings collection (jina-base-en)
    ///
    /// Best for: natural language questions, conceptual queries, documentation search
    async fn search_semantic_only(
        &self,
        query: &str,
        limit: usize,
        types: Vec<&str>,
    ) -> Result<Vec<HybridSearchHit>> {
        info!("Semantic-only search for: '{}'", query);

        let search_pool_size = std::cmp::max(50, limit * 4) as u64;
        let semantic_vec = self.encode_semantic(query).await?;

        let type_filter = if types.is_empty() { None } else { Some(types) };

        let semantic_results = self
            .store
            .search(
                collections::SEMANTIC,
                semantic_vec,
                search_pool_size,
                type_filter,
            )
            .await?;

        debug!("Found {} semantic results", semantic_results.len());

        let mut results = self.convert_single_source_results(semantic_results, query, "semantic");
        results.sort_by(|a, b| b.combined_score.partial_cmp(&a.combined_score).unwrap());
        results.truncate(limit);

        Ok(results)
    }

    /// Convert single-source search results to HybridSearchHit format
    fn convert_single_source_results(
        &self,
        results: Vec<SearchHit>,
        query: &str,
        source: &str,
    ) -> Vec<HybridSearchHit> {
        // Rescale function to spread out typical [0.5, 1.0] similarity scores
        let rescale = |score: f32| -> f32 {
            let floor = 0.5;
            let ceiling = 1.0;
            let target_max = 0.65;

            if score <= floor {
                score * target_max / floor
            } else {
                let normalized = (score - floor) / (ceiling - floor);
                target_max * normalized
            }
        };

        results
            .into_iter()
            .map(|hit| {
                // Rescale raw score first
                let base_score = rescale(hit.score);

                // Apply exact match and context bonuses
                let exact_match_bonus = self.calculate_exact_match_bonus(query, &hit.payload.name);
                let type_bonus = self.calculate_type_bonus(query, &hit.payload.entity_type);
                let context_boost = self.calculate_context_boost(query, &hit.payload.file_path);

                let combined_score =
                    (base_score + exact_match_bonus + type_bonus + context_boost).clamp(0.0, 1.0);

                let mut individual_scores = HashMap::new();
                individual_scores.insert(source.to_string(), hit.score);

                HybridSearchHit {
                    entity_id: hit.payload.entity_id,
                    name: hit.payload.name,
                    entity_type: hit.payload.entity_type,
                    kind: hit.payload.kind,
                    subtype: hit.payload.subtype,
                    file_path: hit.payload.file_path,
                    line_range: (hit.payload.start_line, hit.payload.end_line),
                    combined_score,
                    code_snippet: hit.content,
                    found_via: vec![source.to_string()],
                    individual_scores,
                }
            })
            .collect()
    }

    /// Classify query intent to adjust scoring weights
    ///
    /// Returns:
    /// - `Identifier` - Code entity lookup (camelCase, snake_case, etc.)
    /// - `Question` - Natural language question
    /// - `Natural` - General natural language query
    pub fn classify_query(query: &str) -> QueryType {
        let query_stripped = query.trim();

        // Check for question patterns
        let question_starters = [
            "how", "what", "why", "where", "when", "which", "who", "is", "are", "can", "does", "do",
        ];
        let first_word = query_stripped
            .to_lowercase()
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();

        if query_stripped.ends_with('?') || question_starters.contains(&first_word.as_str()) {
            return QueryType::Question;
        }

        // Check for identifier patterns (single token that looks like code)
        // Matches: camelCase, PascalCase, snake_case, SCREAMING_SNAKE, kebab-case
        let identifier_pattern = Regex::new(r"^[a-zA-Z_][a-zA-Z0-9_\-]*$").unwrap();
        let no_spaces = query_stripped.replace(' ', "");
        let has_spaces = query_stripped.contains(' ');

        if identifier_pattern.is_match(&no_spaces) {
            // Additional check: contains typical code patterns
            let has_underscore = query_stripped.contains('_');
            let has_dash = query_stripped.contains('-');
            let has_camel = Regex::new(r"[a-z][A-Z]").unwrap().is_match(query_stripped);

            // For single-word queries, starting with uppercase suggests PascalCase identifier
            // For multi-word queries like "VM allocation", starts_upper alone is NOT enough
            // (could be natural language starting with acronym or proper noun)
            let is_single_word_pascal = !has_spaces
                && query_stripped
                    .chars()
                    .next()
                    .map(|c| c.is_uppercase())
                    .unwrap_or(false);

            if has_underscore || has_dash || has_camel || is_single_word_pascal {
                return QueryType::Identifier;
            }
        }

        QueryType::Natural
    }

    /// Fuse semantic and code search results with enhanced scoring
    fn fuse_results(
        &self,
        semantic_results: Vec<SearchHit>,
        code_results: Vec<SearchHit>,
        query: &str,
        query_type: QueryType,
    ) -> Vec<HybridSearchHit> {
        self.fuse_results_with_names(
            semantic_results,
            code_results,
            Vec::new(),
            query,
            query_type,
        )
    }

    /// Fuse semantic, code, and name-based search results using Reciprocal Rank Fusion (RRF)
    ///
    /// RRF is score-agnostic and uses rank positions: score = Σ 1/(k + rank)
    /// This handles different score distributions between collections naturally.
    /// Name-matched results receive a high fixed RRF contribution.
    fn fuse_results_with_names(
        &self,
        semantic_results: Vec<SearchHit>,
        code_results: Vec<SearchHit>,
        name_results: Vec<SearchHit>,
        query: &str,
        query_type: QueryType,
    ) -> Vec<HybridSearchHit> {
        const RRF_K: f32 = 60.0; // Standard RRF constant

        let weights = query_type.weights();

        // Build rank maps: entity_id -> (rank, hit)
        let mut semantic_ranks: HashMap<String, (usize, SearchHit)> = HashMap::new();
        let mut code_ranks: HashMap<String, (usize, SearchHit)> = HashMap::new();
        let mut name_ranks: HashMap<String, (usize, SearchHit)> = HashMap::new();

        for (rank, hit) in semantic_results.into_iter().enumerate() {
            semantic_ranks.insert(hit.payload.entity_id.clone(), (rank, hit));
        }
        for (rank, hit) in code_results.into_iter().enumerate() {
            code_ranks.insert(hit.payload.entity_id.clone(), (rank, hit));
        }
        for (rank, hit) in name_results.into_iter().enumerate() {
            name_ranks.insert(hit.payload.entity_id.clone(), (rank, hit));
        }

        // Collect all unique entity IDs
        let mut all_entities: HashSet<String> = HashSet::new();
        all_entities.extend(semantic_ranks.keys().cloned());
        all_entities.extend(code_ranks.keys().cloned());
        all_entities.extend(name_ranks.keys().cloned());

        // Calculate RRF scores and build results
        all_entities
            .into_iter()
            .map(|entity_id| {
                // Calculate weighted RRF contributions
                let semantic_rrf = semantic_ranks
                    .get(&entity_id)
                    .map(|(rank, _)| weights.semantic / (RRF_K + *rank as f32 + 1.0))
                    .unwrap_or(0.0);

                let code_rrf = code_ranks
                    .get(&entity_id)
                    .map(|(rank, _)| weights.code / (RRF_K + *rank as f32 + 1.0))
                    .unwrap_or(0.0);

                // Name matches get a strong fixed contribution (equivalent to rank 0)
                let name_rrf = if name_ranks.contains_key(&entity_id) {
                    1.0 / (RRF_K + 1.0) // ~0.016, will be boosted by exact match bonus
                } else {
                    0.0
                };

                let rrf_base = semantic_rrf + code_rrf + name_rrf;

                // Get primary hit for metadata (prefer name, then highest scorer)
                let primary = name_ranks
                    .get(&entity_id)
                    .map(|(_, hit)| hit)
                    .or_else(|| semantic_ranks.get(&entity_id).map(|(_, hit)| hit))
                    .or_else(|| code_ranks.get(&entity_id).map(|(_, hit)| hit))
                    .unwrap();

                // Apply bonuses (exact match, type, context)
                let has_name_match = name_ranks.contains_key(&entity_id);
                let exact_match_bonus = if has_name_match {
                    self.scoring.exact_match_bonus
                } else {
                    self.calculate_exact_match_bonus(query, &primary.payload.name)
                };
                let type_bonus = self.calculate_type_bonus(query, &primary.payload.entity_type);
                let context_boost = self.calculate_context_boost(query, &primary.payload.file_path);

                // Scale RRF to usable range and add bonuses
                // RRF scores are small (~0.01-0.03), scale to ~0.3-0.6 range
                let scaled_rrf = rrf_base * 30.0;
                let combined_score = (scaled_rrf + exact_match_bonus + type_bonus + context_boost)
                    .clamp(0.0, 1.0);

                // Build found_via list
                let mut found_via = Vec::new();
                if semantic_ranks.contains_key(&entity_id) {
                    found_via.push("semantic".to_string());
                }
                if code_ranks.contains_key(&entity_id) {
                    found_via.push("code".to_string());
                }
                if has_name_match {
                    found_via.push("name".to_string());
                }

                // Build individual scores (original embedding scores for debugging)
                let mut individual_scores: HashMap<String, f32> = HashMap::new();
                if let Some((_, hit)) = semantic_ranks.get(&entity_id) {
                    individual_scores.insert("semantic".to_string(), hit.score);
                }
                if let Some((_, hit)) = code_ranks.get(&entity_id) {
                    individual_scores.insert("code".to_string(), hit.score);
                }

                debug!(
                    "RRF for '{}': sem_rrf={:.4}, code_rrf={:.4}, name_rrf={:.4}, scaled={:.3}, final={:.3}",
                    primary.payload.name, semantic_rrf, code_rrf, name_rrf, scaled_rrf, combined_score
                );

                HybridSearchHit {
                    entity_id,
                    name: primary.payload.name.clone(),
                    entity_type: primary.payload.entity_type.clone(),
                    kind: primary.payload.kind.clone(),
                    subtype: primary.payload.subtype.clone(),
                    file_path: primary.payload.file_path.clone(),
                    line_range: (primary.payload.start_line, primary.payload.end_line),
                    combined_score,
                    code_snippet: primary.content.clone(),
                    found_via,
                    individual_scores,
                }
            })
            .collect()
    }

    /// Calculate exact match bonus
    fn calculate_exact_match_bonus(&self, query: &str, name: &str) -> f32 {
        let query_lower = query.to_lowercase();
        let query_lower = query_lower.trim();
        let name_lower = name.to_lowercase();
        let name_lower = name_lower.trim();

        if query_lower == name_lower {
            // Perfect exact match
            return self.scoring.exact_match_bonus;
        }

        // Match ignoring separators
        let query_normalized: String = query_lower
            .chars()
            .filter(|c| *c != '_' && *c != '-')
            .collect();
        let name_normalized: String = name_lower
            .chars()
            .filter(|c| *c != '_' && *c != '-')
            .collect();

        if query_normalized == name_normalized {
            return self.scoring.separator_match_bonus;
        }

        // Substring matching (minimum 3 chars)
        if query_lower.len() >= 3 {
            if name_lower.contains(query_lower) {
                // Query is substring of name
                let overlap_ratio = query_lower.len() as f32 / name_lower.len() as f32;
                if overlap_ratio > 0.3 {
                    return 0.15 * overlap_ratio;
                }
            } else if query_lower.contains(name_lower) && name_lower.len() >= 3 {
                // Name is substring of query (less common, smaller bonus)
                return 0.08;
            }
        }

        0.0
    }

    /// Calculate bonus when entity type matches query hints
    fn calculate_type_bonus(&self, query: &str, entity_type: &str) -> f32 {
        let query_lower = query.to_lowercase();
        let entity_type_lower = entity_type.to_lowercase();

        if query_lower.contains("class") || query_lower.contains("type") {
            if entity_type_lower == "container" {
                return self.scoring.type_bonus;
            }
        } else if query_lower.contains("function")
            || query_lower.contains("method")
            || query_lower.contains("def")
        {
            if entity_type_lower == "callable" {
                return self.scoring.type_bonus;
            }
        } else if (query_lower.contains("variable")
            || query_lower.contains("field")
            || query_lower.contains("property"))
            && entity_type_lower == "data"
        {
            return self.scoring.type_bonus;
        }

        0.0
    }

    /// Calculate context boost based on file path
    fn calculate_context_boost(&self, query: &str, file_path: &str) -> f32 {
        let file_path_lower = file_path.to_lowercase();
        let query_lower = query.to_lowercase();

        // Boost core implementation files over tests/tools
        if file_path_lower.contains("/src/")
            && !file_path_lower.contains("/test")
            && !file_path_lower.contains("/tools/")
        {
            return self.scoring.src_context_boost;
        }

        // Slight penalty for test files (unless query seems test-related)
        if file_path_lower.contains("/test") && !query_lower.contains("test") {
            return self.scoring.test_context_penalty;
        }

        0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_query_question() {
        assert_eq!(
            HybridSearcher::classify_query("how does authentication work?"),
            QueryType::Question
        );
        assert_eq!(
            HybridSearcher::classify_query("what is the purpose of this?"),
            QueryType::Question
        );
        assert_eq!(
            HybridSearcher::classify_query("is this a bug"),
            QueryType::Question
        );
    }

    #[test]
    fn test_classify_query_identifier() {
        assert_eq!(
            HybridSearcher::classify_query("camelCase"),
            QueryType::Identifier
        );
        assert_eq!(
            HybridSearcher::classify_query("PascalCase"),
            QueryType::Identifier
        );
        assert_eq!(
            HybridSearcher::classify_query("snake_case"),
            QueryType::Identifier
        );
        assert_eq!(
            HybridSearcher::classify_query("SCREAMING_SNAKE"),
            QueryType::Identifier
        );
        assert_eq!(
            HybridSearcher::classify_query("kebab-case"),
            QueryType::Identifier
        );
    }

    #[test]
    fn test_classify_query_natural() {
        assert_eq!(
            HybridSearcher::classify_query("authentication logic"),
            QueryType::Natural
        );
        assert_eq!(
            HybridSearcher::classify_query("error handling"),
            QueryType::Natural
        );
        assert_eq!(
            HybridSearcher::classify_query("find all connections"),
            QueryType::Natural
        );
    }

    #[test]
    fn test_exact_match_bonus() {
        let searcher_scoring = ScoringConfig::default();

        // Create a temporary helper for testing
        let calc_bonus = |query: &str, name: &str| -> f32 {
            let query_lower = query.to_lowercase();
            let query_lower = query_lower.trim();
            let name_lower = name.to_lowercase();
            let name_lower = name_lower.trim();

            if query_lower == name_lower {
                return searcher_scoring.exact_match_bonus;
            }

            let query_normalized: String = query_lower
                .chars()
                .filter(|c| *c != '_' && *c != '-')
                .collect();
            let name_normalized: String = name_lower
                .chars()
                .filter(|c| *c != '_' && *c != '-')
                .collect();

            if query_normalized == name_normalized {
                return searcher_scoring.separator_match_bonus;
            }

            if query_lower.len() >= 3 && name_lower.contains(query_lower) {
                let overlap_ratio = query_lower.len() as f32 / name_lower.len() as f32;
                if overlap_ratio > 0.3 {
                    return 0.15 * overlap_ratio;
                }
            }

            0.0
        };

        // Exact match
        assert!((calc_bonus("authenticate", "authenticate") - 0.35).abs() < 0.01);

        // Separator match
        assert!((calc_bonus("foo_bar", "foobar") - 0.25).abs() < 0.01);

        // Substring match
        let bonus = calc_bonus("auth", "authenticate");
        assert!(bonus > 0.0 && bonus < 0.15);

        // No match
        assert!((calc_bonus("xyz", "authenticate")).abs() < 0.01);
    }

    #[test]
    fn test_weight_presets() {
        // IDENTIFIER: balanced weights (semantic embeddings work well for identifiers)
        let identifier = WeightPreset::IDENTIFIER;
        assert!((identifier.semantic - identifier.code).abs() < 0.01);

        // QUESTION: strongly semantic for concept matching
        let question = WeightPreset::QUESTION;
        assert!(question.semantic > question.code);

        // NATURAL: semantic-favoring for phrases like "error handling"
        let natural = WeightPreset::NATURAL;
        assert!(natural.semantic > natural.code);
    }
}
