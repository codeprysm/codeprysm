//! CodePrysm MCP Server implementation
//!
//! This module implements the MCP server using the rmcp SDK, exposing:
//! - Semantic search (search_graph_nodes with code/info/hybrid modes)
//! - Graph navigation (find_references, find_outgoing_references, find_definitions, find_call_chain)
//! - Code viewing (get_node_info, read_code, find_module_structure)
//! - Index management (sync_repository, get_index_status)

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use rmcp::{
    handler::server::{tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router, ErrorData as McpError,
};
use tokio::sync::{watch, RwLock};
use tracing::{debug, info, warn};

use codeprysm_core::lazy::manager::LazyGraphManager;
use codeprysm_core::{EdgeType, IncrementalUpdater, Node, PetCodeGraph};
use codeprysm_search::{GraphIndexer, HybridSearcher, QdrantConfig};

use crate::tools::*;

/// Timeout for acquiring state write lock (prevents indefinite blocking during sync)
const STATE_LOCK_TIMEOUT: Duration = Duration::from_secs(2);

/// Threshold for logging slow lock acquisition warnings
const LOCK_WARN_THRESHOLD: Duration = Duration::from_millis(100);

/// Acquire a read lock on state with timeout and contention tracking.
/// Read locks allow concurrent access and should be used for query operations.
/// Logs warnings if lock acquisition takes longer than 100ms.
async fn acquire_state_read<'a>(
    state: &'a RwLock<ServerState>,
) -> Result<tokio::sync::RwLockReadGuard<'a, ServerState>, McpError> {
    let start = Instant::now();

    match tokio::time::timeout(STATE_LOCK_TIMEOUT, state.read()).await {
        Ok(guard) => {
            let elapsed = start.elapsed();
            if elapsed >= LOCK_WARN_THRESHOLD {
                warn!(
                    elapsed_ms = elapsed.as_millis() as u64,
                    "Slow state read lock acquisition ({}ms) - possible contention with sync",
                    elapsed.as_millis()
                );
            } else {
                debug!(
                    elapsed_ms = elapsed.as_millis() as u64,
                    "State read lock acquired in {}ms",
                    elapsed.as_millis()
                );
            }
            Ok(guard)
        }
        Err(_) => {
            warn!(
                timeout_secs = STATE_LOCK_TIMEOUT.as_secs(),
                "State read lock acquisition timed out after {}s - sync in progress",
                STATE_LOCK_TIMEOUT.as_secs()
            );
            Err(McpError::internal_error(
                "Server busy (sync in progress). Please retry in a few seconds.",
                None,
            ))
        }
    }
}

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Path to the repository/workspace root
    pub repo_path: PathBuf,
    /// Path to the .codeprysm artifacts directory (contains manifest.json and partitions/)
    pub codeprysm_dir: PathBuf,
    /// Path to SCM queries directory (None = use embedded queries, the default)
    pub queries_path: Option<PathBuf>,
    /// Qdrant configuration for search
    pub qdrant_config: QdrantConfig,
    /// Repository ID for multi-tenant search
    pub repo_id: String,
    /// Enable auto-sync (default: true)
    pub enable_auto_sync: bool,
    /// Auto-sync interval in seconds (default: 30)
    pub sync_interval_secs: u64,
}

impl ServerConfig {
    /// Create config with local Qdrant and default settings.
    /// Uses embedded queries by default.
    pub fn new(repo_path: impl Into<PathBuf>) -> Self {
        let repo_path = repo_path.into();
        let repo_id = repo_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string());

        // Default prism directory
        let codeprysm_dir = repo_path.join(".codeprysm");

        Self {
            repo_path,
            codeprysm_dir,
            queries_path: None, // Use embedded queries by default
            qdrant_config: QdrantConfig::local(),
            repo_id,
            enable_auto_sync: true,
            sync_interval_secs: 30,
        }
    }

    /// Set custom repo ID
    pub fn with_repo_id(mut self, repo_id: impl Into<String>) -> Self {
        self.repo_id = repo_id.into();
        self
    }

    /// Set Qdrant URL
    pub fn with_qdrant_url(mut self, url: impl Into<String>) -> Self {
        self.qdrant_config = QdrantConfig::with_url(url);
        self
    }

    /// Set queries path (overrides embedded queries)
    pub fn with_queries_path(mut self, path: impl Into<PathBuf>) -> Self {
        self.queries_path = Some(path.into());
        self
    }

    /// Set prism artifacts directory
    pub fn with_codeprysm_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.codeprysm_dir = path.into();
        self
    }
}

// ============================================================================
// Index State Management
// ============================================================================

/// Status of the search index
#[derive(Debug, Clone)]
pub enum IndexStatus {
    /// No indexing in progress
    Idle,
    /// Indexing is in progress
    Indexing { started_at: Instant },
    /// Last indexing attempt failed
    Failed { error: String },
}

impl IndexStatus {
    fn as_str(&self) -> &'static str {
        match self {
            IndexStatus::Idle => "idle",
            IndexStatus::Indexing { .. } => "indexing",
            IndexStatus::Failed { .. } => "failed",
        }
    }
}

/// State of the search index for consistency tracking
#[derive(Debug, Clone)]
pub struct IndexState {
    /// Current status
    pub status: IndexStatus,
    /// Hash of the current graph manifest (for consistency checking)
    pub graph_version: Option<String>,
    /// Version that has been indexed in Qdrant
    pub indexed_version: Option<String>,
    /// Progress: (indexed_count, total_count)
    pub progress: (usize, usize),
    /// When indexing last completed successfully
    pub last_indexed_at: Option<Instant>,
    /// Last error message if failed
    pub last_error: Option<String>,
}

impl Default for IndexState {
    fn default() -> Self {
        Self {
            status: IndexStatus::Idle,
            graph_version: None,
            indexed_version: None,
            progress: (0, 0),
            last_indexed_at: None,
            last_error: None,
        }
    }
}

impl IndexState {
    /// Check if index needs to be rebuilt (version mismatch)
    pub fn needs_reindex(&self) -> bool {
        match (&self.graph_version, &self.indexed_version) {
            (Some(graph), Some(indexed)) => graph != indexed,
            (Some(_), None) => true,
            _ => false,
        }
    }

    /// Check if currently indexing
    pub fn is_indexing(&self) -> bool {
        matches!(self.status, IndexStatus::Indexing { .. })
    }
}

// ============================================================================
// Version Tracking
// ============================================================================

