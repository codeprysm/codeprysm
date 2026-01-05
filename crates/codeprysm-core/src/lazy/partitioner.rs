//! Graph Partitioner
//!
//! Converts a PetCodeGraph into partitioned SQLite files for lazy loading.
//!
//! # Design
//!
//! The partitioner splits a code graph by directory, creating one SQLite
//! partition per directory. Cross-partition edges (edges where source and
//! target are in different partitions) are stored separately in cross_refs.db.
//!
//! # Example
//!
//! ```no_run
//! use codeprysm_core::graph::PetCodeGraph;
//! use codeprysm_core::lazy::partitioner::GraphPartitioner;
//! use std::path::Path;
//!
//! let graph = PetCodeGraph::new();
//! // ... populate graph ...
//!
//! let manifest = GraphPartitioner::partition(
//!     &graph,
//!     Path::new(".codeprysm"),
//!     Some("my-repo"),
//! ).unwrap();
//! ```

use crate::graph::{Edge, Node, PetCodeGraph};
use crate::lazy::cross_refs::{CrossRef, CrossRefIndex, CrossRefStore};
use crate::lazy::manager::{LazyGraphManager, Manifest, RootInfo};
use crate::lazy::partition::PartitionConnection;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use thiserror::Error;

/// Errors that can occur during partitioning
#[derive(Debug, Error)]
pub enum PartitionerError {
    #[error("Partition error: {0}")]
    Partition(#[from] crate::lazy::partition::PartitionError),

    #[error("Cross-ref error: {0}")]
    CrossRef(#[from] crate::lazy::cross_refs::CrossRefError),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Manifest error: {0}")]
    Manifest(#[from] crate::lazy::manager::LazyGraphError),
}

/// Statistics from the partitioning process
#[derive(Debug, Clone)]
pub struct PartitioningStats {
    /// Total number of nodes partitioned
    pub total_nodes: usize,
    /// Total number of edges partitioned
    pub total_edges: usize,
    /// Number of partitions created
    pub partition_count: usize,
    /// Number of cross-partition edges
    pub cross_partition_edges: usize,
    /// Number of intra-partition edges
    pub intra_partition_edges: usize,
}

/// Graph Partitioner
///
/// Converts a PetCodeGraph into partitioned SQLite files.
pub struct GraphPartitioner;

impl GraphPartitioner {
    /// Partition a PetCodeGraph into SQLite partition files.
    ///
    /// # Arguments
    /// * `graph` - The code graph to partition
    /// * `prism_dir` - The .codeprysm directory path
    /// * `root_name` - Optional root name for multi-root support (uses "default" if None)
    ///
    /// # Returns
    /// A `Manifest` with file→partition mappings and partition→filename mappings.
    pub fn partition(
        graph: &PetCodeGraph,
        prism_dir: &Path,
        root_name: Option<&str>,
    ) -> Result<Manifest, PartitionerError> {
        let (manifest, _stats) = Self::partition_with_stats(graph, prism_dir, root_name)?;
        Ok(manifest)
    }

    /// Partition a PetCodeGraph and return statistics.
    ///
    /// Like `partition()` but also returns partitioning statistics.
    pub fn partition_with_stats(
        graph: &PetCodeGraph,
        prism_dir: &Path,
        root_name: Option<&str>,
    ) -> Result<(Manifest, PartitioningStats), PartitionerError> {
        let root = root_name.unwrap_or("default");
        let partitions_dir = prism_dir.join("partitions");

        // Create directories
        std::fs::create_dir_all(&partitions_dir)?;

        // Step 1: Group nodes by partition ID (directory-based)
        let node_partitions = Self::group_nodes_by_partition(graph, root);

        // Step 2: Compute partition IDs for all nodes (for edge classification)
        let node_to_partition: HashMap<String, String> = graph
            .iter_nodes()
            .map(|node| {
                let partition_id =
                    LazyGraphManager::compute_partition_id_for_root(root, &node.file);
                (node.id.clone(), partition_id)
            })
            .collect();

        // Step 3: Classify edges as intra-partition or cross-partition
        let (intra_edges, cross_refs) = Self::classify_edges(graph, &node_to_partition);

        // Step 4: Write partition SQLite files
        let mut manifest = Manifest::new();
        let mut partition_filenames: HashMap<String, String> = HashMap::new();

        for (partition_id, nodes) in &node_partitions {
            // Create sanitized filename for partition
            let safe_name = partition_id.replace(['/', '\\', ':'], "_");
            let filename = format!("{}.db", safe_name);
            let db_path = partitions_dir.join(&filename);

            // Create partition connection
            let conn = PartitionConnection::create(&db_path, partition_id)?;

            // Insert nodes
            let node_vec: Vec<Node> = nodes.to_vec();
            conn.insert_nodes(&node_vec)?;

            // Insert intra-partition edges for this partition
            if let Some(edges) = intra_edges.get(partition_id) {
                let edge_vec: Vec<Edge> = edges.to_vec();
                conn.insert_edges(&edge_vec)?;
            }

            // Register in manifest
            partition_filenames.insert(partition_id.clone(), filename.clone());
            manifest.register_partition(partition_id.clone(), filename);

            // Register files in manifest
            for node in nodes {
                if !node.file.is_empty() {
                    manifest.set_file(node.file.clone(), partition_id.clone(), node.hash.clone());
                }
            }
        }

        // Step 5: Write cross-partition edges to cross_refs.db
        let cross_refs_path = prism_dir.join("cross_refs.db");
        let cross_ref_store = CrossRefStore::create(&cross_refs_path)?;
        let cross_ref_vec: Vec<CrossRef> = cross_refs.iter().cloned().collect();
        cross_ref_store.add_refs(&cross_ref_vec)?;

        // Step 6: Register root info (if we have a root name)
        manifest.register_root(RootInfo {
            name: root.to_string(),
            root_type: "code".to_string(), // Default to "code", caller can update
            relative_path: ".".to_string(),
            remote_url: None,
            branch: None,
            commit: None,
        });

        // Step 7: Save manifest
        let manifest_path = prism_dir.join("manifest.json");
        manifest.save(&manifest_path)?;

        // Compute stats
        let stats = PartitioningStats {
            total_nodes: graph.node_count(),
            total_edges: graph.edge_count(),
            partition_count: node_partitions.len(),
            cross_partition_edges: cross_refs.len(),
            intra_partition_edges: intra_edges.values().map(|v| v.len()).sum(),
        };

        Ok((manifest, stats))
    }

    /// Partition a graph with a RootInfo structure for complete root metadata.
    ///
    /// This variant accepts a pre-configured RootInfo for better control over
    /// git metadata and root type.
    pub fn partition_with_root_info(
        graph: &PetCodeGraph,
        prism_dir: &Path,
        root_info: RootInfo,
    ) -> Result<(Manifest, PartitioningStats), PartitionerError> {
        let root = &root_info.name;
        let partitions_dir = prism_dir.join("partitions");

        // Create directories
        std::fs::create_dir_all(&partitions_dir)?;

        // Step 1: Group nodes by partition ID
        let node_partitions = Self::group_nodes_by_partition(graph, root);

        // Step 2: Compute partition IDs for all nodes
        let node_to_partition: HashMap<String, String> = graph
            .iter_nodes()
            .map(|node| {
                let partition_id =
                    LazyGraphManager::compute_partition_id_for_root(root, &node.file);
                (node.id.clone(), partition_id)
            })
            .collect();

        // Step 3: Classify edges
        let (intra_edges, cross_refs) = Self::classify_edges(graph, &node_to_partition);

        // Step 4: Write partitions
        let mut manifest = Manifest::new();

        for (partition_id, nodes) in &node_partitions {
            let safe_name = partition_id.replace(['/', '\\', ':'], "_");
            let filename = format!("{}.db", safe_name);
            let db_path = partitions_dir.join(&filename);

            let conn = PartitionConnection::create(&db_path, partition_id)?;

            let node_vec: Vec<Node> = nodes.to_vec();
            conn.insert_nodes(&node_vec)?;

            if let Some(edges) = intra_edges.get(partition_id) {
                let edge_vec: Vec<Edge> = edges.to_vec();
                conn.insert_edges(&edge_vec)?;
            }

            manifest.register_partition(partition_id.clone(), filename);

            for node in nodes {
                if !node.file.is_empty() {
                    manifest.set_file(node.file.clone(), partition_id.clone(), node.hash.clone());
                }
            }
        }

        // Step 5: Write cross-refs
        let cross_refs_path = prism_dir.join("cross_refs.db");
        let cross_ref_store = CrossRefStore::create(&cross_refs_path)?;
        let cross_ref_vec: Vec<CrossRef> = cross_refs.iter().cloned().collect();
        cross_ref_store.add_refs(&cross_ref_vec)?;

        // Step 6: Register root with full info
        manifest.register_root(root_info);

        // Step 7: Save manifest
        let manifest_path = prism_dir.join("manifest.json");
        manifest.save(&manifest_path)?;

        let stats = PartitioningStats {
            total_nodes: graph.node_count(),
            total_edges: graph.edge_count(),
            partition_count: node_partitions.len(),
            cross_partition_edges: cross_refs.len(),
            intra_partition_edges: intra_edges.values().map(|v| v.len()).sum(),
        };

        Ok((manifest, stats))
    }

    /// Group nodes by their partition ID (directory-based).
    ///
    /// Returns a map from partition_id to the set of nodes in that partition.
    fn group_nodes_by_partition(
        graph: &PetCodeGraph,
        root_name: &str,
    ) -> HashMap<String, Vec<Node>> {
        let mut partitions: HashMap<String, Vec<Node>> = HashMap::new();

        for node in graph.iter_nodes() {
            let partition_id =
                LazyGraphManager::compute_partition_id_for_root(root_name, &node.file);
            partitions
                .entry(partition_id)
                .or_default()
                .push(node.clone());
        }

        partitions
    }

    /// Classify edges as intra-partition or cross-partition.
    ///
    /// Returns:
    /// - intra_edges: Map from partition_id to edges within that partition
    /// - cross_refs: CrossRefIndex for edges spanning partitions
    fn classify_edges(
        graph: &PetCodeGraph,
        node_to_partition: &HashMap<String, String>,
    ) -> (HashMap<String, Vec<Edge>>, CrossRefIndex) {
        let mut intra_edges: HashMap<String, Vec<Edge>> = HashMap::new();
        let mut cross_refs = CrossRefIndex::new();

        for edge in graph.iter_edges() {
            let source_partition = node_to_partition.get(&edge.source);
            let target_partition = node_to_partition.get(&edge.target);

            match (source_partition, target_partition) {
                (Some(src_part), Some(tgt_part)) if src_part == tgt_part => {
                    // Intra-partition edge
                    intra_edges.entry(src_part.clone()).or_default().push(edge);
                }
                (Some(src_part), Some(tgt_part)) => {
                    // Cross-partition edge
                    cross_refs.add(CrossRef::new(
                        edge.source.clone(),
                        src_part.clone(),
                        edge.target.clone(),
                        tgt_part.clone(),
                        edge.edge_type,
                        edge.ref_line,
                        edge.ident.clone(),
                    ));
                }
                _ => {
                    // Edge references unknown node(s) - skip
                    // This can happen if an edge references a node not in the graph
                    // (e.g., external dependency)
                }
            }
        }

        (intra_edges, cross_refs)
    }

    /// Update a single partition after file changes.
    ///
    /// This is useful for incremental updates where only a few files changed.
    ///
    /// # Arguments
    /// * `graph` - The updated code graph (containing only changed files)
    /// * `prism_dir` - The .codeprysm directory path
    /// * `partition_id` - The partition to update
    /// * `root_name` - The root name
    pub fn update_partition(
        graph: &PetCodeGraph,
        prism_dir: &Path,
        partition_id: &str,
        root_name: &str,
    ) -> Result<(), PartitionerError> {
        let partitions_dir = prism_dir.join("partitions");
        let safe_name = partition_id.replace(['/', '\\', ':'], "_");
        let db_path = partitions_dir.join(format!("{}.db", safe_name));

        // Open or create the partition
        let conn = if db_path.exists() {
            PartitionConnection::open(&db_path, partition_id)?
        } else {
            std::fs::create_dir_all(&partitions_dir)?;
            PartitionConnection::create(&db_path, partition_id)?
        };

        // Clear existing data and rewrite
        conn.clear()?;

        // Compute partition IDs for edge classification
        let node_to_partition: HashMap<String, String> = graph
            .iter_nodes()
            .map(|node| {
                let pid = LazyGraphManager::compute_partition_id_for_root(root_name, &node.file);
                (node.id.clone(), pid)
            })
            .collect();

        // Collect nodes for this partition
        let nodes: Vec<Node> = graph
            .iter_nodes()
            .filter(|node| {
                let pid = LazyGraphManager::compute_partition_id_for_root(root_name, &node.file);
                pid == partition_id
            })
            .cloned()
            .collect();

        conn.insert_nodes(&nodes)?;

        // Classify and insert intra-partition edges
        let (intra_edges, _) = Self::classify_edges(graph, &node_to_partition);
        if let Some(edges) = intra_edges.get(partition_id) {
            conn.insert_edges(edges)?;
        }

        Ok(())
    }

    /// Get unique files from a graph.
    ///
    /// Useful for determining which files are in a partition.
    pub fn get_unique_files(graph: &PetCodeGraph) -> HashSet<String> {
        graph
            .iter_nodes()
            .filter(|n| !n.file.is_empty())
            .map(|n| n.file.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CallableKind, EdgeData, Node};
    use tempfile::TempDir;

    fn create_test_node(id: &str, name: &str, file: &str) -> Node {
        Node::callable(
            id.to_string(),
            name.to_string(),
            CallableKind::Function,
            file.to_string(),
            1,
            10,
        )
    }

    fn create_test_graph() -> PetCodeGraph {
        let mut graph = PetCodeGraph::new();

        // Add file nodes
        graph.add_node(Node::source_file(
            "src/core/main.py".to_string(),
            "src/core/main.py".to_string(),
            "abc123".to_string(),
            100,
        ));
        graph.add_node(Node::source_file(
            "src/utils/helper.py".to_string(),
            "src/utils/helper.py".to_string(),
            "def456".to_string(),
            50,
        ));

        // Add function nodes
        graph.add_node(create_test_node(
            "src/core/main.py:main",
            "main",
            "src/core/main.py",
        ));
        graph.add_node(create_test_node(
            "src/core/main.py:process",
            "process",
            "src/core/main.py",
        ));
        graph.add_node(create_test_node(
            "src/utils/helper.py:helper",
            "helper",
            "src/utils/helper.py",
        ));

        // Add edges
        // CONTAINS edges (file -> function)
        graph.add_edge(
            "src/core/main.py",
            "src/core/main.py:main",
            EdgeData::contains(),
        );
        graph.add_edge(
            "src/core/main.py",
            "src/core/main.py:process",
            EdgeData::contains(),
        );
        graph.add_edge(
            "src/utils/helper.py",
            "src/utils/helper.py:helper",
            EdgeData::contains(),
        );

        // Intra-partition USES edge
        graph.add_edge(
            "src/core/main.py:main",
            "src/core/main.py:process",
            EdgeData::uses(Some(5), Some("process".to_string())),
        );

        // Cross-partition USES edge
        graph.add_edge(
            "src/core/main.py:main",
            "src/utils/helper.py:helper",
            EdgeData::uses(Some(10), Some("helper".to_string())),
        );

        graph
    }

    #[test]
    fn test_partition_basic() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let graph = create_test_graph();
        let manifest = GraphPartitioner::partition(&graph, &prism_dir, Some("myrepo")).unwrap();

        // Check manifest has partitions
        assert!(!manifest.partitions.is_empty());

        // Check files are registered
        assert!(manifest
            .get_partition_for_file("src/core/main.py")
            .is_some());
        assert!(manifest
            .get_partition_for_file("src/utils/helper.py")
            .is_some());

        // Check partitions directory exists
        assert!(prism_dir.join("partitions").exists());

        // Check cross_refs.db exists
        assert!(prism_dir.join("cross_refs.db").exists());

        // Check manifest.json exists
        assert!(prism_dir.join("manifest.json").exists());
    }

    #[test]
    fn test_partition_with_stats() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let graph = create_test_graph();
        let (manifest, stats) =
            GraphPartitioner::partition_with_stats(&graph, &prism_dir, Some("myrepo")).unwrap();

        // Check stats
        assert_eq!(stats.total_nodes, 5);
        assert_eq!(stats.total_edges, 5);
        assert_eq!(stats.partition_count, 2); // src/core and src/utils

        // We have 1 cross-partition edge (main -> helper)
        assert_eq!(stats.cross_partition_edges, 1);

        // Intra-partition: 3 CONTAINS + 1 USES within src/core
        assert_eq!(stats.intra_partition_edges, 4);

        // Verify manifest
        assert_eq!(manifest.partitions.len(), 2);
    }

    #[test]
    fn test_partition_creates_correct_files() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let graph = create_test_graph();
        GraphPartitioner::partition(&graph, &prism_dir, Some("myrepo")).unwrap();

        // Check partition files exist
        let partitions_dir = prism_dir.join("partitions");
        let entries: Vec<_> = std::fs::read_dir(&partitions_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();

        // Should have 2 partition files
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn test_partition_roundtrip() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let graph = create_test_graph();
        GraphPartitioner::partition(&graph, &prism_dir, Some("myrepo")).unwrap();

        // Load manifest
        let loaded_manifest = Manifest::load(&prism_dir.join("manifest.json")).unwrap();

        // Verify files
        let partition_for_main = loaded_manifest
            .get_partition_for_file("src/core/main.py")
            .unwrap();
        assert!(partition_for_main.contains("src/core"));

        let partition_for_helper = loaded_manifest
            .get_partition_for_file("src/utils/helper.py")
            .unwrap();
        assert!(partition_for_helper.contains("src/utils"));

        // Verify cross-refs
        let cross_ref_store = CrossRefStore::open(&prism_dir.join("cross_refs.db")).unwrap();
        let cross_refs = cross_ref_store.load_all().unwrap();

        // Should have 1 cross-partition edge
        assert_eq!(cross_refs.len(), 1);

        // Verify the cross-ref
        let refs_to_helper = cross_refs
            .get_by_target("src/utils/helper.py:helper")
            .unwrap();
        assert_eq!(refs_to_helper.len(), 1);
        assert_eq!(refs_to_helper[0].source_id, "src/core/main.py:main");
    }

    #[test]
    fn test_group_nodes_by_partition() {
        let graph = create_test_graph();
        let partitions = GraphPartitioner::group_nodes_by_partition(&graph, "myrepo");

        // Should have 2 partitions
        assert_eq!(partitions.len(), 2);

        // Check partition IDs follow expected format
        assert!(partitions.contains_key("myrepo_src/core"));
        assert!(partitions.contains_key("myrepo_src/utils"));

        // Check node counts
        let core_nodes = partitions.get("myrepo_src/core").unwrap();
        assert_eq!(core_nodes.len(), 3); // file + 2 functions

        let utils_nodes = partitions.get("myrepo_src/utils").unwrap();
        assert_eq!(utils_nodes.len(), 2); // file + 1 function
    }

    #[test]
    fn test_classify_edges() {
        let graph = create_test_graph();

        let node_to_partition: HashMap<String, String> = graph
            .iter_nodes()
            .map(|node| {
                let partition_id =
                    LazyGraphManager::compute_partition_id_for_root("myrepo", &node.file);
                (node.id.clone(), partition_id)
            })
            .collect();

        let (intra_edges, cross_refs) =
            GraphPartitioner::classify_edges(&graph, &node_to_partition);

        // Check intra-partition edges
        assert!(intra_edges.contains_key("myrepo_src/core"));
        assert!(intra_edges.contains_key("myrepo_src/utils"));

        // src/core should have 3 intra-partition edges (2 CONTAINS + 1 USES)
        assert_eq!(intra_edges.get("myrepo_src/core").unwrap().len(), 3);

        // src/utils should have 1 intra-partition edge (1 CONTAINS)
        assert_eq!(intra_edges.get("myrepo_src/utils").unwrap().len(), 1);

        // Cross-partition edges
        assert_eq!(cross_refs.len(), 1);
    }

    #[test]
    fn test_partition_with_root_info() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let graph = create_test_graph();

        let root_info = RootInfo {
            name: "test-repo".to_string(),
            root_type: "git".to_string(),
            relative_path: ".".to_string(),
            remote_url: Some("https://github.com/org/repo".to_string()),
            branch: Some("main".to_string()),
            commit: Some("abc123".to_string()),
        };

        let (manifest, stats) =
            GraphPartitioner::partition_with_root_info(&graph, &prism_dir, root_info).unwrap();

        // Check root info is preserved
        let root = manifest.get_root("test-repo").unwrap();
        assert_eq!(root.root_type, "git");
        assert_eq!(
            root.remote_url,
            Some("https://github.com/org/repo".to_string())
        );
        assert_eq!(root.branch, Some("main".to_string()));

        // Check stats
        assert_eq!(stats.partition_count, 2);
    }

    #[test]
    fn test_partition_empty_graph() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let graph = PetCodeGraph::new();
        let (manifest, stats) =
            GraphPartitioner::partition_with_stats(&graph, &prism_dir, None).unwrap();

        assert_eq!(stats.total_nodes, 0);
        assert_eq!(stats.total_edges, 0);
        assert_eq!(stats.partition_count, 0);
        assert!(manifest.files.is_empty());
    }

