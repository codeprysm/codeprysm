//! Incremental Updater for Code Graph System
//!
//! This module orchestrates incremental updates to the code graph based on
//! file changes detected via Merkle trees. It enables efficient updates by
//! only reprocessing modified, added, or deleted files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::{debug, info, warn};

use crate::builder::{BuilderConfig, GraphBuilder};
use crate::graph::PetCodeGraph;
use crate::lazy::manager::LazyGraphManager;
use crate::lazy::partitioner::GraphPartitioner;
use crate::merkle::{ChangeSet, ExclusionFilter, MerkleTree, MerkleTreeManager};

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur during incremental updates.
#[derive(Debug, Error)]
pub enum UpdaterError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Lazy graph error
    #[error("Graph error: {0}")]
    LazyGraph(#[from] crate::lazy::manager::LazyGraphError),

    /// Partition error
    #[error("Partition error: {0}")]
    Partition(#[from] crate::lazy::partitioner::PartitionerError),

    /// Repository not found
    #[error("Repository not found: {0}")]
    RepoNotFound(PathBuf),

    /// Queries directory not found
    #[error("Queries directory not found: {0}")]
    QueriesNotFound(PathBuf),

    /// Builder error
    #[error("Builder error: {0}")]
    Builder(#[from] crate::builder::BuilderError),

    /// Merkle tree error
    #[error("Merkle tree error: {0}")]
    Merkle(#[from] crate::merkle::MerkleError),
}

/// Result type for updater operations.
pub type Result<T> = std::result::Result<T, UpdaterError>;

// ============================================================================
// Incremental Updater
// ============================================================================

/// Manages incremental updates to the code graph.
///
/// Uses Merkle tree change detection to identify modified files, then
/// selectively updates only the affected entities in the graph.
///
/// ## Example
///
/// ```ignore
/// use codeprysm_core::incremental::IncrementalUpdater;
/// use std::path::Path;
///
/// let mut updater = IncrementalUpdater::new(
///     Path::new("./my-repo"),
///     Path::new("./.codeprysm"),
///     Path::new("./queries"),
/// )?;
///
/// // Perform incremental update
/// let result = updater.update_repository(false)?;
/// if result.has_changes() {
///     println!("Updated {} files", result.changes.total_changes());
/// }
/// ```
pub struct IncrementalUpdater {
    /// Path to the repository being indexed
    repo_path: PathBuf,
    /// Path to the .codeprysm directory containing partitions
    prism_dir: PathBuf,
    /// Path to SCM query files (None = use embedded queries)
    queries_dir: Option<PathBuf>,
    /// Builder configuration
    builder_config: BuilderConfig,
    /// Merkle tree manager
    merkle_manager: MerkleTreeManager,
    /// Current graph state (loaded or built) - uses PetCodeGraph for efficient operations
    graph: Option<PetCodeGraph>,
    /// Current Merkle tree extracted from graph
    current_merkle_tree: MerkleTree,
}

impl IncrementalUpdater {
    /// Create a new incremental updater using embedded queries.
    ///
    /// This is the preferred constructor for production use as it doesn't require
    /// external query files.
    ///
    /// # Arguments
    ///
    /// * `repo_path` - Path to the repository being indexed
    /// * `prism_dir` - Path to the .codeprysm directory containing partitions
    ///
    /// # Errors
    ///
    /// Returns an error if the repository doesn't exist.
    pub fn new_with_embedded_queries(repo_path: &Path, prism_dir: &Path) -> Result<Self> {
        if !repo_path.exists() {
            return Err(UpdaterError::RepoNotFound(repo_path.to_path_buf()));
        }

        let merkle_manager = MerkleTreeManager::default();

        Ok(Self {
            repo_path: repo_path.to_path_buf(),
            prism_dir: prism_dir.to_path_buf(),
            queries_dir: None,
            builder_config: BuilderConfig::default(),
            merkle_manager,
            graph: None,
            current_merkle_tree: HashMap::new(),
        })
    }

    /// Create an updater with embedded queries and custom configuration.
    pub fn with_embedded_queries(
        repo_path: &Path,
        prism_dir: &Path,
        exclusion_filter: ExclusionFilter,
        builder_config: BuilderConfig,
    ) -> Result<Self> {
        if !repo_path.exists() {
            return Err(UpdaterError::RepoNotFound(repo_path.to_path_buf()));
        }

        let merkle_manager = MerkleTreeManager::new(exclusion_filter);

        Ok(Self {
            repo_path: repo_path.to_path_buf(),
            prism_dir: prism_dir.to_path_buf(),
            queries_dir: None,
            builder_config,
            merkle_manager,
            graph: None,
            current_merkle_tree: HashMap::new(),
        })
    }

    /// Create a new incremental updater using query files.
    ///
    /// # Arguments
    ///
    /// * `repo_path` - Path to the repository being indexed
    /// * `prism_dir` - Path to the .codeprysm directory containing partitions
    /// * `queries_dir` - Path to directory containing SCM query files
    ///
    /// # Errors
    ///
    /// Returns an error if the repository or queries directory doesn't exist.
    pub fn new(repo_path: &Path, prism_dir: &Path, queries_dir: &Path) -> Result<Self> {
        // Validate paths
        if !repo_path.exists() {
            return Err(UpdaterError::RepoNotFound(repo_path.to_path_buf()));
        }
        if !queries_dir.exists() {
            return Err(UpdaterError::QueriesNotFound(queries_dir.to_path_buf()));
        }

        let merkle_manager = MerkleTreeManager::default();

        Ok(Self {
            repo_path: repo_path.to_path_buf(),
            prism_dir: prism_dir.to_path_buf(),
            queries_dir: Some(queries_dir.to_path_buf()),
            builder_config: BuilderConfig::default(),
            merkle_manager,
            graph: None,
            current_merkle_tree: HashMap::new(),
        })
    }

    /// Create an updater with custom configuration using query files.
    pub fn with_config(
        repo_path: &Path,
        prism_dir: &Path,
        queries_dir: &Path,
        exclusion_filter: ExclusionFilter,
        builder_config: BuilderConfig,
    ) -> Result<Self> {
        if !repo_path.exists() {
            return Err(UpdaterError::RepoNotFound(repo_path.to_path_buf()));
        }
        if !queries_dir.exists() {
            return Err(UpdaterError::QueriesNotFound(queries_dir.to_path_buf()));
        }

        let merkle_manager = MerkleTreeManager::new(exclusion_filter);

        Ok(Self {
            repo_path: repo_path.to_path_buf(),
            prism_dir: prism_dir.to_path_buf(),
            queries_dir: Some(queries_dir.to_path_buf()),
            builder_config,
            merkle_manager,
            graph: None,
            current_merkle_tree: HashMap::new(),
        })
    }

    /// Load the existing graph state from partitions.
    ///
    /// # Returns
    ///
    /// `true` if graph was loaded successfully, `false` if prism_dir doesn't exist.
    pub fn load_graph_state(&mut self) -> Result<bool> {
        let manifest_path = self.prism_dir.join("manifest.json");
        if !manifest_path.exists() {
            info!("Partitioned graph not found: {:?}", self.prism_dir);
            return Ok(false);
        }

        info!("Loading graph from {:?}", self.prism_dir);

        // Open lazy graph manager and load all partitions
        let manager = LazyGraphManager::open(&self.prism_dir)?;
        manager.load_all_partitions()?;

        // Clone the graph for our use (acquire read lock first)
        let graph = manager.graph_read().clone();

        info!(
            "Loaded graph: {} nodes, {} edges",
            graph.node_count(),
            graph.edge_count()
        );

        // Extract Merkle tree from FILE entities
        self.current_merkle_tree = self.extract_merkle_tree_from_graph(&graph);
        info!(
            "Extracted Merkle tree: {} files",
            self.current_merkle_tree.len()
        );

        self.graph = Some(graph);
        Ok(true)
    }

    /// Extract file hashes from file nodes in the graph.
    /// Supports both legacy FILE type and Container with kind="file".
    fn extract_merkle_tree_from_graph(&self, graph: &PetCodeGraph) -> MerkleTree {
        let mut merkle_tree = HashMap::new();

        for node in graph.iter_nodes().filter(|n| n.is_file()) {
            if let Some(hash) = &node.hash {
                merkle_tree.insert(node.file.clone(), hash.clone());
            }
        }

        merkle_tree
    }

    /// Detect changes in the repository since last update.
    ///
    /// Builds a new Merkle tree from the current filesystem state and
    /// compares it with the stored state.
    pub fn detect_repository_changes(&mut self) -> Result<ChangeSet> {
        info!("Detecting repository changes...");

        // Build current Merkle tree
        let new_merkle_tree = self.merkle_manager.build_merkle_tree(&self.repo_path)?;

        // Compare with stored state
        let changes = self
            .merkle_manager
            .detect_changes(&self.current_merkle_tree, &new_merkle_tree);

        // Update current state
        self.current_merkle_tree = new_merkle_tree;

        Ok(changes)
    }

    /// Perform incremental update of the repository.
    ///
    /// # Arguments
    ///
    /// * `force_rebuild` - If true, rebuild everything regardless of changes
    ///
    /// # Returns
    ///
    /// `UpdateResult` with information about what was updated.
    pub fn update_repository(&mut self, force_rebuild: bool) -> Result<UpdateResult> {
        if force_rebuild {
            info!("Performing force rebuild...");
            return self.full_rebuild();
        }

        // Load existing state
        if !self.load_graph_state()? {
            info!("No existing graph found, performing initial build...");
            return self.full_rebuild();
        }

        // Detect changes
        let changes = self.detect_repository_changes()?;

        if !changes.has_changes() {
            info!("No changes detected, graph is up to date");
            return Ok(UpdateResult {
                success: true,
                changes,
                was_full_rebuild: false,
            });
        }

        info!("Processing {} changed files...", changes.total_changes());

        // Process changes
        self.process_changes(&changes)?;

        // Save updated graph
        self.save_graph()?;

        info!("Incremental update completed successfully");

        Ok(UpdateResult {
            success: true,
            changes,
            was_full_rebuild: false,
        })
    }

    /// Process detected file changes.
    fn process_changes(&mut self, changes: &ChangeSet) -> Result<()> {
        let start = std::time::Instant::now();

        // Handle deleted files first
        if !changes.deleted.is_empty() {
            self.process_deleted_files(&changes.deleted);
        }

        // Handle modified files - remove old nodes before reparsing
        if !changes.modified.is_empty() {
            self.process_modified_files(&changes.modified);
        }

        // Reparse modified and added files
        let files_to_reparse: Vec<String> = changes
            .modified
            .iter()
            .chain(changes.added.iter())
            .cloned()
            .collect();

        if !files_to_reparse.is_empty() {
            self.reparse_files(&files_to_reparse)?;
        }

        let elapsed = start.elapsed();
        info!(
            "Change processing completed in {:.2}s",
            elapsed.as_secs_f64()
        );

        Ok(())
    }

    /// Remove nodes for deleted files.
    fn process_deleted_files(&mut self, deleted_files: &[String]) {
        info!("Processing {} deleted files...", deleted_files.len());

        let graph = self
            .graph
            .as_mut()
            .expect("Graph must be loaded before processing changes");

        for file_path in deleted_files {
            graph.remove_file_nodes(file_path);
            debug!("Removed nodes for deleted file: {}", file_path);
        }
    }

    /// Remove nodes for modified files (before reparsing).
    fn process_modified_files(&mut self, modified_files: &[String]) {
        let graph = self
            .graph
            .as_mut()
            .expect("Graph must be loaded before processing changes");

        for file_path in modified_files {
            graph.remove_file_nodes(file_path);
            debug!("Removed nodes for modified file: {}", file_path);
        }
    }

    /// Reparse modified and added files.
    fn reparse_files(&mut self, file_paths: &[String]) -> Result<()> {
        info!("Reparsing {} files...", file_paths.len());

        // Create a builder for parsing - use embedded queries or custom directory
        let mut builder = match &self.queries_dir {
            Some(dir) => GraphBuilder::with_config(dir, self.builder_config.clone())?,
            None => GraphBuilder::with_embedded_queries(self.builder_config.clone()),
        };

        // Collect file graphs first to avoid borrow issues
        let mut file_graphs = Vec::new();

        for rel_path in file_paths {
            let abs_path = self.repo_path.join(rel_path);

            if !abs_path.exists() {
                warn!("File not found during reparse: {}", rel_path);
                continue;
            }

            // Parse single file with full entity extraction (returns PetCodeGraph directly)
            match builder.parse_file(&abs_path, rel_path) {
                Ok(file_graph) => {
                    file_graphs.push((rel_path.clone(), file_graph));
                }
                Err(e) => {
                    warn!("Error reparsing {}: {}", rel_path, e);
                }
            }
        }

        // Now merge all file graphs into the main graph
        let graph = self
            .graph
            .as_mut()
            .expect("Graph must be loaded before processing changes");

        for (rel_path, file_graph) in file_graphs {
            Self::merge_file_graph(graph, file_graph);
            debug!("Reparsed file: {}", rel_path);
        }

        Ok(())
    }

    /// Merge a file's graph into the main graph.
    fn merge_file_graph(main_graph: &mut PetCodeGraph, file_graph: PetCodeGraph) {
        // Add all nodes from file graph
        for node in file_graph.iter_nodes() {
            if !main_graph.contains_node(&node.id) {
                main_graph.add_node(node.clone());
            }
        }

        // Add all edges from file graph
        for edge in file_graph.iter_edges() {
            main_graph.add_edge_from_struct(&edge);
        }
    }

    /// Save the updated graph to partitions.
    fn save_graph(&self) -> Result<()> {
        let graph = self.graph.as_ref().expect("Graph must exist to save");

        info!("Saving graph to {:?}", self.prism_dir);

        // Determine root name from repo path
        let root_name = self
            .repo_path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "default".to_string());

        // Partition and save to prism directory
        let (_, stats) =
            GraphPartitioner::partition_with_stats(graph, &self.prism_dir, Some(&root_name))?;

        info!(
            "Saved graph: {} nodes, {} partitions, {} cross-partition edges",
            stats.total_nodes, stats.partition_count, stats.cross_partition_edges
        );

        Ok(())
    }

    /// Perform a full rebuild of the repository.
    fn full_rebuild(&mut self) -> Result<UpdateResult> {
        info!("Performing full rebuild...");

        // Build Merkle tree
        let merkle_tree = self.merkle_manager.build_merkle_tree(&self.repo_path)?;

        // Build graph using GraphBuilder - use embedded queries or custom directory
        let mut builder = match &self.queries_dir {
            Some(dir) => GraphBuilder::with_config(dir, self.builder_config.clone())?,
            None => GraphBuilder::with_embedded_queries(self.builder_config.clone()),
        };
        let mut graph = builder.build_from_directory(&self.repo_path)?;

        // Add file hashes to file entities (legacy FILE type or Container with kind="file")
        // Collect file nodes first to avoid borrow issues
        let file_nodes: Vec<(String, String)> = graph
            .iter_nodes()
            .filter(|n| n.is_file())
            .map(|n| (n.id.clone(), n.file.clone()))
            .collect();

        for (node_id, file_path) in file_nodes {
            if let Some(hash) = merkle_tree.get(&file_path) {
                if let Some(node_mut) = graph.get_node_mut(&node_id) {
                    node_mut.hash = Some(hash.clone());
                }
            }
        }

        // Store state
        self.graph = Some(graph);
        self.current_merkle_tree = merkle_tree.clone();

        // Save graph
        self.save_graph()?;

        // Return result indicating full rebuild
        let changes = ChangeSet {
            added: merkle_tree.keys().cloned().collect(),
            modified: vec![],
            deleted: vec![],
        };

        info!("Full rebuild completed");

        Ok(UpdateResult {
            success: true,
            changes,
            was_full_rebuild: true,
        })
    }

    /// Get a reference to the current graph.
    pub fn graph(&self) -> Option<&PetCodeGraph> {
        self.graph.as_ref()
    }

    /// Get a mutable reference to the current graph.
    pub fn graph_mut(&mut self) -> Option<&mut PetCodeGraph> {
        self.graph.as_mut()
    }

    /// Get the current Merkle tree.
    pub fn merkle_tree(&self) -> &MerkleTree {
        &self.current_merkle_tree
    }
}

// ============================================================================
// Update Result
// ============================================================================

/// Result of an incremental update operation.
#[derive(Debug, Clone)]
pub struct UpdateResult {
    /// Whether the update was successful.
    pub success: bool,
    /// Changes that were processed.
    pub changes: ChangeSet,
    /// Whether this was a full rebuild.
    pub was_full_rebuild: bool,
}

impl UpdateResult {
    /// Check if any changes were made.
    pub fn has_changes(&self) -> bool {
        self.changes.has_changes() || self.was_full_rebuild
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn setup_test_repo() -> (TempDir, PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let repo_path = temp_dir.path().to_path_buf();

        // Create a simple Python file
        let py_file = repo_path.join("test.py");
        let mut file = File::create(&py_file).unwrap();
        writeln!(file, "def hello():").unwrap();
        writeln!(file, "    print('Hello, World!')").unwrap();

        (temp_dir, repo_path)
    }

    fn get_queries_dir() -> PathBuf {
        // Find the queries directory relative to the crate root
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let queries_dir = manifest_dir.join("queries");

        if queries_dir.exists() {
            queries_dir
        } else {
            // Try parent directory (workspace root)
            manifest_dir
                .parent()
                .unwrap()
                .parent()
                .unwrap()
                .join("src")
                .join("queries")
        }
    }

    #[test]
    fn test_updater_creation() {
        let (_temp_dir, repo_path) = setup_test_repo();
        let prism_dir = repo_path.join(".codeprysm");
        std::fs::create_dir_all(&prism_dir).unwrap();
        let queries_dir = get_queries_dir();

        if !queries_dir.exists() {
            // Skip test if queries not available
            return;
        }

        let result = IncrementalUpdater::new(&repo_path, &prism_dir, &queries_dir);
        assert!(result.is_ok());
    }

    #[test]
    fn test_updater_missing_repo() {
        let prism_dir = PathBuf::from("/tmp/.codeprysm");
        let queries_dir = get_queries_dir();

        if !queries_dir.exists() {
            return;
        }

        let result =
            IncrementalUpdater::new(Path::new("/nonexistent/repo"), &prism_dir, &queries_dir);

        assert!(matches!(result, Err(UpdaterError::RepoNotFound(_))));
    }

    #[test]
    fn test_extract_merkle_tree_from_graph() {
        let (_temp_dir, repo_path) = setup_test_repo();
        let prism_dir = repo_path.join(".codeprysm");
        std::fs::create_dir_all(&prism_dir).unwrap();
        let queries_dir = get_queries_dir();

        if !queries_dir.exists() {
            return;
        }

        let updater = IncrementalUpdater::new(&repo_path, &prism_dir, &queries_dir).unwrap();

        // Create a graph with file nodes (Container with kind="file")
        let mut graph = PetCodeGraph::new();
        graph.add_node(crate::graph::Node::source_file(
            "test.py".to_string(),
            "test.py".to_string(),
            "abc123".to_string(),
            100,
        ));
        graph.add_node(crate::graph::Node::source_file(
            "main.py".to_string(),
            "main.py".to_string(),
            "def456".to_string(),
            100,
        ));

        let merkle_tree = updater.extract_merkle_tree_from_graph(&graph);

        assert_eq!(merkle_tree.len(), 2);
        assert_eq!(merkle_tree.get("test.py"), Some(&"abc123".to_string()));
        assert_eq!(merkle_tree.get("main.py"), Some(&"def456".to_string()));
    }

    #[test]
    fn test_update_result() {
        let result = UpdateResult {
            success: true,
            changes: ChangeSet {
                modified: vec!["a.py".to_string()],
                added: vec![],
                deleted: vec![],
            },
            was_full_rebuild: false,
        };

        assert!(result.has_changes());

        let result_no_changes = UpdateResult {
            success: true,
            changes: ChangeSet::new(),
            was_full_rebuild: false,
        };

        assert!(!result_no_changes.has_changes());

        let result_full_rebuild = UpdateResult {
            success: true,
            changes: ChangeSet::new(),
            was_full_rebuild: true,
        };

        assert!(result_full_rebuild.has_changes());
    }
}