/// Compute a version hash from the manifest file
fn compute_graph_version(codeprysm_dir: &std::path::Path) -> Option<String> {
    let manifest_path = codeprysm_dir.join("manifest.json");
    if !manifest_path.exists() {
        return None;
    }

    match fs::read(&manifest_path) {
        Ok(content) => {
            let mut hasher = Sha256::new();
            hasher.update(&content);
            Some(format!("{:x}", hasher.finalize()))
        }
        Err(e) => {
            warn!("Failed to read manifest for version hash: {}", e);
            None
        }
    }
}

/// Path to the index version file
fn index_version_path(codeprysm_dir: &std::path::Path) -> PathBuf {
    codeprysm_dir.join("index_version.json")
}

/// Load the indexed version from disk
fn load_indexed_version(codeprysm_dir: &std::path::Path) -> Option<String> {
    let path = index_version_path(codeprysm_dir);
    if !path.exists() {
        return None;
    }

    match fs::read_to_string(&path) {
        Ok(content) => match serde_json::from_str::<serde_json::Value>(&content) {
            Ok(json) => json.get("version")?.as_str().map(|s| s.to_string()),
            Err(_) => None,
        },
        Err(_) => None,
    }
}

/// Save the indexed version to disk
fn save_indexed_version(codeprysm_dir: &std::path::Path, version: &str) -> std::io::Result<()> {
    let path = index_version_path(codeprysm_dir);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let json = serde_json::json!({
        "version": version,
        "indexed_at_unix": timestamp,
    });
    fs::write(
        &path,
        serde_json::to_string_pretty(&json).unwrap_or_default(),
    )
}

/// Shared state for the MCP server (contains only graph data)
///
/// Note: IncrementalUpdater and HybridSearcher are intentionally kept outside
/// ServerState to allow sync and search operations to run concurrently with
/// graph queries. Only the lazy_graph requires the state lock.
struct ServerState {
    /// Lazy-loading graph manager (partitioned SQLite storage)
    lazy_graph: LazyGraphManager,
    repo_path: PathBuf,
    /// Path to .codeprysm directory for version tracking
    codeprysm_dir: PathBuf,
    /// Qdrant config for re-indexing during sync
    qdrant_config: QdrantConfig,
    /// Repository ID for search indexing
    repo_id: String,
}

/// CodePrysm MCP Server exposing code graph tools
#[derive(Clone)]
pub struct PrismServer {
    state: Arc<RwLock<ServerState>>,
    /// Hybrid searcher (separate from state lock for concurrent search)
    /// Uses Arc for cheap cloning and shared access without lock
    searcher: Option<Arc<HybridSearcher>>,
    /// Incremental updater (separate from state lock for non-blocking sync)
    /// Uses Mutex since updates are sequential and exclusive
    updater: Option<Arc<tokio::sync::Mutex<IncrementalUpdater>>>,
    /// Index state for tracking indexing progress and consistency
    index_state: Arc<RwLock<IndexState>>,
    tool_router: ToolRouter<Self>,
    /// Shutdown signal sender - send `()` to trigger graceful shutdown
    shutdown_tx: watch::Sender<bool>,
}