    #[test]
    fn test_partition_single_file() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        let mut graph = PetCodeGraph::new();
        graph.add_node(create_test_node("main.py:func", "func", "main.py"));

        let (manifest, stats) =
            GraphPartitioner::partition_with_stats(&graph, &prism_dir, Some("myrepo")).unwrap();

        assert_eq!(stats.total_nodes, 1);
        assert_eq!(stats.partition_count, 1);

        // Root-level files go to "{root}_root" partition
        let partition = manifest.get_partition_for_file("main.py").unwrap();
        assert_eq!(partition, "myrepo_root");
    }

    #[test]
    fn test_update_partition() {
        let temp_dir = TempDir::new().unwrap();
        let prism_dir = temp_dir.path().join(".codeprysm");

        // Create initial partition
        let mut graph = PetCodeGraph::new();
        graph.add_node(create_test_node(
            "src/core/main.py:func1",
            "func1",
            "src/core/main.py",
        ));

        GraphPartitioner::partition(&graph, &prism_dir, Some("myrepo")).unwrap();

        // Update with new graph
        let mut updated_graph = PetCodeGraph::new();
        updated_graph.add_node(create_test_node(
            "src/core/main.py:func1",
            "func1",
            "src/core/main.py",
        ));
        updated_graph.add_node(create_test_node(
            "src/core/main.py:func2",
            "func2",
            "src/core/main.py",
        ));

        GraphPartitioner::update_partition(&updated_graph, &prism_dir, "myrepo_src/core", "myrepo")
            .unwrap();

        // Verify partition was updated
        let db_path = prism_dir.join("partitions/myrepo_src_core.db");
        let conn = PartitionConnection::open(&db_path, "myrepo_src/core").unwrap();
        let stats = conn.stats().unwrap();

        assert_eq!(stats.node_count, 2);
    }

    #[test]
    fn test_get_unique_files() {
        let graph = create_test_graph();
        let files = GraphPartitioner::get_unique_files(&graph);

        assert_eq!(files.len(), 2);
        assert!(files.contains("src/core/main.py"));
        assert!(files.contains("src/utils/helper.py"));
    }
}
