//! Lazy Graph Manager
//!
//! Orchestrates lazy-loading of code graph partitions from SQLite into petgraph.
//! Provides transparent access to nodes and edges, loading partitions on-demand.

use crate::discovery::{DiscoveredRoot, DiscoveryError, RootDiscovery, RootType};
use crate::graph::{EdgeData, Node, PetCodeGraph};
use crate::lazy::cache::{CacheMetrics, MemoryBudgetCache, PartitionStats as CachePartitionStats};
use crate::lazy::cross_refs::{CrossRef, CrossRefError, CrossRefIndex, CrossRefStore};
use crate::lazy::partition::{PartitionConnection, PartitionError};
use dashmap::{DashMap, DashSet};
use parking_lot::RwLock;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur during lazy graph operations
#[derive(Debug, Error)]
pub enum LazyGraphError {
    #[error("Partition error: {0}")]
    Partition(#[from] PartitionError),

    #[error("Manifest error: {0}")]
    Manifest(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Partition not found: {0}")]
    PartitionNotFound(String),

    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("Discovery error: {0}")]
    Discovery(#[from] DiscoveryError),

    #[error("Cross-ref error: {0}")]
    CrossRef(#[from] CrossRefError),
}

/// Manifest entry for a file's partition assignment
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ManifestEntry {
    /// The partition ID this file belongs to
    pub partition_id: String,
    /// Content hash of the file (for change detection)
    pub content_hash: Option<String>,
}

/// Information about a discovered code root
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RootInfo {
    /// Unique name for this root (typically directory name)
    pub name: String,
    /// Type of root: "git" or "code"
    pub root_type: String,
    /// Relative path from workspace root
    pub relative_path: String,
    /// Remote URL for git repositories
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_url: Option<String>,
    /// Branch name for git repositories
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// Commit SHA for git repositories
    #[serde(skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
}

impl RootInfo {
    /// Create a RootInfo from a DiscoveredRoot
    pub fn from_discovered_root(discovered: &DiscoveredRoot) -> Self {
        let (root_type, remote_url, branch, commit) = match &discovered.root_type {
            RootType::GitRepository {
                remote,
                branch,
                commit,
            } => (
                "git".to_string(),
                remote.clone(),
                branch.clone(),
                commit.clone(),
            ),
            RootType::CodeDirectory => ("code".to_string(), None, None, None),
        };

        Self {
            name: discovered.name.clone(),
            root_type,
            relative_path: discovered.relative_path.clone(),
            remote_url,
            branch,
            commit,
        }
    }
}

/// Manifest that maps files to partitions
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct Manifest {
    /// Schema version for compatibility
    pub schema_version: String,
    /// Map from root name to root info (multi-root support)
    #[serde(default)]
    pub roots: HashMap<String, RootInfo>,
    /// Map from file path to manifest entry
    pub files: HashMap<String, ManifestEntry>,
    /// Map from partition ID to partition file name
    pub partitions: HashMap<String, String>,
}

impl Manifest {
    /// Create a new empty manifest
    pub fn new() -> Self {
        Self {
            schema_version: "1.0".to_string(),
            roots: HashMap::new(),
            files: HashMap::new(),
            partitions: HashMap::new(),
        }
    }

    /// Load manifest from a JSON file
    pub fn load(path: &Path) -> Result<Self, LazyGraphError> {
        let content = std::fs::read_to_string(path)?;
        let manifest: Manifest = serde_json::from_str(&content)?;
        Ok(manifest)
    }

    /// Save manifest to a JSON file
    pub fn save(&self, path: &Path) -> Result<(), LazyGraphError> {
        let content = serde_json::to_string_pretty(self)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get the partition ID for a file
    pub fn get_partition_for_file(&self, file: &str) -> Option<&str> {
        self.files.get(file).map(|e| e.partition_id.as_str())
    }

    /// Get the partition database file name
    pub fn get_partition_file(&self, partition_id: &str) -> Option<&str> {
        self.partitions.get(partition_id).map(|s| s.as_str())
    }

    /// Add or update a file entry
    pub fn set_file(&mut self, file: String, partition_id: String, content_hash: Option<String>) {
        self.files.insert(
            file,
            ManifestEntry {
                partition_id,
                content_hash,
            },
        );
    }

    /// Register a partition
    pub fn register_partition(&mut self, partition_id: String, filename: String) {
        self.partitions.insert(partition_id, filename);
    }

    /// Register a root
    pub fn register_root(&mut self, root_info: RootInfo) {
        self.roots.insert(root_info.name.clone(), root_info);
    }

    /// Get a root by name
    pub fn get_root(&self, name: &str) -> Option<&RootInfo> {
        self.roots.get(name)
    }

    /// Get all root names
    pub fn root_names(&self) -> impl Iterator<Item = &str> {
        self.roots.keys().map(|s| s.as_str())
    }

    /// Check if this is a multi-root workspace
    pub fn is_multi_root(&self) -> bool {
        self.roots.len() > 1
    }

    /// Get the number of roots
    pub fn root_count(&self) -> usize {
        self.roots.len()
    }
}

/// Registry tracking which nodes belong to which partition
///
/// Uses interior mutability via DashMap/DashSet for thread-safe concurrent access.
/// All methods take `&self` to enable concurrent operations without external locking.
struct PartitionRegistry {
    /// Map from partition ID to the set of node IDs in that partition
    partition_to_nodes: DashMap<String, HashSet<String>>,
    /// Map from node ID to the partition it belongs to
    node_to_partition: DashMap<String, String>,
    /// Set of currently loaded partitions
    loaded_partitions: DashSet<String>,
    /// Per-partition loading locks to prevent duplicate concurrent loads
    /// Uses Arc<Mutex> so we can clone and lock without holding the DashMap entry
    loading_locks: DashMap<String, Arc<parking_lot::Mutex<()>>>,
}

impl Default for PartitionRegistry {
    fn default() -> Self {
        Self {
            partition_to_nodes: DashMap::new(),
            node_to_partition: DashMap::new(),
            loaded_partitions: DashSet::new(),
            loading_locks: DashMap::new(),
        }
    }
}

impl PartitionRegistry {
    fn new() -> Self {
        Self::default()
    }

    /// Check if a partition is currently loaded
    fn is_loaded(&self, partition_id: &str) -> bool {
        self.loaded_partitions.contains(partition_id)
    }

    /// Mark a partition as loaded and register its nodes
    fn register_loaded(&self, partition_id: &str, node_ids: Vec<String>) {
        self.loaded_partitions.insert(partition_id.to_string());
        let nodes_set: HashSet<String> = node_ids.iter().cloned().collect();
        for node_id in &node_ids {
            self.node_to_partition
                .insert(node_id.clone(), partition_id.to_string());
        }
        self.partition_to_nodes
            .insert(partition_id.to_string(), nodes_set);
    }

    /// Mark a partition as unloaded and remove its node registrations
    fn unregister(&self, partition_id: &str) -> Option<HashSet<String>> {
        self.loaded_partitions.remove(partition_id);
        if let Some((_, nodes)) = self.partition_to_nodes.remove(partition_id) {
            for node_id in &nodes {
                self.node_to_partition.remove(node_id);
            }
            Some(nodes)
        } else {
            None
        }
    }

    /// Get the partition a node belongs to (if known)
    ///
    /// Returns owned String because DashMap doesn't support returning references.
    fn get_node_partition(&self, node_id: &str) -> Option<String> {
        self.node_to_partition.get(node_id).map(|r| r.clone())
    }

    /// Get the set of loaded partition IDs
    fn loaded_partition_ids(&self) -> Vec<String> {
        self.loaded_partitions.iter().map(|r| r.clone()).collect()
    }

    /// Get count of loaded partitions
    fn loaded_count(&self) -> usize {
        self.loaded_partitions.len()
    }

    /// Get node IDs belonging to a loaded partition
    fn get_node_ids(&self, partition_id: &str) -> Option<Vec<String>> {
        self.partition_to_nodes
            .get(partition_id)
            .map(|r| r.iter().cloned().collect())
    }

    /// Get or create a loading lock for a partition
    ///
    /// Used for double-checked locking during partition loading.
    /// Returns the Arc<Mutex> which the caller can lock.
    fn get_loading_lock(&self, partition_id: &str) -> Arc<parking_lot::Mutex<()>> {
        self.loading_locks
            .entry(partition_id.to_string())
            .or_insert_with(|| Arc::new(parking_lot::Mutex::new(())))
            .clone()
    }
}

/// The lazy-loading graph manager
///
/// Manages partitioned code graphs stored in SQLite, loading them on-demand
/// into a petgraph-based in-memory representation.
///
/// Uses interior mutability via `RwLock<PetCodeGraph>` to enable concurrent
/// read access to the graph while maintaining thread safety for writes.
pub struct LazyGraphManager {
    /// The petgraph instance holding all loaded nodes/edges
    /// Protected by RwLock for concurrent read access during queries
    graph: RwLock<PetCodeGraph>,

    /// Registry tracking loaded partitions and node ownership
    registry: PartitionRegistry,

    /// Manifest for partition lookups
    manifest: Manifest,

    /// Memory budget cache for partition eviction
    cache: MemoryBudgetCache,

    /// Cross-partition edge index (always in memory)
    cross_refs: CrossRefIndex,

    /// Base directory for partition storage (.codeprysm/partitions/)
    partitions_dir: PathBuf,

    /// Path to manifest file
    manifest_path: PathBuf,

    /// Path to cross_refs.db
    cross_refs_path: PathBuf,
}

impl LazyGraphManager {
    /// Create a new lazy graph manager with default memory budget (512 MB)
    ///
    /// # Arguments
    /// * `prism_dir` - The .codeprysm directory path
    pub fn new(prism_dir: &Path) -> Self {
        Self::with_memory_budget(prism_dir, None)
    }

    /// Create a new lazy graph manager with a custom memory budget
    ///
    /// # Arguments
    /// * `prism_dir` - The .codeprysm directory path
    /// * `memory_budget_bytes` - Optional memory budget in bytes (default: 512 MB)
    pub fn with_memory_budget(prism_dir: &Path, memory_budget_bytes: Option<usize>) -> Self {
        let partitions_dir = prism_dir.join("partitions");
        let manifest_path = prism_dir.join("manifest.json");
        let cross_refs_path = prism_dir.join("cross_refs.db");

        let cache = match memory_budget_bytes {
            Some(bytes) => MemoryBudgetCache::new(bytes),
            None => MemoryBudgetCache::with_default_budget(),
        };

        Self {
            graph: RwLock::new(PetCodeGraph::new()),
            registry: PartitionRegistry::new(),
            manifest: Manifest::new(),
            cache,
            cross_refs: CrossRefIndex::new(),
            partitions_dir,
            manifest_path,
            cross_refs_path,
        }
    }

    /// Open an existing lazy graph from a .codeprysm directory
    ///
    /// Loads the manifest but does not load any partitions yet.
    pub fn open(prism_dir: &Path) -> Result<Self, LazyGraphError> {
        Self::open_with_memory_budget(prism_dir, None)
    }

    /// Open an existing lazy graph with a custom memory budget
    pub fn open_with_memory_budget(
        prism_dir: &Path,
        memory_budget_bytes: Option<usize>,
    ) -> Result<Self, LazyGraphError> {
        let partitions_dir = prism_dir.join("partitions");
        let manifest_path = prism_dir.join("manifest.json");
        let cross_refs_path = prism_dir.join("cross_refs.db");

        let manifest = if manifest_path.exists() {
            Manifest::load(&manifest_path)?
        } else {
            Manifest::new()
        };

        // Load cross-refs from SQLite if database exists
        let cross_refs = if cross_refs_path.exists() {
            let store = CrossRefStore::open(&cross_refs_path)?;
            store.load_all()?
        } else {
            CrossRefIndex::new()
        };

        let cache = match memory_budget_bytes {
            Some(bytes) => MemoryBudgetCache::new(bytes),
            None => MemoryBudgetCache::with_default_budget(),
        };

        Ok(Self {
            graph: RwLock::new(PetCodeGraph::new()),
            registry: PartitionRegistry::new(),
            manifest,
            cache,
            cross_refs,
            partitions_dir,
            manifest_path,
            cross_refs_path,
        })
    }

    /// Initialize a new lazy graph with an empty manifest
    ///
    /// Creates the partitions directory if it doesn't exist.
    pub fn init(prism_dir: &Path) -> Result<Self, LazyGraphError> {
        Self::init_with_memory_budget(prism_dir, None)
    }

    /// Initialize a new lazy graph with a custom memory budget
    pub fn init_with_memory_budget(
        prism_dir: &Path,
        memory_budget_bytes: Option<usize>,
    ) -> Result<Self, LazyGraphError> {
        let partitions_dir = prism_dir.join("partitions");
        std::fs::create_dir_all(&partitions_dir)?;

        let manifest_path = prism_dir.join("manifest.json");
        let cross_refs_path = prism_dir.join("cross_refs.db");

        let manifest = Manifest::new();
        manifest.save(&manifest_path)?;

        // Create empty cross_refs.db
        CrossRefStore::create(&cross_refs_path)?;

        let cache = match memory_budget_bytes {
            Some(bytes) => MemoryBudgetCache::new(bytes),
            None => MemoryBudgetCache::with_default_budget(),
        };

        Ok(Self {
            graph: RwLock::new(PetCodeGraph::new()),
            registry: PartitionRegistry::new(),
            manifest,
            cache,
            cross_refs: CrossRefIndex::new(),
            partitions_dir,
            manifest_path,
            cross_refs_path,
        })
    }

    /// Initialize a lazy graph by discovering roots in a workspace
    ///
    /// Uses `RootDiscovery` to find git repositories and code directories,
    /// then registers them in the manifest for multi-root support.
    ///
    /// # Arguments
    /// * `workspace_path` - The workspace root to discover roots in
    /// * `prism_dir` - The .codeprysm directory for storage
    ///
    /// # Examples
    /// ```no_run
    /// use codeprysm_core::lazy::manager::LazyGraphManager;
    /// use std::path::Path;
    ///
    /// // Single repo workspace
    /// let manager = LazyGraphManager::init_workspace(
    ///     Path::new("/path/to/repo"),
    ///     Path::new("/path/to/repo/.codeprysm"),
    /// ).unwrap();
    ///
    /// // Multi-root workspace
    /// let manager = LazyGraphManager::init_workspace(
    ///     Path::new("/path/to/workspace"),
    ///     Path::new("/path/to/workspace/.codeprysm"),
    /// ).unwrap();
    /// ```
    pub fn init_workspace(workspace_path: &Path, prism_dir: &Path) -> Result<Self, LazyGraphError> {
        Self::init_workspace_with_options(workspace_path, prism_dir, None, None)
    }

    /// Initialize a lazy graph with custom options
    ///
    /// # Arguments
    /// * `workspace_path` - The workspace root to discover roots in
    /// * `prism_dir` - The .codeprysm directory for storage
    /// * `memory_budget_bytes` - Optional memory budget (default: 512 MB)
    /// * `max_discovery_depth` - Optional max depth for root discovery (default: 3)
    pub fn init_workspace_with_options(
        workspace_path: &Path,
        prism_dir: &Path,
        memory_budget_bytes: Option<usize>,
        max_discovery_depth: Option<usize>,
    ) -> Result<Self, LazyGraphError> {
        // Create directory structure
        let partitions_dir = prism_dir.join("partitions");
        std::fs::create_dir_all(&partitions_dir)?;

        // Discover roots
        let discovery = match max_discovery_depth {
            Some(depth) => RootDiscovery::with_defaults().with_max_depth(depth),
            None => RootDiscovery::with_defaults(),
        };

        let discovered_roots = discovery.discover(workspace_path)?;

        // Create manifest with discovered roots
        let mut manifest = Manifest::new();
        for discovered in &discovered_roots {
            let root_info = RootInfo::from_discovered_root(discovered);
            manifest.register_root(root_info);
        }

        // Save manifest
        let manifest_path = prism_dir.join("manifest.json");
        manifest.save(&manifest_path)?;

        // Create empty cross_refs.db
        let cross_refs_path = prism_dir.join("cross_refs.db");
        CrossRefStore::create(&cross_refs_path)?;

        // Create cache
        let cache = match memory_budget_bytes {
            Some(bytes) => MemoryBudgetCache::new(bytes),
            None => MemoryBudgetCache::with_default_budget(),
        };

        Ok(Self {
            graph: RwLock::new(PetCodeGraph::new()),
            registry: PartitionRegistry::new(),
            manifest,
            cache,
            cross_refs: CrossRefIndex::new(),
            partitions_dir,
            manifest_path,
            cross_refs_path,
        })
    }

    /// Get the discovered roots from the manifest
    pub fn roots(&self) -> impl Iterator<Item = &RootInfo> {
        self.manifest.roots.values()
    }

    /// Check if this is a multi-root workspace
    pub fn is_multi_root(&self) -> bool {
        self.manifest.is_multi_root()
    }

    // =========================================================================
    // Manifest Operations
    // =========================================================================

    /// Get the manifest
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Get mutable access to the manifest
    pub fn manifest_mut(&mut self) -> &mut Manifest {
        &mut self.manifest
    }

    /// Reload manifest from disk
    pub fn reload_manifest(&mut self) -> Result<(), LazyGraphError> {
        if self.manifest_path.exists() {
            self.manifest = Manifest::load(&self.manifest_path)?;
        }
        Ok(())
    }

    /// Save manifest to disk
    pub fn save_manifest(&self) -> Result<(), LazyGraphError> {
        self.manifest.save(&self.manifest_path)
    }

    // =========================================================================
    // Partition Lookup
    // =========================================================================

    /// Get the partition ID for a file path
    pub fn get_partition_for_file(&self, file: &str) -> Option<&str> {
        self.manifest.get_partition_for_file(file)
    }

    /// Get the partition ID for a node by looking at its file path
    ///
    /// Returns owned String since registry uses interior mutability (DashMap).
    pub fn get_partition_for_node(&self, node_id: &str) -> Option<String> {
        // First check if we already know this node's partition
        if let Some(partition) = self.registry.get_node_partition(node_id) {
            return Some(partition);
        }

        // Otherwise, extract file from node ID and look up in manifest
        // Node IDs are typically "file.py:ClassName:method_name"
        let file = node_id.split(':').next()?;
        self.manifest
            .get_partition_for_file(file)
            .map(|s| s.to_string())
    }

    /// Compute the partition ID for a file path (directory-based partitioning)
    ///
    /// For multi-root workspaces, use `compute_partition_id_for_root` instead.
    /// Returns the parent directory path as the partition ID.
    #[deprecated(note = "Use compute_partition_id_for_root for multi-root support")]
    pub fn compute_partition_id(file: &str) -> String {
        Self::compute_directory_partition(file)
    }

    /// Compute partition ID for a file in a specific root (multi-root support)
    ///
    /// Returns partition ID in format `{root_name}_{directory}` to avoid
    /// collisions between roots with similar directory structures.
    ///
    /// # Examples
    /// ```
    /// use codeprysm_core::lazy::manager::LazyGraphManager;
    ///
    /// let pid = LazyGraphManager::compute_partition_id_for_root("repo-a", "src/core/main.py");
    /// assert_eq!(pid, "repo-a_src/core");
    ///
    /// let pid = LazyGraphManager::compute_partition_id_for_root("repo-b", "src/core/main.py");
    /// assert_eq!(pid, "repo-b_src/core");
    ///
    /// // Root-level files use "{root}_root" partition
    /// let pid = LazyGraphManager::compute_partition_id_for_root("myrepo", "main.py");
    /// assert_eq!(pid, "myrepo_root");
    /// ```
    pub fn compute_partition_id_for_root(root_name: &str, file: &str) -> String {
        let dir_part = Self::compute_directory_partition(file);
        format!("{}_{}", root_name, dir_part)
    }

    /// Internal helper: compute the directory-based partition component
    fn compute_directory_partition(file: &str) -> String {
        Path::new(file)
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| "root".to_string())
    }

    /// Get the SQLite database path for a partition
    fn partition_db_path(&self, partition_id: &str) -> PathBuf {
        // Sanitize partition ID for use as filename
        let safe_name = partition_id.replace(['/', '\\', ':'], "_");
        self.partitions_dir.join(format!("{}.db", safe_name))
    }

    // =========================================================================
    // Partition Loading
    // =========================================================================

    /// Check if a partition is currently loaded
    pub fn is_partition_loaded(&self, partition_id: &str) -> bool {
        self.registry.is_loaded(partition_id)
    }

    /// Get the number of loaded partitions
    pub fn loaded_partition_count(&self) -> usize {
        self.registry.loaded_count()
    }

    /// Get the list of loaded partition IDs
    pub fn loaded_partitions(&self) -> Vec<String> {
        self.registry.loaded_partition_ids()
    }

    /// Get all partition IDs from manifest (whether loaded or not)
    pub fn partition_ids(&self) -> Vec<String> {
        self.manifest.partitions.keys().cloned().collect()
    }

    /// Get node IDs that belong to a specific loaded partition
    ///
    /// Returns None if the partition is not currently loaded.
    /// Use this after load_partition() to get the nodes for indexing.
    pub fn node_ids_in_partition(&self, partition_id: &str) -> Option<Vec<String>> {
        self.registry.get_node_ids(partition_id)
    }

    /// Load a partition from SQLite into the petgraph
    ///
    /// Uses double-checked locking to prevent duplicate concurrent loads of the same
    /// partition while allowing concurrent loads of different partitions.
    ///
    /// If the partition is already loaded, this is a no-op (cache hit).
    /// If memory budget is exceeded, evicts least-recently-used partitions first.
    pub fn load_partition(&self, partition_id: &str) -> Result<(), LazyGraphError> {
        // First check: Is the partition already loaded? (lock-free read via DashSet)
        if self.registry.is_loaded(partition_id) {
            self.cache.touch(partition_id);
            return Ok(());
        }

        // Acquire per-partition loading lock to prevent concurrent loads of same partition
        // This allows different partitions to load concurrently while preventing duplicate work
        let loading_lock = self.registry.get_loading_lock(partition_id);
        let _guard = loading_lock.lock();

        // Second check: Re-check after acquiring lock (another thread may have loaded it)
        if self.registry.is_loaded(partition_id) {
            self.cache.touch(partition_id);
            return Ok(());
        }

        // Cache miss
        self.cache.touch(partition_id); // records miss since not in cache

        // Open partition database
        let db_path = self.partition_db_path(partition_id);
        if !db_path.exists() {
            return Err(LazyGraphError::PartitionNotFound(partition_id.to_string()));
        }

        let conn = PartitionConnection::open(&db_path, partition_id)?;

        // Get partition stats for memory estimation
        let partition_stats = conn.stats()?;
        let cache_stats =
            CachePartitionStats::new(partition_stats.node_count, partition_stats.edge_count);

        // Check if we need to evict partitions to make room
        let needed = self.cache.memory_needed_for(cache_stats.estimated_bytes);
        if needed > 0 {
            let candidates = self.cache.get_eviction_candidates_for(needed);
            for candidate_id in candidates {
                self.unload_partition(&candidate_id);
            }
        }

        // Load all nodes from partition
        let nodes = conn.query_all_nodes()?;
        let node_ids: Vec<String> = nodes.iter().map(|n| n.id.clone()).collect();

        // Load all edges from partition
        let edges = conn.query_all_edges()?;

        // Acquire write lock and add nodes/edges
        {
            let mut graph = self.graph.write();
            for node in nodes {
                graph.add_node(node);
            }
            for edge in edges {
                graph.add_edge(
                    &edge.source,
                    &edge.target,
                    EdgeData {
                        edge_type: edge.edge_type,
                        ref_line: edge.ref_line,
                        ident: edge.ident,
                        version_spec: edge.version_spec,
                        is_dev_dependency: edge.is_dev_dependency,
                    },
                );
            }
        }

        // Register partition as loaded
        self.registry.register_loaded(partition_id, node_ids);

        // Track in cache
        self.cache
            .record_loaded(partition_id.to_string(), cache_stats);

        Ok(())
    }

    /// Ensure a partition is loaded (load if not already loaded)
    pub fn ensure_partition_loaded(&self, partition_id: &str) -> Result<(), LazyGraphError> {
        self.load_partition(partition_id)
    }

    /// Load all partitions into memory
    ///
    /// This is useful when you need access to the entire graph, such as
    /// for full reindexing. Note that this bypasses the memory budget
    /// and may use significant memory for large repositories.
    ///
    /// Returns the number of partitions loaded.
    pub fn load_all_partitions(&self) -> Result<usize, LazyGraphError> {
        let partition_ids: Vec<String> = self.manifest.partitions.keys().cloned().collect();
        let mut loaded = 0;

        for partition_id in &partition_ids {
            if !self.registry.is_loaded(partition_id) {
                self.load_partition(partition_id)?;
                loaded += 1;
            }
        }

        Ok(loaded)
    }

    /// Unload a partition from petgraph to free memory
    ///
    /// Removes all nodes and edges belonging to this partition.
    /// Returns the number of nodes unloaded.
    ///
    /// Thread-safe: Uses interior mutability via DashMap (registry), Mutex (cache),
    /// and RwLock (graph).
    pub fn unload_partition(&self, partition_id: &str) -> usize {
        // Get the nodes belonging to this partition
        let nodes = match self.registry.unregister(partition_id) {
            Some(nodes) => nodes,
            None => return 0,
        };

        // Remove from cache (tracks eviction metrics)
        self.cache.remove(partition_id);

        // Remove all nodes (this also removes their incident edges)
        let count = nodes.len();
        {
            let mut graph = self.graph.write();
            for node_id in nodes {
                graph.remove_node(&node_id);
            }
        }

        count
    }

    // =========================================================================
    // Node Access (Lazy)
    // =========================================================================

    /// Get a node by ID, loading its partition if necessary
    ///
    /// Returns an owned clone of the node for thread safety. This enables
    /// concurrent read access without holding locks across call boundaries.
    pub fn get_node(&self, id: &str) -> Result<Option<Node>, LazyGraphError> {
        // Fast path: check if in graph (read lock)
        {
            let graph = self.graph.read();
            if let Some(node) = graph.get_node(id) {
                return Ok(Some(node.clone()));
            }
        }

        // Slow path: load partition then fetch
        if let Some(partition_id) = self.get_partition_for_node(id) {
            self.load_partition(&partition_id)?;
            let graph = self.graph.read();
            Ok(graph.get_node(id).cloned())
        } else {
            Ok(None)
        }
    }

    /// Get a node by ID without loading (only returns if already loaded)
    ///
    /// Returns an owned clone of the node for thread safety.
    pub fn get_node_if_loaded(&self, id: &str) -> Option<Node> {
        let graph = self.graph.read();
        graph.get_node(id).cloned()
    }

    /// Check if a node exists (may require loading partition)
    pub fn contains_node(&self, id: &str) -> Result<bool, LazyGraphError> {
        Ok(self.get_node(id)?.is_some())
    }

    /// Check if a node exists in the currently loaded graph (no lazy loading)
    pub fn contains_node_if_loaded(&self, id: &str) -> bool {
        let graph = self.graph.read();
        graph.contains_node(id)
    }

    // =========================================================================
    // Edge Access (Lazy)
    // =========================================================================

    /// Get incoming edges for a node, loading its partition if necessary
    ///
    /// This includes both intra-partition edges (from petgraph) and cross-partition
    /// edges (from CrossRefIndex). Cross-partition source nodes are loaded as needed.
    pub fn get_incoming_edges(
        &self,
        node_id: &str,
    ) -> Result<Vec<(Node, EdgeData)>, LazyGraphError> {
        // Ensure the target node's partition is loaded
        if let Some(partition_id) = self.get_partition_for_node(node_id) {
            self.load_partition(&partition_id)?;
        }

        // Collect intra-partition edges from petgraph (read lock)
        let mut edges: Vec<(Node, EdgeData)> = {
            let graph = self.graph.read();
            graph
                .incoming_edges(node_id)
                .map(|(n, e)| (n.clone(), e.clone()))
                .collect()
        };

        // Clone cross-refs to avoid borrow conflict when loading partitions
        let cross_refs: Vec<CrossRef> = self
            .cross_refs
            .get_by_target(node_id)
            .cloned()
            .unwrap_or_default();

        // Add cross-partition edges
        for cross_ref in cross_refs {
            // Load the source partition if not already loaded
            self.load_partition(&cross_ref.source_partition)?;

            // Get the source node (read lock)
            let graph = self.graph.read();
            if let Some(source_node) = graph.get_node(&cross_ref.source_id) {
                let edge_data = EdgeData {
                    edge_type: cross_ref.edge_type,
                    ref_line: cross_ref.ref_line,
                    ident: cross_ref.ident,
                    version_spec: cross_ref.version_spec,
                    is_dev_dependency: cross_ref.is_dev_dependency,
                };
                edges.push((source_node.clone(), edge_data));
            }
        }

        Ok(edges)
    }

    /// Get outgoing edges from a node, loading its partition if necessary
    ///
    /// This includes both intra-partition edges (from petgraph) and cross-partition
    /// edges (from CrossRefIndex). Cross-partition target nodes are loaded as needed.
    pub fn get_outgoing_edges(
        &self,
        node_id: &str,
    ) -> Result<Vec<(Node, EdgeData)>, LazyGraphError> {
        // Ensure the source node's partition is loaded
        if let Some(partition_id) = self.get_partition_for_node(node_id) {
            self.load_partition(&partition_id)?;
        }

        // Collect intra-partition edges from petgraph (read lock)
        let mut edges: Vec<(Node, EdgeData)> = {
            let graph = self.graph.read();
            graph
                .outgoing_edges(node_id)
                .map(|(n, e)| (n.clone(), e.clone()))
                .collect()
        };

        // Clone cross-refs to avoid borrow conflict when loading partitions
        let cross_refs: Vec<CrossRef> = self
            .cross_refs
            .get_by_source(node_id)
            .cloned()
            .unwrap_or_default();

        // Add cross-partition edges
        for cross_ref in cross_refs {
            // Load the target partition if not already loaded
            self.load_partition(&cross_ref.target_partition)?;

            // Get the target node (read lock)
            let graph = self.graph.read();
            if let Some(target_node) = graph.get_node(&cross_ref.target_id) {
                let edge_data = EdgeData {
                    edge_type: cross_ref.edge_type,
                    ref_line: cross_ref.ref_line,
                    ident: cross_ref.ident,
                    version_spec: cross_ref.version_spec,
                    is_dev_dependency: cross_ref.is_dev_dependency,
                };
                edges.push((target_node.clone(), edge_data));
            }
        }

        Ok(edges)
    }

    // =========================================================================
    // Cross-Partition Edge Management
    // =========================================================================

    /// Add a cross-partition edge reference
    ///
    /// Use this when an edge spans two different partitions.
    pub fn add_cross_ref(&mut self, cross_ref: CrossRef) {
        self.cross_refs.add(cross_ref);
    }

    /// Add multiple cross-partition edge references
    pub fn add_cross_refs(&mut self, refs: impl IntoIterator<Item = CrossRef>) {
        self.cross_refs.add_all(refs);
    }

    /// Get cross-partition edges targeting a specific node
    pub fn get_cross_refs_by_target(&self, target_id: &str) -> Option<&Vec<CrossRef>> {
        self.cross_refs.get_by_target(target_id)
    }

    /// Get cross-partition edges from a specific source node
    pub fn get_cross_refs_by_source(&self, source_id: &str) -> Option<&Vec<CrossRef>> {
        self.cross_refs.get_by_source(source_id)
    }

    /// Remove all cross-partition edges involving a specific partition
    ///
    /// Call this when a partition is being rebuilt.
    pub fn remove_cross_refs_by_partition(&mut self, partition: &str) {
        self.cross_refs.remove_by_partition(partition);
    }

    /// Get the number of cross-partition edges
    pub fn cross_ref_count(&self) -> usize {
        self.cross_refs.len()
    }

    /// Iterate over all cross-partition edge references
    pub fn iter_cross_refs(&self) -> impl Iterator<Item = &CrossRef> {
        self.cross_refs.iter()
    }

    /// Save cross-partition edges to SQLite
    pub fn save_cross_refs(&self) -> Result<(), LazyGraphError> {
        let store = if self.cross_refs_path.exists() {
            CrossRefStore::open(&self.cross_refs_path)?
        } else {
            CrossRefStore::create(&self.cross_refs_path)?
        };
        store.save_all(&self.cross_refs)?;
        Ok(())
    }

    /// Reload cross-partition edges from SQLite
    pub fn reload_cross_refs(&mut self) -> Result<(), LazyGraphError> {
        if self.cross_refs_path.exists() {
            let store = CrossRefStore::open(&self.cross_refs_path)?;
            self.cross_refs = store.load_all()?;
        }
        Ok(())
    }

    // =========================================================================
    // Direct Graph Access
    // =========================================================================

    /// Get access to the underlying RwLock-protected PetCodeGraph
    ///
    /// Callers must acquire read or write lock as appropriate.
    /// Use this for operations that don't require lazy loading.
    pub fn graph(&self) -> &RwLock<PetCodeGraph> {
        &self.graph
    }

    /// Get a read lock guard on the underlying PetCodeGraph
    ///
    /// Convenience method for read-only graph operations.
    pub fn graph_read(&self) -> parking_lot::RwLockReadGuard<'_, PetCodeGraph> {
        self.graph.read()
    }

    /// Get a write lock guard on the underlying PetCodeGraph
    ///
    /// Convenience method for mutable graph operations.
    pub fn graph_write(&self) -> parking_lot::RwLockWriteGuard<'_, PetCodeGraph> {
        self.graph.write()
    }

    // =========================================================================
    // Cache Operations
    // =========================================================================

    /// Get a snapshot of cache metrics (hit/miss rates, evictions)
    pub fn cache_metrics(&self) -> CacheMetrics {
        self.cache.metrics()
    }

    /// Reset cache metrics
    pub fn reset_cache_metrics(&self) {
        self.cache.reset_metrics();
    }

    /// Get current memory usage in bytes
    pub fn memory_usage_bytes(&self) -> usize {
        self.cache.current_memory_bytes()
    }

    /// Get memory budget in bytes
    pub fn memory_budget_bytes(&self) -> usize {
        self.cache.max_memory_bytes()
    }

    /// Get memory usage as a ratio (0.0 - 1.0)
    pub fn memory_usage_ratio(&self) -> f64 {
        self.cache.memory_usage_ratio()
    }

    /// Check if memory usage exceeds budget
    pub fn is_over_budget(&self) -> bool {
        self.cache.is_over_budget()
    }

    // =========================================================================
    // Statistics
    // =========================================================================

    /// Get statistics about the lazy graph
    pub fn stats(&self) -> LazyGraphStats {
        let cache_metrics = self.cache.metrics().clone();
        let graph = self.graph.read();
        LazyGraphStats {
            loaded_partitions: self.registry.loaded_count(),
            total_partitions: self.manifest.partitions.len(),
            loaded_nodes: graph.node_count(),
            loaded_edges: graph.edge_count(),
            cross_partition_edges: self.cross_refs.len(),
            total_files: self.manifest.files.len(),
            memory_usage_bytes: self.cache.current_memory_bytes(),
            memory_budget_bytes: self.cache.max_memory_bytes(),
            cache_hit_rate: cache_metrics.hit_rate(),
            cache_evictions: cache_metrics.evictions,
        }
    }
}

/// Statistics about the lazy graph state
#[derive(Debug, Clone)]
pub struct LazyGraphStats {
    /// Number of currently loaded partitions
    pub loaded_partitions: usize,
    /// Total number of partitions in manifest
    pub total_partitions: usize,
    /// Number of nodes currently in memory
    pub loaded_nodes: usize,
    /// Number of edges currently in memory (intra-partition)
    pub loaded_edges: usize,
    /// Number of cross-partition edges in index
    pub cross_partition_edges: usize,
    /// Total number of files tracked in manifest
    pub total_files: usize,
    /// Current memory usage in bytes
    pub memory_usage_bytes: usize,
    /// Memory budget in bytes
    pub memory_budget_bytes: usize,
    /// Cache hit rate (0.0 - 1.0)
    pub cache_hit_rate: f64,
    /// Number of partitions evicted
    pub cache_evictions: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CallableKind, NodeType};
    use tempfile::TempDir;