#[tool_router]
impl PrismServer {
    /// Create a new server instance
    #[allow(clippy::await_holding_lock)]
    pub async fn new(config: ServerConfig) -> Result<Self, crate::McpError> {
        info!("Initializing CodePrysm MCP server");
        info!("  Repository: {}", config.repo_path.display());
        info!("  CodePrysm dir: {}", config.codeprysm_dir.display());

        // Check if we have partitioned storage
        let manifest_path = config.codeprysm_dir.join("manifest.json");

        let lazy_graph = if manifest_path.exists() {
            // Partitioned storage exists - use lazy loading
            info!("Using partitioned graph storage");
            let lazy_graph = LazyGraphManager::open(&config.codeprysm_dir).map_err(|e| {
                crate::McpError::GraphLoad(format!("Failed to open partitioned graph: {}", e))
            })?;

            let stats = lazy_graph.stats();
            info!(
                "Opened lazy graph: {} partitions, {} files tracked, {} cross-partition edges",
                stats.total_partitions, stats.total_files, stats.cross_partition_edges
            );

            lazy_graph
        } else {
            // No graph exists - initialize empty
            // Use `codeprysm-core` CLI to generate the graph first
            warn!("No partitioned graph found, initializing empty storage");
            warn!(
                "Run `codeprysm-core --repo {} --output {}` to generate the graph",
                config.repo_path.display(),
                config.codeprysm_dir.display()
            );

            LazyGraphManager::init(&config.codeprysm_dir).map_err(|e| {
                crate::McpError::GraphLoad(format!(
                    "Failed to initialize partitioned storage: {}",
                    e
                ))
            })?
        };

        let stats = lazy_graph.stats();
        info!(
            "Lazy graph ready: {} partitions available, {} loaded",
            stats.total_partitions, stats.loaded_partitions
        );

        // Try to connect to search (optional - gracefully degrade if unavailable)
        let searcher =
            match HybridSearcher::connect(config.qdrant_config.clone(), &config.repo_id).await {
                Ok(s) => {
                    info!("Connected to Qdrant for hybrid search");

                    // Preload embedding models for faster first query
                    info!("Preloading embedding models...");
                    if let Err(e) = s.preload_models() {
                        warn!("Failed to preload embedding models: {}", e);
                    } else {
                        info!("Embedding models preloaded successfully");
                    }

                    // Index graph on startup if needed
                    match GraphIndexer::new(
                        config.qdrant_config.clone(),
                        &config.repo_id,
                        &config.repo_path,
                    )
                    .await
                    {
                        Ok(mut indexer) => {
                            // Check if we need to index
                            match indexer.needs_indexing().await {
                                Ok(true) | Err(_) => {
                                    info!("Indexing graph into Qdrant...");

                                    // Load all partitions for full indexing
                                    match lazy_graph.load_all_partitions() {
                                        Ok(loaded) => {
                                            info!("Loaded {} partitions for indexing", loaded);

                                            // Index using the PetCodeGraph from lazy_graph (acquire read lock)
                                            let pet_graph = lazy_graph.graph_read();
                                            match indexer.index_graph(&pet_graph).await {
                                                Ok(stats) => {
                                                    info!(
                                                        "Indexed {} nodes ({} semantic, {} code)",
                                                        stats.total_indexed,
                                                        stats.semantic_indexed,
                                                        stats.code_indexed
                                                    );
                                                }
                                                Err(e) => {
                                                    warn!("Failed to index graph: {}", e);
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            warn!("Failed to load partitions for indexing: {}", e);
                                        }
                                    }
                                }
                                Ok(false) => {
                                    debug!("Collections exist, skipping indexing");
                                }
                            }
                        }
                        Err(e) => {
                            warn!("Failed to create indexer: {}", e);
                        }
                    }

                    Some(Arc::new(s))
                }
                Err(e) => {
                    warn!("Qdrant not available, search disabled: {}", e);
                    None
                }
            };

        // Initialize incremental updater - use embedded queries by default
        // Note: Kept outside ServerState for non-blocking sync operations
        let updater = match &config.queries_path {
            Some(queries_dir) => {
                IncrementalUpdater::new(&config.repo_path, &config.codeprysm_dir, queries_dir).ok()
            }
            None => IncrementalUpdater::new_with_embedded_queries(
                &config.repo_path,
                &config.codeprysm_dir,
            )
            .ok(),
        };
        let updater = updater.map(|u| Arc::new(tokio::sync::Mutex::new(u)));

        // Compute graph version for consistency tracking
        let graph_version = compute_graph_version(&config.codeprysm_dir);
        let indexed_version = load_indexed_version(&config.codeprysm_dir);
        let codeprysm_dir = config.codeprysm_dir.clone();

        info!("Graph version: {:?}", graph_version);
        info!("Indexed version: {:?}", indexed_version);

        let state = ServerState {
            lazy_graph,
            repo_path: config.repo_path,
            codeprysm_dir: codeprysm_dir.clone(),
            qdrant_config: config.qdrant_config,
            repo_id: config.repo_id,
        };

        let state = Arc::new(RwLock::new(state));
        let (shutdown_tx, shutdown_rx) = watch::channel(false);

        // Spawn auto-sync background task if enabled
        if config.enable_auto_sync {
            if let Some(ref updater) = updater {
                let sync_state = Arc::clone(&state);
                let sync_updater = Arc::clone(updater);
                let sync_interval = config.sync_interval_secs;

                tokio::spawn(auto_sync_task(
                    sync_state,
                    sync_updater,
                    sync_interval,
                    shutdown_rx,
                ));

                info!("Auto-sync enabled with {}s interval", sync_interval);
            } else {
                info!("Auto-sync disabled: incremental updater not available");
            }
        }

        // Initialize index state with version tracking
        let index_state = Arc::new(RwLock::new(IndexState {
            graph_version: graph_version.clone(),
            indexed_version: indexed_version.clone(),
            ..Default::default()
        }));

        Ok(Self {
            state,
            searcher,
            updater,
            index_state,
            tool_router: Self::tool_router(),
            shutdown_tx,
        })
    }

    /// Trigger graceful shutdown of background tasks
    pub fn shutdown(&self) {
        info!("Shutdown signal sent to background tasks");
        let _ = self.shutdown_tx.send(true);
    }

    // =========================================================================
    // MCP Tools
    // =========================================================================

    #[tool(
        name = "search_graph_nodes",
        description = "Find code entities by name or description. Returns matches with file locations, scores, and snippets.\n\nMODES:\n- 'code': For identifiers/names (e.g., 'parseConfig', 'UserService', 'handle_request')\n- 'info': For concepts (e.g., 'authentication logic', 'error handling', 'database setup')\n- default: Hybrid - use when unsure (recommended for most queries)\n\nEXAMPLES: search('UserService', mode='code'), search('how errors are handled', mode='info')"
    )]
    async fn search_graph_nodes(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let query = params.query;
        let max_results = params.max_results.unwrap_or(20);
        let node_types = params.node_types;
        let mode = params.mode;

        debug!(
            "search_graph_nodes: query='{}', mode={:?}, max_results={}",
            query, mode, max_results
        );

        // Check if search is available (no state lock needed - searcher is separate)
        let searcher = self.searcher.as_ref().ok_or_else(|| {
            McpError::internal_error(
                "Hybrid search not available. Ensure Qdrant is running.",
                None,
            )
        })?;

        // Parse type filters
        let type_filter: Vec<&str> = node_types
            .as_ref()
            .map(|types| types.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        // Perform search with mode
        let results = searcher
            .search_by_mode(&query, max_results, type_filter, mode.as_deref())
            .await
            .map_err(|e| McpError::internal_error(format!("Search failed: {}", e), None))?;

        // Format results
        let formatted: Vec<serde_json::Value> = results
            .iter()
            .map(|hit| {
                serde_json::json!({
                    "id": hit.entity_id,
                    "name": hit.name,
                    "type": hit.entity_type,
                    "kind": hit.kind,
                    "subtype": hit.subtype,
                    "file": hit.file_path,
                    "line": hit.line_range.0,
                    "end_line": hit.line_range.1,
                    "score": hit.combined_score,
                    "sources": hit.found_via,
                    "code_snippet": truncate_snippet(&hit.code_snippet, 200),
                })
            })
            .collect();

        // Check index status if no results found
        let index_hint = if results.is_empty() {
            match searcher.is_index_empty().await {
                Ok(true) => Some("Index is empty. Call sync_repository to index the codebase."),
                Ok(false) => None, // Index has data, just no matches
                Err(_) => None,    // Couldn't check, don't hint
            }
        } else {
            None
        };

        let mut response = serde_json::json!({
            "query": query,
            "mode": mode,
            "node_types": node_types,
            "result_count": results.len(),
            "results": formatted,
        });

        if let Some(hint) = index_hint {
            response["hint"] = serde_json::json!(hint);
        }

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap_or_default(),
        )]))
    }

    #[tool(
        name = "get_node_info",
        description = "Get metadata about a code entity: name, type, kind, file path, line numbers. Does NOT include source code - use read_code for that. Use after search to inspect a specific result."
    )]
    async fn get_node_info(
        &self,
        Parameters(params): Parameters<NodeInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        let node_id = params.node_id;
        debug!("get_node_info: node_id='{}'", node_id);

        // Use read lock - LazyGraphManager uses interior mutability for partition loading
        let state = acquire_state_read(&self.state).await?;

        let node = state
            .lazy_graph
            .get_node(&node_id)
            .map_err(|e| McpError::internal_error(format!("Failed to load node: {}", e), None))?
            .ok_or_else(|| {
                McpError::invalid_params(format!("Node not found: {}", node_id), None)
            })?;

        let response = format_node_info(&node);

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap_or_default(),
        )]))
    }

    #[tool(
        name = "read_code",
        description = "Read source code for a node or file range. Use node_id to read a function/class implementation, or file_path+line_start/line_end for arbitrary ranges. Use context_lines to see surrounding code. Use max_lines to limit output (default 100)."
    )]
    async fn read_code(
        &self,
        Parameters(params): Parameters<ReadCodeParams>,
    ) -> Result<CallToolResult, McpError> {
        let max_lines = params.max_lines.unwrap_or(100);
        let context_lines = params.context_lines.unwrap_or(0);

        debug!(
            "read_code: node_id={:?}, file_path={:?}",
            params.node_id, params.file_path
        );

        // Extract file path and metadata under lock, then release before file I/O
        let (file_path, mut line_start, mut line_end, node_info, full_path) = {
            // Use read lock - LazyGraphManager uses interior mutability for partition loading
            let state = acquire_state_read(&self.state).await?;

            // Determine file path and line range
            let (file_path, line_start, line_end, node_info) =
                if let Some(ref node_id) = params.node_id {
                    let node = state
                        .lazy_graph
                        .get_node(node_id)
                        .map_err(|e| {
                            McpError::internal_error(format!("Failed to load node: {}", e), None)
                        })?
                        .ok_or_else(|| {
                            McpError::invalid_params(format!("Node not found: {}", node_id), None)
                        })?;

                    let start = params.line_start.unwrap_or(node.line);
                    let end = params.line_end.unwrap_or(node.end_line);

                    (node.file.clone(), start, end, Some(format_node_info(&node)))
                } else if let Some(ref file_path) = params.file_path {
                    let start = params.line_start.unwrap_or(1);
                    let end = params.line_end.unwrap_or(start + max_lines - 1);
                    (file_path.clone(), start, end, None)
                } else {
                    return Err(McpError::invalid_params(
                        "Either node_id or file_path must be provided",
                        None,
                    ));
                };

            // Clone repo_path before releasing lock
            let full_path = state.repo_path.join(&file_path);

            (file_path, line_start, line_end, node_info, full_path)
        }; // Read lock released here

        // Apply context lines (outside lock)
        if context_lines > 0 {
            line_start = line_start.saturating_sub(context_lines);
            if line_start == 0 {
                line_start = 1;
            }
            line_end += context_lines;
        }

        // Enforce max_lines limit (outside lock)
        if line_end - line_start + 1 > max_lines {
            line_end = line_start + max_lines - 1;
        }

        // Read the file (outside lock - this is the slow I/O operation)
        let content = tokio::task::spawn_blocking({
            let full_path = full_path.clone();
            move || std::fs::read_to_string(&full_path)
        })
        .await
        .map_err(|e| McpError::internal_error(format!("File read task panicked: {}", e), None))?
        .map_err(|e| {
            McpError::invalid_params(
                format!("Failed to read file {}: {}", full_path.display(), e),
                None,
            )
        })?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        // Adjust bounds
        let start_idx = (line_start.saturating_sub(1)).min(total_lines);
        let end_idx = line_end.min(total_lines);

        let selected_lines = &lines[start_idx..end_idx];
        let content_text = selected_lines.join("\n");

        let mut response = serde_json::json!({
            "file": file_path,
            "full_path": full_path.display().to_string(),
            "line_start": line_start,
            "line_end": end_idx,
            "lines_read": selected_lines.len(),
            "content": content_text,
        });

        if let Some(info) = node_info {
            response["node_info"] = info;
        }

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap_or_default(),
        )]))
    }

    #[tool(
        name = "find_references",
        description = "Find code that uses/calls this node (incoming edges). For a function: returns callers. For a class: returns instantiators/importers. For a variable: returns readers. Answer: 'Who uses this?'"
    )]
    async fn find_references(
        &self,
        Parameters(params): Parameters<FindReferencesParams>,
    ) -> Result<CallToolResult, McpError> {
        let node_id = params.node_id;
        let edge_types = params.edge_types;
        let include_line_info = params.include_line_info.unwrap_or(true);
        let max_results = params.max_results.unwrap_or(50);

        debug!(
            "find_references: node_id='{}', edge_types={:?}",
            node_id, edge_types
        );

        // Use read lock - LazyGraphManager uses interior mutability for partition loading
        let state = acquire_state_read(&self.state).await?;

        // Verify node exists and get its name
        let node = state
            .lazy_graph
            .get_node(&node_id)
            .map_err(|e| McpError::internal_error(format!("Failed to load node: {}", e), None))?
            .ok_or_else(|| {
                McpError::invalid_params(format!("Node not found: {}", node_id), None)
            })?;
        let node_name = node.name.clone();

        // Parse edge type filters
        let edge_filter: HashSet<&str> = edge_types
            .as_ref()
            .map(|types| types.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        // Find incoming edges (lazy loads cross-partition source nodes as needed)
        let incoming_edges = state.lazy_graph.get_incoming_edges(&node_id).map_err(|e| {
            McpError::internal_error(format!("Failed to find references: {}", e), None)
        })?;

        let mut references = Vec::new();
        for (source_node, edge_data) in incoming_edges {
            if !edge_filter.is_empty() && !edge_filter.contains(edge_data.edge_type.as_str()) {
                continue;
            }

            let mut ref_info = serde_json::json!({
                "source_id": source_node.id,
                "source_name": source_node.name,
                "source_type": source_node.node_type.as_str(),
                "source_file": source_node.file,
                "edge_type": edge_data.edge_type.as_str(),
            });

            if include_line_info {
                ref_info["source_line"] = serde_json::json!(source_node.line);
                if let Some(ref_line) = edge_data.ref_line {
                    ref_info["reference_line"] = serde_json::json!(ref_line);
                }
            }

            references.push(ref_info);

            if references.len() >= max_results {
                break;
            }
        }

        let response = serde_json::json!({
            "node_id": node_id,
            "node_name": node_name,
            "reference_count": references.len(),
            "references": references,
            "truncated": references.len() == max_results,
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap_or_default(),
        )]))
    }

    #[tool(
        name = "find_outgoing_references",
        description = "Find what this node calls/uses (outgoing edges). For a function: returns called functions and read variables. For a class: returns extended types and imports. Answer: 'What does this depend on?'"
    )]
    async fn find_outgoing_references(
        &self,
        Parameters(params): Parameters<FindOutgoingReferencesParams>,
    ) -> Result<CallToolResult, McpError> {
        let node_id = params.node_id;
        let edge_types = params.edge_types;
        let include_line_info = params.include_line_info.unwrap_or(true);
        let max_results = params.max_results.unwrap_or(50);

        debug!(
            "find_outgoing_references: node_id='{}', edge_types={:?}",
            node_id, edge_types
        );

        // Use read lock - LazyGraphManager uses interior mutability for partition loading
        let state = acquire_state_read(&self.state).await?;

        // Verify node exists and get its name
        let node = state
            .lazy_graph
            .get_node(&node_id)
            .map_err(|e| McpError::internal_error(format!("Failed to load node: {}", e), None))?
            .ok_or_else(|| {
                McpError::invalid_params(format!("Node not found: {}", node_id), None)
            })?;
        let node_name = node.name.clone();

        // Parse edge type filters
        let edge_filter: HashSet<&str> = edge_types
            .as_ref()
            .map(|types| types.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        // Find outgoing edges (lazy loads cross-partition target nodes as needed)
        let outgoing_edges = state.lazy_graph.get_outgoing_edges(&node_id).map_err(|e| {
            McpError::internal_error(format!("Failed to find outgoing references: {}", e), None)
        })?;

        let mut references = Vec::new();
        for (target_node, edge_data) in outgoing_edges {
            if !edge_filter.is_empty() && !edge_filter.contains(edge_data.edge_type.as_str()) {
                continue;
            }

            let mut ref_info = serde_json::json!({
                "target_id": target_node.id,
                "target_name": target_node.name,
                "target_type": target_node.node_type.as_str(),
                "target_file": target_node.file,
                "edge_type": edge_data.edge_type.as_str(),
            });

            if let Some(ref ident) = edge_data.ident {
                ref_info["identifier"] = serde_json::json!(ident);
            }

            if include_line_info {
                ref_info["target_line"] = serde_json::json!(target_node.line);
                if let Some(ref_line) = edge_data.ref_line {
                    ref_info["reference_line"] = serde_json::json!(ref_line);
                }
            }

            references.push(ref_info);

            if references.len() >= max_results {
                break;
            }
        }

        let response = serde_json::json!({
            "node_id": node_id,
            "node_name": node_name,
            "reference_count": references.len(),
            "references": references,
            "truncated": references.len() == max_results,
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap_or_default(),
        )]))
    }

    #[tool(
        name = "find_definitions",
        description = "Find entities defined inside this node. For a class: returns methods, fields, nested types. For a function: returns parameters and locals. For a file: returns top-level definitions. Answer: 'What does this contain?'"
    )]
    async fn find_definitions(
        &self,
        Parameters(params): Parameters<FindDefinitionsParams>,
    ) -> Result<CallToolResult, McpError> {
        let node_id = params.node_id;
        let include_line_info = params.include_line_info.unwrap_or(true);
        let max_results = params.max_results.unwrap_or(50);

        debug!("find_definitions: node_id='{}'", node_id);

        // Use read lock - LazyGraphManager uses interior mutability for partition loading
        let state = acquire_state_read(&self.state).await?;

        // Verify node exists and get its name/type
        let node = state
            .lazy_graph
            .get_node(&node_id)
            .map_err(|e| McpError::internal_error(format!("Failed to load node: {}", e), None))?
            .ok_or_else(|| {
                McpError::invalid_params(format!("Node not found: {}", node_id), None)
            })?;
        let node_name = node.name.clone();
        let node_type = node.node_type.as_str().to_string();

        // Find DEFINES edges (lazy loads cross-partition target nodes as needed)
        let outgoing_edges = state.lazy_graph.get_outgoing_edges(&node_id).map_err(|e| {
            McpError::internal_error(format!("Failed to find definitions: {}", e), None)
        })?;

        let mut definitions = Vec::new();
        for (target_node, edge_data) in outgoing_edges {
            if edge_data.edge_type != EdgeType::Defines {
                continue;
            }

            let mut def_info = serde_json::json!({
                "defined_id": target_node.id,
                "defined_name": target_node.name,
                "defined_type": target_node.node_type.as_str(),
                "defined_file": target_node.file,
                "edge_type": "DEFINES",
            });

            if include_line_info {
                def_info["defined_line"] = serde_json::json!(target_node.line);
            }

            definitions.push(def_info);

            if definitions.len() >= max_results {
                break;
            }
        }

        let response = serde_json::json!({
            "node_id": node_id,
            "node_name": node_name,
            "node_type": node_type,
            "definition_count": definitions.len(),
            "definitions": definitions,
            "truncated": definitions.len() == max_results,
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap_or_default(),
        )]))
    }

    #[tool(
        name = "find_call_chain",
        description = "Trace execution paths through the call graph. 'upstream': trace back to entry points (who eventually calls this - useful for understanding how code is reached). 'downstream': trace forward to leaves (what this eventually calls - useful for impact analysis)."
    )]
    async fn find_call_chain(
        &self,
        Parameters(params): Parameters<FindCallChainParams>,
    ) -> Result<CallToolResult, McpError> {
        let node_id = params.node_id;
        let direction = params.direction.unwrap_or_else(|| "upstream".to_string());
        let max_depth = params.max_depth.unwrap_or(3);
        let max_chains = params.max_chains.unwrap_or(5);
        let edge_types = params
            .edge_types
            .unwrap_or_else(|| vec!["USES".to_string()]);

        debug!(
            "find_call_chain: node_id='{}', direction='{}', max_depth={}",
            node_id, direction, max_depth
        );

        if !["upstream", "downstream", "both"].contains(&direction.as_str()) {
            return Err(McpError::invalid_params(
                format!(
                    "Invalid direction: {}. Must be 'upstream', 'downstream', or 'both'",
                    direction
                ),
                None,
            ));
        }

        // Use read lock - LazyGraphManager uses interior mutability for partition loading
        let state = acquire_state_read(&self.state).await?;

        // Verify node exists and load its partition
        let node = state
            .lazy_graph
            .get_node(&node_id)
            .map_err(|e| McpError::internal_error(format!("Failed to load node: {}", e), None))?
            .ok_or_else(|| {
                McpError::invalid_params(format!("Node not found: {}", node_id), None)
            })?;
        let node_name = node.name.clone();

        let edge_filter: HashSet<String> = edge_types.into_iter().collect();

        // Build chains using the underlying PetCodeGraph (acquire read lock)
        // Note: This uses currently loaded partitions; unloaded nodes will be skipped
        let graph = state.lazy_graph.graph_read();
        let mut upstream_chains = Vec::new();
        let mut downstream_chains = Vec::new();

        if direction == "upstream" || direction == "both" {
            build_chains_pet(
                &graph,
                &node_id,
                max_depth,
                max_chains,
                &edge_filter,
                false, // upstream
                &mut upstream_chains,
            );
        }

        if direction == "downstream" || direction == "both" {
            build_chains_pet(
                &graph,
                &node_id,
                max_depth,
                max_chains,
                &edge_filter,
                true, // downstream
                &mut downstream_chains,
            );
        }

        // Format chains
        let format_chains =
            |chains: &[Vec<String>], graph: &PetCodeGraph| -> Vec<serde_json::Value> {
                chains
                    .iter()
                    .map(|chain| {
                        let nodes: Vec<serde_json::Value> = chain
                            .iter()
                            .filter_map(|id| graph.get_node(id))
                            .map(|n| {
                                serde_json::json!({
                                    "id": n.id,
                                    "name": n.name,
                                    "type": n.node_type.as_str(),
                                    "file": n.file,
                                    "line": n.line,
                                })
                            })
                            .collect();
                        serde_json::json!({
                            "length": chain.len(),
                            "nodes": nodes,
                        })
                    })
                    .collect()
            };

        let response = if direction == "both" {
            serde_json::json!({
                "node_id": node_id,
                "node_name": node_name,
                "direction": direction,
                "max_depth": max_depth,
                "chains": {
                    "upstream": format_chains(&upstream_chains, &graph),
                    "downstream": format_chains(&downstream_chains, &graph),
                },
                "chain_count": {
                    "upstream": upstream_chains.len(),
                    "downstream": downstream_chains.len(),
                },
            })
        } else {
            let chains = if direction == "upstream" {
                &upstream_chains
            } else {
                &downstream_chains
            };
            serde_json::json!({
                "node_id": node_id,
                "node_name": node_name,
                "direction": direction,
                "max_depth": max_depth,
                "chains": format_chains(chains, &graph),
                "chain_count": chains.len(),
            })
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap_or_default(),
        )]))
    }

    #[tool(
        name = "find_module_structure",
        description = "Explore directory organization and get entity counts by type. Useful for understanding unfamiliar codebases before diving into specific files. Returns hierarchy with file counts and node type breakdowns."
    )]
    async fn find_module_structure(
        &self,
        Parameters(params): Parameters<FindModuleStructureParams>,
    ) -> Result<CallToolResult, McpError> {
        let base_path = params.base_path;
        let max_depth = params.max_depth.unwrap_or(2).clamp(1, 3);
        let node_types = params.node_types;
        let include_empty = params.include_empty.unwrap_or(false);

        debug!(
            "find_module_structure: base_path='{}', max_depth={}",
            base_path, max_depth
        );

        // Use read lock since we only iterate over currently loaded nodes
        let state = self.state.read().await;

        // Normalize base path
        let base_path_normalized = if base_path.ends_with('/') {
            base_path.clone()
        } else {
            format!("{}/", base_path)
        };

        let type_filter: HashSet<&str> = node_types
            .as_ref()
            .map(|types| types.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        // Collect module structure from loaded nodes
        // Note: This analyzes currently loaded partitions; use search first to load relevant data
        let mut modules: HashMap<String, ModuleInfo> = HashMap::new();
        let mut total_nodes = 0;

        // Acquire read lock for graph iteration
        let graph = state.lazy_graph.graph_read();
        for node in graph.iter_nodes() {
            // Check if node is in the specified directory
            if !node.file.starts_with(&base_path_normalized) {
                continue;
            }

            // Filter by node type
            if !type_filter.is_empty() && !type_filter.contains(node.node_type.as_str()) {
                continue;
            }

            // Get relative path
            let relative_path = &node.file[base_path_normalized.len()..];
            let path_parts: Vec<&str> = relative_path.split('/').collect();

            // Build directory paths up to max_depth
            for depth in 1..=max_depth.min(path_parts.len()) {
                let dir_path = path_parts[..depth].join("/");

                let module = modules
                    .entry(dir_path.clone())
                    .or_insert_with(|| ModuleInfo {
                        path: dir_path,
                        depth,
                        node_count: 0,
                        nodes_by_type: HashMap::new(),
                        files: HashSet::new(),
                    });

                module.node_count += 1;
                *module
                    .nodes_by_type
                    .entry(node.node_type.as_str().to_string())
                    .or_insert(0) += 1;
                module.files.insert(node.file.clone());
            }

            total_nodes += 1;
        }

        // Format output
        let mut module_list: Vec<serde_json::Value> = modules
            .into_iter()
            .filter(|(_, info)| include_empty || info.node_count > 0)
            .map(|(_, info)| {
                serde_json::json!({
                    "path": info.path,
                    "depth": info.depth,
                    "node_count": info.node_count,
                    "file_count": info.files.len(),
                    "nodes_by_type": info.nodes_by_type,
                })
            })
            .collect();

        // Sort by path
        module_list.sort_by(|a, b| {
            a["path"]
                .as_str()
                .unwrap_or("")
                .cmp(b["path"].as_str().unwrap_or(""))
        });

        let response = serde_json::json!({
            "base_path": base_path_normalized,
            "max_depth": max_depth,
            "module_count": module_list.len(),
            "total_nodes": total_nodes,
            "modules": module_list,
        });

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap_or_default(),
        )]))
    }

    #[tool(
        name = "sync_repository",
        description = "Trigger re-indexing after code changes. Call when search returns outdated results or after editing files. Runs in background - use get_index_status to check completion."
    )]
    async fn sync_repository(
        &self,
        Parameters(_params): Parameters<SyncRepositoryParams>,
    ) -> Result<CallToolResult, McpError> {
        info!("Manual repository sync triggered");

        // Check if already syncing/indexing
        {
            let index_state = self.index_state.read().await;
            if index_state.is_indexing() {
                return Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "already_syncing",
                        "message": "Sync/indexing is already in progress. Use get_index_status to check progress.",
                        "progress": {
                            "indexed": index_state.progress.0,
                            "total": index_state.progress.1,
                        },
                    }))
                    .unwrap_or_default(),
                )]));
            }
        }

        // Mark as indexing immediately (we're starting background work)
        {
            let mut index_state = self.index_state.write().await;
            index_state.status = IndexStatus::Indexing {
                started_at: Instant::now(),
            };
        }

        // Clone what we need for background task
        let updater = match &self.updater {
            Some(u) => Arc::clone(u),
            None => {
                // Reset indexing status
                let mut idx = self.index_state.write().await;
                idx.status = IndexStatus::Idle;
                return Ok(CallToolResult::success(vec![Content::text(
                    serde_json::to_string_pretty(&serde_json::json!({
                        "status": "error",
                        "message": "Incremental updater not available. Cannot sync.",
                    }))
                    .unwrap_or_default(),
                )]));
            }
        };
        let state = Arc::clone(&self.state);
        let index_state = Arc::clone(&self.index_state);
        let has_searcher = self.searcher.is_some();

        // Spawn the entire sync operation in background
        tokio::spawn(background_sync_task(
            state,
            updater,
            index_state,
            has_searcher,
        ));

        // Return immediately
        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&serde_json::json!({
                "status": "started",
                "message": "Background sync started. Use get_index_status to check progress.",
            }))
            .unwrap_or_default(),
        )]))
    }

    #[tool(
        name = "get_index_status",
        description = "Check indexing status and progress. Shows: idle (ready), indexing (in progress), or failed. Use after sync_repository to confirm completion. Also shows if index needs refresh."
    )]
    async fn get_index_status(
        &self,
        Parameters(_params): Parameters<GetIndexStatusParams>,
    ) -> Result<CallToolResult, McpError> {
        debug!("get_index_status called");

        let index_state = self.index_state.read().await;

        // Get Qdrant stats (no state lock needed - searcher is separate)
        let qdrant_stats = if let Some(ref searcher) = self.searcher {
            match searcher.index_status().await {
                Ok(Some((semantic, code))) => Some(serde_json::json!({
                    "semantic_points": semantic,
                    "code_points": code,
                })),
                Ok(None) => Some(serde_json::json!({
                    "error": "collections do not exist"
                })),
                Err(e) => Some(serde_json::json!({
                    "error": format!("{}", e)
                })),
            }
        } else {
            None
        };

        // Build status response
        let status_str = index_state.status.as_str();
        let mut response = serde_json::json!({
            "status": status_str,
            "graph_version": index_state.graph_version,
            "indexed_version": index_state.indexed_version,
            "needs_reindex": index_state.needs_reindex(),
            "progress": {
                "indexed": index_state.progress.0,
                "total": index_state.progress.1,
            },
        });

        // Add timing info for indexing status
        if let IndexStatus::Indexing { started_at } = &index_state.status {
            let elapsed = started_at.elapsed();
            response["elapsed_secs"] = serde_json::json!(elapsed.as_secs());
        }

        // Add last error if failed
        if let IndexStatus::Failed { error } = &index_state.status {
            response["error"] = serde_json::json!(error);
        }

        if let Some(ref last_error) = index_state.last_error {
            response["last_error"] = serde_json::json!(last_error);
        }

        // Add Qdrant stats
        if let Some(stats) = qdrant_stats {
            response["qdrant"] = stats;
        }

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&response).unwrap_or_default(),
        )]))
    }
}

// Implement ServerHandler for tool routing
#[tool_handler]
impl rmcp::ServerHandler for PrismServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "CodePrysm: Code graph and semantic search for AI assistants.\n\n\
                TOOLS:\n\
                - search_graph_nodes: Find code by name or description (start here)\n\
                - get_node_info: Get metadata (type, file, lines) for a node ID\n\
                - read_code: View source code for a node or file range\n\
                - find_references: Who calls/uses this? (incoming edges)\n\
                - find_outgoing_references: What does this call/use? (dependencies)\n\
                - find_definitions: What does this contain? (methods, fields)\n\
                - find_call_chain: Trace execution paths (upstream/downstream)\n\
                - find_module_structure: Explore directory organization\n\
                - sync_repository / get_index_status: Keep index current\n\n\
                NODE IDs: Format is 'file_path:entity_name' (e.g., 'src/main.rs:main', 'app/user.py:User').\n\
                NODE TYPES: Container (files, classes, modules), Callable (functions, methods), Data (fields, variables).\n\n\
                WORKFLOW: search_graph_nodes → get_node_info → read_code → find_references/find_outgoing_references"
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Format node info as JSON
fn format_node_info(node: &Node) -> serde_json::Value {
    let mut info = serde_json::json!({
        "id": node.id,
        "name": node.name,
        "type": node.node_type.as_str(),
        "file": node.file,
        "line": node.line,
        "end_line": node.end_line,
    });

    if let Some(ref kind) = node.kind {
        info["kind"] = serde_json::json!(kind);
    }
    if let Some(ref subtype) = node.subtype {
        info["subtype"] = serde_json::json!(subtype);
    }

    info
}