    fn create_test_node(id: &str, name: &str, file: &str) -> Node {
        Node {
            id: id.to_string(),
            name: name.to_string(),
            node_type: NodeType::Callable,
            kind: Some(CallableKind::Function.as_str().to_string()),
            subtype: None,
            file: file.to_string(),
            line: 1,
            end_line: 10,
            text: Some("def test(): pass".to_string()),
            hash: None,
            metadata: Default::default(),
        }
    }

    #[test]
    fn test_manifest_new() {
        let manifest = Manifest::new();
        assert_eq!(manifest.schema_version, "1.0");
        assert!(manifest.files.is_empty());
        assert!(manifest.partitions.is_empty());
    }

    #[test]
    fn test_manifest_save_load() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("manifest.json");

        let mut manifest = Manifest::new();
        manifest.set_file(
            "src/main.py".to_string(),
            "src".to_string(),
            Some("abc123".to_string()),
        );
        manifest.register_partition("src".to_string(), "src.db".to_string());

        manifest.save(&manifest_path).unwrap();

        let loaded = Manifest::load(&manifest_path).unwrap();
        assert_eq!(loaded.get_partition_for_file("src/main.py"), Some("src"));
        assert_eq!(loaded.get_partition_file("src"), Some("src.db"));
    }

    #[test]
    fn test_manifest_roots() {
        let temp_dir = TempDir::new().unwrap();
        let manifest_path = temp_dir.path().join("manifest.json");

        let mut manifest = Manifest::new();

        // Register git root
        manifest.register_root(RootInfo {
            name: "repo-a".to_string(),
            root_type: "git".to_string(),
            relative_path: "repo-a".to_string(),
            remote_url: Some("https://github.com/org/repo-a".to_string()),
            branch: Some("main".to_string()),
            commit: Some("abc123".to_string()),
        });

        // Register code directory root
        manifest.register_root(RootInfo {
            name: "scripts".to_string(),
            root_type: "code".to_string(),
            relative_path: "scripts".to_string(),
            remote_url: None,
            branch: None,
            commit: None,
        });

        assert_eq!(manifest.root_count(), 2);
        assert!(manifest.is_multi_root());

        // Save and reload
        manifest.save(&manifest_path).unwrap();
        let loaded = Manifest::load(&manifest_path).unwrap();

        assert_eq!(loaded.root_count(), 2);
        assert!(loaded.is_multi_root());

        let repo_a = loaded.get_root("repo-a").unwrap();
        assert_eq!(repo_a.root_type, "git");
        assert_eq!(
            repo_a.remote_url,
            Some("https://github.com/org/repo-a".to_string())
        );

        let scripts = loaded.get_root("scripts").unwrap();
        assert_eq!(scripts.root_type, "code");
        assert!(scripts.remote_url.is_none());
    }

    #[test]
    fn test_manifest_single_root() {
        let mut manifest = Manifest::new();

        manifest.register_root(RootInfo {
            name: "myrepo".to_string(),
            root_type: "git".to_string(),
            relative_path: ".".to_string(),
            remote_url: None,
            branch: None,
            commit: None,
        });

        assert_eq!(manifest.root_count(), 1);
        assert!(!manifest.is_multi_root());

        // Root names iterator
        let names: Vec<&str> = manifest.root_names().collect();
        assert_eq!(names, vec!["myrepo"]);
    }

    #[test]
    #[allow(deprecated)]
    fn test_compute_partition_id() {
        assert_eq!(
            LazyGraphManager::compute_partition_id("src/core/main.py"),
            "src/core"
        );
        assert_eq!(LazyGraphManager::compute_partition_id("main.py"), "root");
        assert_eq!(
            LazyGraphManager::compute_partition_id("a/b/c/d.rs"),
            "a/b/c"
        );
    }

    #[test]
    fn test_compute_partition_id_for_root() {
        // Multi-root: partition IDs are namespaced by root
        assert_eq!(
            LazyGraphManager::compute_partition_id_for_root("repo-a", "src/core/main.py"),
            "repo-a_src/core"
        );
        assert_eq!(
            LazyGraphManager::compute_partition_id_for_root("repo-b", "src/core/main.py"),
            "repo-b_src/core"
        );
        // No collision between different roots with same directory structure
        assert_ne!(
            LazyGraphManager::compute_partition_id_for_root("repo-a", "src/main.py"),
            LazyGraphManager::compute_partition_id_for_root("repo-b", "src/main.py")
        );
        // Root-level files
        assert_eq!(
            LazyGraphManager::compute_partition_id_for_root("myrepo", "main.py"),
            "myrepo_root"
        );
    }

    #[test]
    fn test_lazy_graph_manager_init() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let manager = LazyGraphManager::init(&prism_dir).unwrap();

        assert!(prism_dir.join("partitions").exists());
        assert!(prism_dir.join("manifest.json").exists());
        assert_eq!(manager.loaded_partition_count(), 0);
    }

    #[test]
    fn test_lazy_graph_manager_open() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        // Init first
        let _manager = LazyGraphManager::init(&prism_dir).unwrap();

        // Then open
        let manager = LazyGraphManager::open(&prism_dir).unwrap();
        assert_eq!(manager.loaded_partition_count(), 0);
    }

    #[test]
    fn test_partition_registry() {
        let registry = PartitionRegistry::new();

        // Register some nodes (methods now take &self due to interior mutability)
        registry.register_loaded(
            "src/core",
            vec![
                "src/core/main.py:func1".to_string(),
                "src/core/main.py:func2".to_string(),
            ],
        );

        assert!(registry.is_loaded("src/core"));
        assert!(!registry.is_loaded("src/other"));
        assert_eq!(
            registry.get_node_partition("src/core/main.py:func1"),
            Some("src/core".to_string())
        );

        // Unregister
        let nodes = registry.unregister("src/core").unwrap();
        assert_eq!(nodes.len(), 2);
        assert!(!registry.is_loaded("src/core"));
        assert!(registry
            .get_node_partition("src/core/main.py:func1")
            .is_none());
    }

    #[test]
    fn test_load_unload_partition() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        // Init manager
        let mut manager = LazyGraphManager::init(&prism_dir).unwrap();

        // Create a partition with some data
        let partition_id = "test_partition";
        let db_path = manager.partition_db_path(partition_id);
        let conn = PartitionConnection::create(&db_path, partition_id).unwrap();

        let node1 = create_test_node("test.py:func1", "func1", "test.py");
        let node2 = create_test_node("test.py:func2", "func2", "test.py");
        conn.insert_node(&node1).unwrap();
        conn.insert_node(&node2).unwrap();

        // Register in manifest
        manager
            .manifest_mut()
            .set_file("test.py".to_string(), partition_id.to_string(), None);
        manager
            .manifest_mut()
            .register_partition(partition_id.to_string(), "test_partition.db".to_string());

        // Load partition
        manager.load_partition(partition_id).unwrap();
        assert!(manager.is_partition_loaded(partition_id));
        assert_eq!(manager.graph_read().node_count(), 2);

        // Unload partition
        let unloaded = manager.unload_partition(partition_id);
        assert_eq!(unloaded, 2);
        assert!(!manager.is_partition_loaded(partition_id));
        assert_eq!(manager.graph_read().node_count(), 0);
    }

    #[test]
    fn test_get_node_lazy() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let mut manager = LazyGraphManager::init(&prism_dir).unwrap();

        // Create partition
        let partition_id = "src";
        let db_path = manager.partition_db_path(partition_id);
        let conn = PartitionConnection::create(&db_path, partition_id).unwrap();

        let node = create_test_node("src/main.py:main", "main", "src/main.py");
        conn.insert_node(&node).unwrap();

        // Register in manifest
        manager
            .manifest_mut()
            .set_file("src/main.py".to_string(), partition_id.to_string(), None);
        manager
            .manifest_mut()
            .register_partition(partition_id.to_string(), "src.db".to_string());

        // Node not loaded yet
        assert!(manager.get_node_if_loaded("src/main.py:main").is_none());

        // Lazy load via get_node
        let node = manager.get_node("src/main.py:main").unwrap();
        assert!(node.is_some());
        assert_eq!(node.unwrap().name, "main");

        // Now it should be loaded
        assert!(manager.is_partition_loaded(partition_id));
    }

    #[test]
    fn test_stats() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let mut manager = LazyGraphManager::init(&prism_dir).unwrap();

        // Add some manifest entries
        manager
            .manifest_mut()
            .set_file("src/a.py".to_string(), "src".to_string(), None);
        manager
            .manifest_mut()
            .set_file("src/b.py".to_string(), "src".to_string(), None);
        manager
            .manifest_mut()
            .register_partition("src".to_string(), "src.db".to_string());
        manager
            .manifest_mut()
            .register_partition("tests".to_string(), "tests.db".to_string());

        let stats = manager.stats();
        assert_eq!(stats.loaded_partitions, 0);
        assert_eq!(stats.total_partitions, 2);
        assert_eq!(stats.total_files, 2);
        assert_eq!(stats.loaded_nodes, 0);
        // Check cache stats are included
        assert_eq!(stats.memory_usage_bytes, 0);
        assert!(stats.memory_budget_bytes > 0);
    }

    #[test]
    fn test_cache_eviction() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        // Create manager with a small memory budget (15 KB) to trigger eviction
        // Each partition with 10 nodes is ~7168 bytes (10 * 512 * 1.4)
        // With budget of 15KB and min_partitions=2, loading 4 partitions should
        // trigger eviction of at least one partition
        let mut manager =
            LazyGraphManager::init_with_memory_budget(&prism_dir, Some(15_000)).unwrap();

        // Create four partitions with nodes
        for i in 1..=4 {
            let partition_id = format!("partition_{}", i);
            let db_path = manager.partition_db_path(&partition_id);
            let conn = PartitionConnection::create(&db_path, &partition_id).unwrap();

            // Add enough nodes to use some memory (~7KB per partition)
            for j in 0..10 {
                let node_id = format!("p{}/file.py:func_{}", i, j);
                let node =
                    create_test_node(&node_id, &format!("func_{}", j), &format!("p{}/file.py", i));
                conn.insert_node(&node).unwrap();
            }

            // Register in manifest
            manager
                .manifest_mut()
                .set_file(format!("p{}/file.py", i), partition_id.clone(), None);
            manager
                .manifest_mut()
                .register_partition(partition_id.clone(), format!("{}.db", partition_id));
        }

        // Load first two partitions (should fit within budget)
        manager.load_partition("partition_1").unwrap();
        manager.load_partition("partition_2").unwrap();
        assert!(manager.is_partition_loaded("partition_1"));
        assert!(manager.is_partition_loaded("partition_2"));

        // Load third partition - should trigger eviction
        // Current: 2 partitions (~14KB), budget 15KB, loading ~7KB more
        // Need to free space, but min_partitions=2 prevents eviction
        manager.load_partition("partition_3").unwrap();
        assert!(manager.is_partition_loaded("partition_3"));

        // Load fourth partition - should definitely trigger eviction
        // Now we have 3 partitions, min_partitions=2, so we can evict 1
        manager.load_partition("partition_4").unwrap();
        assert!(manager.is_partition_loaded("partition_4"));

        // Verify eviction happened
        let stats = manager.stats();
        assert!(stats.cache_evictions > 0, "Expected at least one eviction");

        // Should have 2-3 partitions loaded (at least min_partitions kept)
        assert!(
            stats.loaded_partitions >= 2,
            "Should keep at least min_partitions"
        );
    }

    #[test]
    fn test_cache_hit_miss_tracking() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let mut manager = LazyGraphManager::init(&prism_dir).unwrap();

        // Create a partition
        let partition_id = "test_partition";
        let db_path = manager.partition_db_path(partition_id);
        let conn = PartitionConnection::create(&db_path, partition_id).unwrap();

        let node = create_test_node("test.py:func", "func", "test.py");
        conn.insert_node(&node).unwrap();

        manager
            .manifest_mut()
            .set_file("test.py".to_string(), partition_id.to_string(), None);
        manager
            .manifest_mut()
            .register_partition(partition_id.to_string(), "test_partition.db".to_string());

        // First load should be a miss
        manager.load_partition(partition_id).unwrap();
        let metrics = manager.cache_metrics();
        assert_eq!(metrics.misses, 1);
        assert_eq!(metrics.hits, 0);

        // Second load should be a hit
        manager.load_partition(partition_id).unwrap();
        let metrics = manager.cache_metrics();
        assert_eq!(metrics.hits, 1);
        assert_eq!(metrics.misses, 1);
        assert!((metrics.hit_rate() - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_init_workspace_single_git_repo() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path();
        let prism_dir = workspace.join(".codeprysm");

        // Create a git repo with source files
        std::fs::create_dir(workspace.join(".git")).unwrap();
        std::fs::write(workspace.join("main.py"), "print('hello')").unwrap();

        let manager = LazyGraphManager::init_workspace(workspace, &prism_dir).unwrap();

        assert!(!manager.is_multi_root());
        assert_eq!(manager.manifest().root_count(), 1);

        // Check the root was discovered correctly
        let roots: Vec<_> = manager.roots().collect();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].root_type, "git");
        assert_eq!(roots[0].relative_path, ".");
    }

    #[test]
    fn test_init_workspace_multi_root() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path();
        let prism_dir = workspace.join(".codeprysm");

        // Create two git repos
        let repo_a = workspace.join("repo-a");
        let repo_b = workspace.join("repo-b");

        std::fs::create_dir_all(repo_a.join(".git")).unwrap();
        std::fs::write(repo_a.join("main.py"), "# repo a").unwrap();

        std::fs::create_dir_all(repo_b.join(".git")).unwrap();
        std::fs::write(repo_b.join("main.rs"), "fn main() {}").unwrap();

        let manager = LazyGraphManager::init_workspace(workspace, &prism_dir).unwrap();

        assert!(manager.is_multi_root());
        assert_eq!(manager.manifest().root_count(), 2);

        // Check roots were discovered
        let root_names: Vec<_> = manager.manifest().root_names().collect();
        assert!(root_names.contains(&"repo-a"));
        assert!(root_names.contains(&"repo-b"));
    }

    #[test]
    fn test_init_workspace_code_directory() {
        let temp_dir = TempDir::new().unwrap();
        let workspace = temp_dir.path();
        let prism_dir = workspace.join(".codeprysm");

        // Create a code directory (no .git)
        std::fs::write(workspace.join("script.py"), "print('hello')").unwrap();

        let manager = LazyGraphManager::init_workspace(workspace, &prism_dir).unwrap();

        assert!(!manager.is_multi_root());
        let roots: Vec<_> = manager.roots().collect();
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].root_type, "code");
    }

    #[test]
    fn test_root_info_from_discovered_root() {
        use crate::discovery::{DiscoveredRoot, RootType};

        // Test git repo conversion
        let git_root = DiscoveredRoot {
            path: std::path::PathBuf::from("/workspace/repo"),
            relative_path: "repo".to_string(),
            root_type: RootType::GitRepository {
                remote: Some("https://github.com/org/repo".to_string()),
                branch: Some("main".to_string()),
                commit: Some("abc123".to_string()),
            },
            name: "repo".to_string(),
        };

        let root_info = RootInfo::from_discovered_root(&git_root);
        assert_eq!(root_info.name, "repo");
        assert_eq!(root_info.root_type, "git");
        assert_eq!(root_info.relative_path, "repo");
        assert_eq!(
            root_info.remote_url,
            Some("https://github.com/org/repo".to_string())
        );
        assert_eq!(root_info.branch, Some("main".to_string()));
        assert_eq!(root_info.commit, Some("abc123".to_string()));

        // Test code directory conversion
        let code_root = DiscoveredRoot {
            path: std::path::PathBuf::from("/workspace/scripts"),
            relative_path: "scripts".to_string(),
            root_type: RootType::CodeDirectory,
            name: "scripts".to_string(),
        };

        let root_info = RootInfo::from_discovered_root(&code_root);
        assert_eq!(root_info.name, "scripts");
        assert_eq!(root_info.root_type, "code");
        assert!(root_info.remote_url.is_none());
        assert!(root_info.branch.is_none());
        assert!(root_info.commit.is_none());
    }

    #[test]
    fn test_cross_partition_edges() {
        use crate::graph::EdgeType;

        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let mut manager = LazyGraphManager::init(&prism_dir).unwrap();

        // Create partition A with a function
        let partition_a = "partition_a";
        let db_path_a = manager.partition_db_path(partition_a);
        let conn_a = PartitionConnection::create(&db_path_a, partition_a).unwrap();
        let node_a = create_test_node("a/main.py:caller", "caller", "a/main.py");
        conn_a.insert_node(&node_a).unwrap();

        // Create partition B with a function that is called
        let partition_b = "partition_b";
        let db_path_b = manager.partition_db_path(partition_b);
        let conn_b = PartitionConnection::create(&db_path_b, partition_b).unwrap();
        let node_b = create_test_node("b/lib.py:helper", "helper", "b/lib.py");
        conn_b.insert_node(&node_b).unwrap();

        // Register in manifest
        manager
            .manifest_mut()
            .set_file("a/main.py".to_string(), partition_a.to_string(), None);
        manager
            .manifest_mut()
            .set_file("b/lib.py".to_string(), partition_b.to_string(), None);
        manager
            .manifest_mut()
            .register_partition(partition_a.to_string(), "partition_a.db".to_string());
        manager
            .manifest_mut()
            .register_partition(partition_b.to_string(), "partition_b.db".to_string());

        // Add a cross-partition edge: a/main.py:caller -> b/lib.py:helper
        let cross_ref = CrossRef::new(
            "a/main.py:caller".to_string(),
            partition_a.to_string(),
            "b/lib.py:helper".to_string(),
            partition_b.to_string(),
            EdgeType::Uses,
            Some(10),
            Some("helper".to_string()),
        );
        manager.add_cross_ref(cross_ref);

        // Verify cross-ref was added
        assert_eq!(manager.cross_ref_count(), 1);
        assert_eq!(manager.stats().cross_partition_edges, 1);

        // Get outgoing edges from caller - should include cross-partition edge to helper
        let outgoing = manager.get_outgoing_edges("a/main.py:caller").unwrap();
        assert_eq!(outgoing.len(), 1);
        assert_eq!(outgoing[0].0.id, "b/lib.py:helper");
        assert_eq!(outgoing[0].1.edge_type, EdgeType::Uses);
        assert_eq!(outgoing[0].1.ref_line, Some(10));
        assert_eq!(outgoing[0].1.ident, Some("helper".to_string()));

        // Verify both partitions are now loaded (cross-ref triggered load)
        assert!(manager.is_partition_loaded(partition_a));
        assert!(manager.is_partition_loaded(partition_b));

        // Get incoming edges to helper - should include cross-partition edge from caller
        let incoming = manager.get_incoming_edges("b/lib.py:helper").unwrap();
        assert_eq!(incoming.len(), 1);
        assert_eq!(incoming[0].0.id, "a/main.py:caller");
    }

    #[test]
    fn test_cross_refs_persistence() {
        use crate::graph::EdgeType;

        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        // Create manager and add cross-refs
        {
            let mut manager = LazyGraphManager::init(&prism_dir).unwrap();

            manager.add_cross_ref(CrossRef::new(
                "a:x".to_string(),
                "p1".to_string(),
                "b:y".to_string(),
                "p2".to_string(),
                EdgeType::Uses,
                Some(5),
                None,
            ));
            manager.add_cross_ref(CrossRef::new(
                "c:z".to_string(),
                "p1".to_string(),
                "d:w".to_string(),
                "p2".to_string(),
                EdgeType::Defines,
                None,
                Some("w".to_string()),
            ));

            assert_eq!(manager.cross_ref_count(), 2);

            // Save cross-refs to SQLite
            manager.save_cross_refs().unwrap();
        }

        // Open manager again - should load cross-refs
        {
            let manager = LazyGraphManager::open(&prism_dir).unwrap();
            assert_eq!(manager.cross_ref_count(), 2);

            // Verify data integrity
            let refs_to_b = manager.get_cross_refs_by_target("b:y").unwrap();
            assert_eq!(refs_to_b.len(), 1);
            assert_eq!(refs_to_b[0].source_id, "a:x");
            assert_eq!(refs_to_b[0].edge_type, EdgeType::Uses);
            assert_eq!(refs_to_b[0].ref_line, Some(5));
        }
    }

    #[test]
    fn test_remove_cross_refs_by_partition() {
        use crate::graph::EdgeType;

        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let mut manager = LazyGraphManager::init(&prism_dir).unwrap();

        // Add cross-refs from different partitions
        manager.add_cross_ref(CrossRef::new(
            "a:x".to_string(),
            "p1".to_string(),
            "b:y".to_string(),
            "p2".to_string(),
            EdgeType::Uses,
            None,
            None,
        ));
        manager.add_cross_ref(CrossRef::new(
            "c:z".to_string(),
            "p2".to_string(),
            "d:w".to_string(),
            "p3".to_string(),
            EdgeType::Uses,
            None,
            None,
        ));
        manager.add_cross_ref(CrossRef::new(
            "e:v".to_string(),
            "p3".to_string(),
            "f:u".to_string(),
            "p4".to_string(),
            EdgeType::Uses,
            None,
            None,
        ));

        assert_eq!(manager.cross_ref_count(), 3);

        // Remove all cross-refs involving p2 (should remove first two)
        manager.remove_cross_refs_by_partition("p2");

        assert_eq!(manager.cross_ref_count(), 1);

        // Remaining should be p3 -> p4
        let remaining = manager.get_cross_refs_by_source("e:v").unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].target_id, "f:u");
    }
}