/// Truncate code snippet for display
fn truncate_snippet(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len])
    }
}

/// Module info for find_module_structure
struct ModuleInfo {
    path: String,
    depth: usize,
    node_count: usize,
    nodes_by_type: HashMap<String, usize>,
    files: HashSet<String>,
}

/// Build call chains recursively using PetCodeGraph
fn build_chains_pet(
    graph: &PetCodeGraph,
    start_id: &str,
    max_depth: usize,
    max_chains: usize,
    edge_filter: &HashSet<String>,
    downstream: bool,
    chains: &mut Vec<Vec<String>>,
) {
    let mut visited = HashSet::new();
    let mut current_chain = vec![start_id.to_string()];

    build_chains_recursive_pet(
        graph,
        start_id,
        0,
        max_depth,
        max_chains,
        edge_filter,
        downstream,
        &mut visited,
        &mut current_chain,
        chains,
    );
}

#[allow(clippy::too_many_arguments)]
fn build_chains_recursive_pet(
    graph: &PetCodeGraph,
    current_id: &str,
    depth: usize,
    max_depth: usize,
    max_chains: usize,
    edge_filter: &HashSet<String>,
    downstream: bool,
    visited: &mut HashSet<String>,
    current_chain: &mut Vec<String>,
    chains: &mut Vec<Vec<String>>,
) {
    if chains.len() >= max_chains || depth >= max_depth {
        if current_chain.len() > 1 {
            chains.push(current_chain.clone());
        }
        return;
    }

    visited.insert(current_id.to_string());

    // Get edges from PetCodeGraph - returns (Node, EdgeData) tuples
    let edges: Vec<_> = if downstream {
        graph.outgoing_edges(current_id).collect()
    } else {
        graph.incoming_edges(current_id).collect()
    };

    let mut found_edge = false;

    for (node, edge_data) in edges {
        if !edge_filter.contains(edge_data.edge_type.as_str()) {
            continue;
        }

        let next_id = &node.id;
        if next_id == current_id || visited.contains(next_id) {
            continue;
        }

        found_edge = true;
        current_chain.push(next_id.clone());

        build_chains_recursive_pet(
            graph,
            next_id,
            depth + 1,
            max_depth,
            max_chains,
            edge_filter,
            downstream,
            &mut visited.clone(),
            current_chain,
            chains,
        );

        current_chain.pop();

        if chains.len() >= max_chains {
            return;
        }
    }

    // Terminal node - save chain if it has more than starting node
    if !found_edge && current_chain.len() > 1 {
        chains.push(current_chain.clone());
    }
}

/// Background task for automatic repository synchronization
///
/// The updater is passed separately to avoid holding the state lock during
/// the (slow) update_repository operation.
async fn auto_sync_task(
    state: Arc<RwLock<ServerState>>,
    updater: Arc<tokio::sync::Mutex<IncrementalUpdater>>,
    interval_secs: u64,
    mut shutdown_rx: watch::Receiver<bool>,
) {
    let mut ticker = tokio::time::interval(Duration::from_secs(interval_secs));

    // Skip the first immediate tick
    ticker.tick().await;

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                debug!("Auto-sync: checking for repository changes");

                // Step 1: Run update_repository WITHOUT state lock
                let update_result = {
                    let mut updater_guard = updater.lock().await;
                    match updater_guard.update_repository(false) {
                        Ok(result) => Some(result),
                        Err(e) => {
                            warn!("Auto-sync: update failed: {}", e);
                            None
                        }
                    }
                }; // Updater lock released

                // Step 2: If changes detected, take brief state lock to reload
                if let Some(result) = update_result {
                    if result.has_changes() {
                        info!(
                            "Auto-sync: detected changes (added: {}, modified: {}, deleted: {})",
                            result.changes.added.len(),
                            result.changes.modified.len(),
                            result.changes.deleted.len()
                        );

                        // Brief write lock only to reload manifest/cross-refs
                        let mut state_guard = state.write().await;

                        if let Err(e) = state_guard.lazy_graph.reload_manifest() {
                            warn!("Auto-sync: failed to reload manifest: {}", e);
                        }
                        if let Err(e) = state_guard.lazy_graph.reload_cross_refs() {
                            warn!("Auto-sync: failed to reload cross-refs: {}", e);
                        }

                        let stats = state_guard.lazy_graph.stats();
                        info!(
                            "Auto-sync: reloaded lazy graph ({} partitions, {} loaded)",
                            stats.total_partitions, stats.loaded_partitions
                        );
                    } else {
                        debug!("Auto-sync: no changes detected");
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                if *shutdown_rx.borrow() {
                    info!("Auto-sync: shutdown signal received, stopping");
                    break;
                }
            }
        }
    }
}

/// Background task for full sync operation (graph update + indexing)
///
/// This runs the entire sync operation in a background task to avoid blocking
/// the MCP tool response. The updater is passed separately to avoid holding
/// the state lock during the (slow) update_repository operation.
async fn background_sync_task(
    state: Arc<RwLock<ServerState>>,
    updater: Arc<tokio::sync::Mutex<IncrementalUpdater>>,
    index_state: Arc<RwLock<IndexState>>,
    has_searcher: bool,
) {
    info!("Background sync task started");

    // Step 1: Update repository (detect changes, rebuild graph)
    // This runs WITHOUT holding the state lock - updater is separate
    let update_result = {
        let mut updater_guard = updater.lock().await;
        match updater_guard.update_repository(false) {
            Ok(result) => {
                info!(
                    "Sync detected: {} added, {} modified, {} deleted",
                    result.changes.added.len(),
                    result.changes.modified.len(),
                    result.changes.deleted.len()
                );
                result
            }
            Err(e) => {
                warn!("Sync failed: {}", e);
                let mut idx = index_state.write().await;
                idx.status = IndexStatus::Failed {
                    error: format!("Sync failed: {}", e),
                };
                idx.last_error = Some(format!("Sync failed: {}", e));
                return;
            }
        }
    }; // Updater lock released - queries can now run concurrently

    // Step 2: Brief write lock to reload manifest/cross-refs and extract data for indexing
    let sync_result = {
        let mut state_guard = state.write().await;

        // If changes occurred, reload manifest and cross-refs
        if update_result.has_changes() {
            if let Err(e) = state_guard.lazy_graph.reload_manifest() {
                warn!("Failed to reload manifest: {}", e);
            }
            if let Err(e) = state_guard.lazy_graph.reload_cross_refs() {
                warn!("Failed to reload cross-refs: {}", e);
            }
        }

        // Clone data needed for indexing
        let qdrant_config = state_guard.qdrant_config.clone();
        let repo_id = state_guard.repo_id.clone();
        let repo_path = state_guard.repo_path.clone();
        let codeprysm_dir = state_guard.codeprysm_dir.clone();

        // Compute graph version
        let graph_version = compute_graph_version(&codeprysm_dir);

        // Load partitions and clone graph only if there are changes to index
        let graph_clone = if has_searcher && update_result.has_changes() {
            match state_guard.lazy_graph.load_all_partitions() {
                Ok(loaded) => {
                    info!("Loaded {} partitions for indexing", loaded);
                    Some(state_guard.lazy_graph.graph_read().clone())
                }
                Err(e) => {
                    warn!("Failed to load partitions: {}", e);
                    None
                }
            }
        } else {
            None
        };

        (
            update_result,
            qdrant_config,
            repo_id,
            repo_path,
            codeprysm_dir,
            graph_version,
            graph_clone,
        )
    }; // Write lock released - queries can resume

    // Step 3: Index changes if needed (no state lock held)
    let (
        update_result,
        qdrant_config,
        repo_id,
        repo_path,
        codeprysm_dir,
        graph_version,
        graph_clone,
    ) = sync_result;
    if let Some(graph) = graph_clone {
        // Update graph version in index state
        {
            let mut idx = index_state.write().await;
            idx.graph_version = graph_version.clone();
            idx.progress = (0, graph.node_count());
        }

        // Create indexer and index
        match GraphIndexer::new(qdrant_config, &repo_id, &repo_path).await {
            Ok(mut indexer) => {
                let use_incremental =
                    !update_result.was_full_rebuild && update_result.changes.has_changes();

                let index_result = if use_incremental {
                    info!("Starting incremental indexing...");
                    indexer.index_changes(&graph, &update_result.changes).await
                } else {
                    info!("Starting full indexing...");
                    indexer.index_graph(&graph).await
                };

                match index_result {
                    Ok(stats) => {
                        info!(
                            "Indexing complete: {} indexed ({} semantic, {} code)",
                            stats.total_indexed, stats.semantic_indexed, stats.code_indexed
                        );

                        // Save indexed version
                        if let Some(ref version) = graph_version {
                            if let Err(e) = save_indexed_version(&codeprysm_dir, version) {
                                warn!("Failed to save indexed version: {}", e);
                            }
                        }

                        // Update state
                        let mut idx = index_state.write().await;
                        idx.status = IndexStatus::Idle;
                        idx.progress = (stats.total_indexed, stats.total_processed);
                        idx.last_indexed_at = Some(Instant::now());
                        idx.last_error = None;
                        idx.indexed_version = graph_version;
                    }
                    Err(e) => {
                        let error_msg = format!("Indexing failed: {}", e);
                        warn!("{}", error_msg);
                        let mut idx = index_state.write().await;
                        idx.status = IndexStatus::Failed {
                            error: error_msg.clone(),
                        };
                        idx.last_error = Some(error_msg);
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Failed to create indexer: {}", e);
                warn!("{}", error_msg);
                let mut idx = index_state.write().await;
                idx.status = IndexStatus::Failed {
                    error: error_msg.clone(),
                };
                idx.last_error = Some(error_msg);
            }
        }
    } else {
        // No changes to index, mark as complete
        info!("No changes detected, sync complete");
        let mut idx = index_state.write().await;
        idx.status = IndexStatus::Idle;
        idx.last_indexed_at = Some(Instant::now());
    }

    info!("Background sync task completed");
}
