//! SQLite Partition Connection and CRUD Operations
//!
//! This module provides a wrapper around rusqlite for partition database operations.
//! Each partition is a self-contained SQLite database storing nodes and edges
//! for a subset of the repository.

use crate::graph::{Edge, EdgeType, Node, NodeType};
use rusqlite::{params, Connection, OptionalExtension, Result as SqliteResult};
use std::path::Path;
use thiserror::Error;

use super::schema::{
    PARTITION_SCHEMA_VERSION, SCHEMA_CREATE_EDGES, SCHEMA_CREATE_INDEXES, SCHEMA_CREATE_METADATA,
    SCHEMA_CREATE_NODES,
};

/// Errors that can occur during partition operations
#[derive(Debug, Error)]
pub enum PartitionError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Schema version mismatch: expected {expected}, found {found}")]
    SchemaVersionMismatch { expected: String, found: String },

    #[error("Node not found: {0}")]
    NodeNotFound(String),

    #[error("JSON serialization error: {0}")]
    JsonError(#[from] serde_json::Error),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// A connection to a partition SQLite database
pub struct PartitionConnection {
    conn: Connection,
    /// The partition ID (usually based on directory path or content hash)
    partition_id: String,
}

impl PartitionConnection {
    /// Open an existing partition database
    ///
    /// If the partition is at an older schema version (v1.0), it will be
    /// automatically migrated to the current version (v1.1).
    pub fn open(path: &Path, partition_id: &str) -> Result<Self, PartitionError> {
        let conn = Connection::open(path)?;
        Self::configure_connection(&conn)?;

        let pc = Self {
            conn,
            partition_id: partition_id.to_string(),
        };

        // Check and migrate schema version if needed
        if let Some(version) = pc.get_metadata("schema_version")? {
            match version.as_str() {
                v if v == PARTITION_SCHEMA_VERSION => {
                    // Already at current version, nothing to do
                }
                "1.0" => {
                    // Migrate from v1.0 to v1.1
                    pc.migrate_v1_0_to_v1_1()?;
                }
                _ => {
                    return Err(PartitionError::SchemaVersionMismatch {
                        expected: PARTITION_SCHEMA_VERSION.to_string(),
                        found: version,
                    });
                }
            }
        }

        Ok(pc)
    }

    /// Migrate partition from v1.0 to v1.1
    ///
    /// Adds version_spec and is_dev_dependency columns to edges table.
    fn migrate_v1_0_to_v1_1(&self) -> Result<(), PartitionError> {
        // Execute migration SQL (each ALTER TABLE is a separate statement)
        self.conn
            .execute("ALTER TABLE edges ADD COLUMN version_spec TEXT", [])?;
        self.conn
            .execute("ALTER TABLE edges ADD COLUMN is_dev_dependency INTEGER", [])?;

        // Update schema version
        self.set_metadata("schema_version", PARTITION_SCHEMA_VERSION)?;

        Ok(())
    }

    /// Create a new partition database with schema
    pub fn create(path: &Path, partition_id: &str) -> Result<Self, PartitionError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        Self::configure_connection(&conn)?;

        // Create schema
        conn.execute(SCHEMA_CREATE_NODES, [])?;
        conn.execute(SCHEMA_CREATE_EDGES, [])?;
        conn.execute(SCHEMA_CREATE_METADATA, [])?;
        conn.execute_batch(SCHEMA_CREATE_INDEXES)?;

        let pc = Self {
            conn,
            partition_id: partition_id.to_string(),
        };

        // Store schema version
        pc.set_metadata("schema_version", PARTITION_SCHEMA_VERSION)?;
        pc.set_metadata("partition_id", partition_id)?;

        Ok(pc)
    }

    /// Create an in-memory partition database (for testing)
    pub fn in_memory(partition_id: &str) -> Result<Self, PartitionError> {
        let conn = Connection::open_in_memory()?;
        Self::configure_connection(&conn)?;

        // Create schema
        conn.execute(SCHEMA_CREATE_NODES, [])?;
        conn.execute(SCHEMA_CREATE_EDGES, [])?;
        conn.execute(SCHEMA_CREATE_METADATA, [])?;
        conn.execute_batch(SCHEMA_CREATE_INDEXES)?;

        let pc = Self {
            conn,
            partition_id: partition_id.to_string(),
        };

        pc.set_metadata("schema_version", PARTITION_SCHEMA_VERSION)?;
        pc.set_metadata("partition_id", partition_id)?;

        Ok(pc)
    }

    /// Configure connection with optimal settings
    fn configure_connection(conn: &Connection) -> SqliteResult<()> {
        // Enable WAL mode for better concurrent read performance
        conn.pragma_update(None, "journal_mode", "WAL")?;
        // Enable foreign keys (not currently used but good practice)
        conn.pragma_update(None, "foreign_keys", "ON")?;
        // Increase cache size (negative value = KB)
        conn.pragma_update(None, "cache_size", -64000)?; // 64MB cache
                                                         // Synchronous mode: NORMAL is good balance of safety/speed
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        // Temp store in memory for better performance
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        // Enable memory-mapped I/O for reads
        conn.pragma_update(None, "mmap_size", 268435456)?; // 256MB mmap
        Ok(())
    }

    /// Get the partition ID
    pub fn partition_id(&self) -> &str {
        &self.partition_id
    }

    // =========================================================================
    // Metadata Operations
    // =========================================================================

    /// Get a metadata value
    pub fn get_metadata(&self, key: &str) -> Result<Option<String>, PartitionError> {
        let result = self
            .conn
            .query_row(
                "SELECT value FROM partition_metadata WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }

    /// Set a metadata value
    pub fn set_metadata(&self, key: &str, value: &str) -> Result<(), PartitionError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO partition_metadata (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    // =========================================================================
    // Node Operations
    // =========================================================================

    /// Insert a node into the partition
    pub fn insert_node(&self, node: &Node) -> Result<(), PartitionError> {
        let metadata_json = if node.metadata.is_empty() {
            None
        } else {
            Some(serde_json::to_string(&node.metadata)?)
        };

        self.conn.execute(
            r#"
            INSERT OR REPLACE INTO nodes
                (id, name, node_type, kind, subtype, file, line, end_line, text, hash, metadata_json)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            "#,
            params![
                node.id,
                node.name,
                node.node_type.as_str(),
                node.kind,
                node.subtype,
                node.file,
                node.line as i64,
                node.end_line as i64,
                node.text,
                node.hash,
                metadata_json,
            ],
        )?;
        Ok(())
    }

    /// Insert multiple nodes in a transaction
    pub fn insert_nodes(&self, nodes: &[Node]) -> Result<(), PartitionError> {
        let tx = self.conn.unchecked_transaction()?;

        {
            let mut stmt = tx.prepare(
                r#"
                INSERT OR REPLACE INTO nodes
                    (id, name, node_type, kind, subtype, file, line, end_line, text, hash, metadata_json)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                "#,
            )?;

            for node in nodes {
                let metadata_json = if node.metadata.is_empty() {
                    None
                } else {
                    Some(serde_json::to_string(&node.metadata)?)
                };

                stmt.execute(params![
                    node.id,
                    node.name,
                    node.node_type.as_str(),
                    node.kind,
                    node.subtype,
                    node.file,
                    node.line as i64,
                    node.end_line as i64,
                    node.text,
                    node.hash,
                    metadata_json,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Get a node by ID
    pub fn get_node(&self, id: &str) -> Result<Option<Node>, PartitionError> {
        let result = self
            .conn
            .query_row(
                r#"
                SELECT id, name, node_type, kind, subtype, file, line, end_line, text, hash, metadata_json
                FROM nodes WHERE id = ?1
                "#,
                [id],
                Self::row_to_node,
            )
            .optional()?;
        Ok(result)
    }

    /// Query nodes by file path
    pub fn query_nodes_by_file(&self, file: &str) -> Result<Vec<Node>, PartitionError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, name, node_type, kind, subtype, file, line, end_line, text, hash, metadata_json
            FROM nodes WHERE file = ?1
            "#,
        )?;

        let nodes = stmt
            .query_map([file], Self::row_to_node)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(nodes)
    }

    /// Query all nodes in the partition
    pub fn query_all_nodes(&self) -> Result<Vec<Node>, PartitionError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT id, name, node_type, kind, subtype, file, line, end_line, text, hash, metadata_json
            FROM nodes
            "#,
        )?;

        let nodes = stmt
            .query_map([], Self::row_to_node)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(nodes)
    }

    /// Delete nodes by file path
    pub fn delete_nodes_by_file(&self, file: &str) -> Result<usize, PartitionError> {
        let deleted = self
            .conn
            .execute("DELETE FROM nodes WHERE file = ?1", [file])?;
        Ok(deleted)
    }

    /// Delete a node by ID
    pub fn delete_node(&self, id: &str) -> Result<bool, PartitionError> {
        let deleted = self.conn.execute("DELETE FROM nodes WHERE id = ?1", [id])?;
        Ok(deleted > 0)
    }

    /// Get node count
    pub fn node_count(&self) -> Result<usize, PartitionError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM nodes", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Convert a database row to a Node
    fn row_to_node(row: &rusqlite::Row<'_>) -> SqliteResult<Node> {
        let node_type_str: String = row.get(2)?;
        let metadata_json: Option<String> = row.get(10)?;

        // Handle legacy FILE type by converting to Container
        let (node_type, is_legacy_file) = match node_type_str.as_str() {
            "FILE" => (NodeType::Container, true), // Legacy: convert to Container
            "Container" => (NodeType::Container, false),
            "Callable" => (NodeType::Callable, false),
            "Data" => (NodeType::Data, false),
            _ => (NodeType::Container, false), // Default fallback
        };

        // For legacy FILE nodes, ensure kind is "file"
        let kind: Option<String> = if is_legacy_file {
            Some("file".to_string())
        } else {
            row.get(3)?
        };

        let metadata = metadata_json
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();

        Ok(Node {
            id: row.get(0)?,
            name: row.get(1)?,
            node_type,
            kind,
            subtype: row.get(4)?,
            file: row.get(5)?,
            line: row.get::<_, i64>(6)? as usize,
            end_line: row.get::<_, i64>(7)? as usize,
            text: row.get(8)?,
            hash: row.get(9)?,
            metadata,
        })
    }

    // =========================================================================
    // Edge Operations
    // =========================================================================

    /// Insert an edge into the partition
    pub fn insert_edge(&self, edge: &Edge) -> Result<(), PartitionError> {
        self.conn.execute(
            r#"
            INSERT OR IGNORE INTO edges (source, target, edge_type, ref_line, ident, version_spec, is_dev_dependency)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                edge.source,
                edge.target,
                edge.edge_type.as_str(),
                edge.ref_line.map(|l| l as i64),
                edge.ident,
                edge.version_spec,
                edge.is_dev_dependency,
            ],
        )?;
        Ok(())
    }

    /// Insert multiple edges in a transaction
    pub fn insert_edges(&self, edges: &[Edge]) -> Result<(), PartitionError> {
        let tx = self.conn.unchecked_transaction()?;

        {
            let mut stmt = tx.prepare(
                r#"
                INSERT OR IGNORE INTO edges (source, target, edge_type, ref_line, ident, version_spec, is_dev_dependency)
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                "#,
            )?;

            for edge in edges {
                stmt.execute(params![
                    edge.source,
                    edge.target,
                    edge.edge_type.as_str(),
                    edge.ref_line.map(|l| l as i64),
                    edge.ident,
                    edge.version_spec,
                    edge.is_dev_dependency,
                ])?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Query edges by source node ID
    pub fn query_edges_by_source(&self, source: &str) -> Result<Vec<Edge>, PartitionError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT source, target, edge_type, ref_line, ident, version_spec, is_dev_dependency
            FROM edges WHERE source = ?1
            "#,
        )?;

        let edges = stmt
            .query_map([source], Self::row_to_edge)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(edges)
    }

    /// Query edges by target node ID
    pub fn query_edges_by_target(&self, target: &str) -> Result<Vec<Edge>, PartitionError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT source, target, edge_type, ref_line, ident, version_spec, is_dev_dependency
            FROM edges WHERE target = ?1
            "#,
        )?;

        let edges = stmt
            .query_map([target], Self::row_to_edge)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(edges)
    }

    /// Query all edges in the partition
    pub fn query_all_edges(&self) -> Result<Vec<Edge>, PartitionError> {
        let mut stmt = self.conn.prepare(
            r#"
            SELECT source, target, edge_type, ref_line, ident, version_spec, is_dev_dependency
            FROM edges
            "#,
        )?;

        let edges = stmt
            .query_map([], Self::row_to_edge)?
            .collect::<SqliteResult<Vec<_>>>()?;

        Ok(edges)
    }

    /// Delete edges involving a node (as source or target)
    pub fn delete_edges_involving(&self, node_id: &str) -> Result<usize, PartitionError> {
        let deleted = self.conn.execute(
            "DELETE FROM edges WHERE source = ?1 OR target = ?1",
            [node_id],
        )?;
        Ok(deleted)
    }

    /// Delete edges by source node
    pub fn delete_edges_by_source(&self, source: &str) -> Result<usize, PartitionError> {
        let deleted = self
            .conn
            .execute("DELETE FROM edges WHERE source = ?1", [source])?;
        Ok(deleted)
    }

    /// Get edge count
    pub fn edge_count(&self) -> Result<usize, PartitionError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM edges", [], |row| row.get(0))?;
        Ok(count as usize)
    }

    /// Convert a database row to an Edge
    fn row_to_edge(row: &rusqlite::Row<'_>) -> SqliteResult<Edge> {
        let edge_type_str: String = row.get(2)?;
        let edge_type = match edge_type_str.as_str() {
            "CONTAINS" => EdgeType::Contains,
            "USES" => EdgeType::Uses,
            "DEFINES" => EdgeType::Defines,
            "DEPENDS_ON" => EdgeType::DependsOn,
            _ => EdgeType::Uses, // Default fallback
        };

        Ok(Edge {
            source: row.get(0)?,
            target: row.get(1)?,
            edge_type,
            ref_line: row.get::<_, Option<i64>>(3)?.map(|l| l as usize),
            ident: row.get(4)?,
            version_spec: row.get(5).ok().flatten(),
            is_dev_dependency: row.get(6).ok().flatten(),
        })
    }

    // =========================================================================
    // Bulk Operations
    // =========================================================================

    /// Clear all data from the partition (keeps schema)
    pub fn clear(&self) -> Result<(), PartitionError> {
        self.conn.execute("DELETE FROM edges", [])?;
        self.conn.execute("DELETE FROM nodes", [])?;
        Ok(())
    }

    /// Get partition statistics
    pub fn stats(&self) -> Result<PartitionStats, PartitionError> {
        Ok(PartitionStats {
            node_count: self.node_count()?,
            edge_count: self.edge_count()?,
            partition_id: self.partition_id.clone(),
        })
    }
}

/// Statistics about a partition
#[derive(Debug, Clone)]
pub struct PartitionStats {
    pub node_count: usize,
    pub edge_count: usize,
    pub partition_id: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{CallableKind, ContainerKind, NodeMetadata};

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
            metadata: NodeMetadata::default(),
        }
    }

    fn create_test_edge(source: &str, target: &str, edge_type: EdgeType) -> Edge {
        Edge {
            source: source.to_string(),
            target: target.to_string(),
            edge_type,
            ref_line: Some(5),
            ident: Some("test".to_string()),
            version_spec: None,
            is_dev_dependency: None,
        }
    }

    #[test]
    fn test_create_in_memory() {
        let conn = PartitionConnection::in_memory("test_partition").unwrap();
        assert_eq!(conn.partition_id(), "test_partition");
        assert_eq!(conn.node_count().unwrap(), 0);
        assert_eq!(conn.edge_count().unwrap(), 0);
    }

    #[test]
    fn test_insert_and_get_node() {
        let conn = PartitionConnection::in_memory("test").unwrap();
        let node = create_test_node("test.py:test_func", "test_func", "test.py");

        conn.insert_node(&node).unwrap();
        assert_eq!(conn.node_count().unwrap(), 1);

        let retrieved = conn.get_node("test.py:test_func").unwrap().unwrap();
        assert_eq!(retrieved.id, "test.py:test_func");
        assert_eq!(retrieved.name, "test_func");
        assert_eq!(retrieved.node_type, NodeType::Callable);
        assert_eq!(retrieved.kind, Some("function".to_string()));
        assert_eq!(retrieved.file, "test.py");
        assert_eq!(retrieved.line, 1);
        assert_eq!(retrieved.end_line, 10);
    }

    #[test]
    fn test_insert_bulk_nodes() {
        let conn = PartitionConnection::in_memory("test").unwrap();
        let nodes: Vec<Node> = (0..100)
            .map(|i| {
                create_test_node(
                    &format!("test.py:func_{}", i),
                    &format!("func_{}", i),
                    "test.py",
                )
            })
            .collect();

        conn.insert_nodes(&nodes).unwrap();
        assert_eq!(conn.node_count().unwrap(), 100);
    }

    #[test]
    fn test_query_nodes_by_file() {
        let conn = PartitionConnection::in_memory("test").unwrap();

        conn.insert_node(&create_test_node("a.py:func1", "func1", "a.py"))
            .unwrap();
        conn.insert_node(&create_test_node("a.py:func2", "func2", "a.py"))
            .unwrap();
        conn.insert_node(&create_test_node("b.py:func3", "func3", "b.py"))
            .unwrap();

        let a_nodes = conn.query_nodes_by_file("a.py").unwrap();
        assert_eq!(a_nodes.len(), 2);

        let b_nodes = conn.query_nodes_by_file("b.py").unwrap();
        assert_eq!(b_nodes.len(), 1);
    }

    #[test]
    fn test_delete_node() {
        let conn = PartitionConnection::in_memory("test").unwrap();
        let node = create_test_node("test.py:func", "func", "test.py");

        conn.insert_node(&node).unwrap();
        assert_eq!(conn.node_count().unwrap(), 1);

        let deleted = conn.delete_node("test.py:func").unwrap();
        assert!(deleted);
        assert_eq!(conn.node_count().unwrap(), 0);

        // Deleting non-existent node returns false
        let deleted = conn.delete_node("nonexistent").unwrap();
        assert!(!deleted);
    }

    #[test]
    fn test_insert_and_query_edges() {
        let conn = PartitionConnection::in_memory("test").unwrap();

        let edge1 = create_test_edge("a", "b", EdgeType::Contains);
        let edge2 = create_test_edge("a", "c", EdgeType::Uses);
        let edge3 = create_test_edge("b", "c", EdgeType::Defines);

        conn.insert_edge(&edge1).unwrap();
        conn.insert_edge(&edge2).unwrap();
        conn.insert_edge(&edge3).unwrap();
        assert_eq!(conn.edge_count().unwrap(), 3);

        // Query by source
        let from_a = conn.query_edges_by_source("a").unwrap();
        assert_eq!(from_a.len(), 2);

        // Query by target
        let to_c = conn.query_edges_by_target("c").unwrap();
        assert_eq!(to_c.len(), 2);
    }

    #[test]
    fn test_insert_bulk_edges() {
        let conn = PartitionConnection::in_memory("test").unwrap();
        let edges: Vec<Edge> = (0..100)
            .map(|i| {
                create_test_edge(
                    &format!("node_{}", i),
                    &format!("node_{}", i + 1),
                    EdgeType::Uses,
                )
            })
            .collect();

        conn.insert_edges(&edges).unwrap();
        assert_eq!(conn.edge_count().unwrap(), 100);
    }

    #[test]
    fn test_delete_edges_involving() {
        let conn = PartitionConnection::in_memory("test").unwrap();

        conn.insert_edge(&create_test_edge("a", "b", EdgeType::Contains))
            .unwrap();
        conn.insert_edge(&create_test_edge("b", "c", EdgeType::Uses))
            .unwrap();
        conn.insert_edge(&create_test_edge("c", "a", EdgeType::Defines))
            .unwrap();
        assert_eq!(conn.edge_count().unwrap(), 3);

        // Delete edges involving "b" (should delete 2 edges)
        let deleted = conn.delete_edges_involving("b").unwrap();
        assert_eq!(deleted, 2);
        assert_eq!(conn.edge_count().unwrap(), 1);
    }

    #[test]
    fn test_node_with_metadata() {
        let conn = PartitionConnection::in_memory("test").unwrap();

        let mut node = create_test_node("test.py:async_func", "async_func", "test.py");
        node.metadata.is_async = Some(true);
        node.metadata.visibility = Some("public".to_string());
        node.metadata.decorators = Some(vec!["staticmethod".to_string()]);

        conn.insert_node(&node).unwrap();

        let retrieved = conn.get_node("test.py:async_func").unwrap().unwrap();
        assert_eq!(retrieved.metadata.is_async, Some(true));
        assert_eq!(retrieved.metadata.visibility, Some("public".to_string()));
        assert_eq!(
            retrieved.metadata.decorators,
            Some(vec!["staticmethod".to_string()])
        );
    }

    #[test]
    fn test_file_node_with_hash() {
        let conn = PartitionConnection::in_memory("test").unwrap();

        let node = Node {
            id: "test.py".to_string(),
            name: "test.py".to_string(),
            node_type: NodeType::Container,
            kind: Some(ContainerKind::File.as_str().to_string()),
            subtype: None,
            file: "test.py".to_string(),
            line: 1,
            end_line: 100,
            text: None,
            hash: Some("abc123".to_string()),
            metadata: NodeMetadata::default(),
        };

        conn.insert_node(&node).unwrap();

        let retrieved = conn.get_node("test.py").unwrap().unwrap();
        assert_eq!(retrieved.hash, Some("abc123".to_string()));
    }

    #[test]
    fn test_stats() {
        let conn = PartitionConnection::in_memory("test_partition").unwrap();

        conn.insert_node(&create_test_node("a", "a", "a.py"))
            .unwrap();
        conn.insert_node(&create_test_node("b", "b", "b.py"))
            .unwrap();
        conn.insert_edge(&create_test_edge("a", "b", EdgeType::Uses))
            .unwrap();

        let stats = conn.stats().unwrap();
        assert_eq!(stats.node_count, 2);
        assert_eq!(stats.edge_count, 1);
        assert_eq!(stats.partition_id, "test_partition");
    }

    #[test]
    fn test_clear() {
        let conn = PartitionConnection::in_memory("test").unwrap();

        conn.insert_node(&create_test_node("a", "a", "a.py"))
            .unwrap();
        conn.insert_edge(&create_test_edge("a", "b", EdgeType::Uses))
            .unwrap();

        conn.clear().unwrap();

        assert_eq!(conn.node_count().unwrap(), 0);
        assert_eq!(conn.edge_count().unwrap(), 0);
    }

    #[test]
    fn test_depends_on_edge_with_metadata() {
        let conn = PartitionConnection::in_memory("test").unwrap();

        // Create a DependsOn edge with version_spec and is_dev_dependency
        let edge = Edge {
            source: "pkg/foo".to_string(),
            target: "pkg/bar".to_string(),
            edge_type: EdgeType::DependsOn,
            ref_line: None,
            ident: Some("bar".to_string()),
            version_spec: Some("^1.0.0".to_string()),
            is_dev_dependency: Some(true),
        };

        conn.insert_edge(&edge).unwrap();
        assert_eq!(conn.edge_count().unwrap(), 1);

        // Query the edge back
        let edges = conn.query_edges_by_source("pkg/foo").unwrap();
        assert_eq!(edges.len(), 1);

        let retrieved = &edges[0];
        assert_eq!(retrieved.source, "pkg/foo");
        assert_eq!(retrieved.target, "pkg/bar");
        assert_eq!(retrieved.edge_type, EdgeType::DependsOn);
        assert_eq!(retrieved.ident, Some("bar".to_string()));
        assert_eq!(retrieved.version_spec, Some("^1.0.0".to_string()));
        assert_eq!(retrieved.is_dev_dependency, Some(true));
    }

    #[test]
    fn test_migrate_v1_0_to_v1_1() {
        use rusqlite::Connection;

        // Create a temporary directory for the test
        let temp_dir = tempfile::tempdir().unwrap();
        let db_path = temp_dir.path().join("test_partition.db");

        // Create a v1.0 partition manually (old schema without new columns)
        {
            let conn = Connection::open(&db_path).unwrap();

            // Create v1.0 schema (without version_spec and is_dev_dependency)
            conn.execute(
                r#"
                CREATE TABLE nodes (
                    id TEXT PRIMARY KEY NOT NULL,
                    name TEXT NOT NULL,
                    node_type TEXT NOT NULL,
                    kind TEXT,
                    subtype TEXT,
                    file TEXT NOT NULL,
                    line INTEGER NOT NULL,
                    end_line INTEGER NOT NULL,
                    text TEXT,
                    hash TEXT,
                    metadata_json TEXT
                )
                "#,
                [],
            )
            .unwrap();

            conn.execute(
                r#"
                CREATE TABLE edges (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    source TEXT NOT NULL,
                    target TEXT NOT NULL,
                    edge_type TEXT NOT NULL,
                    ref_line INTEGER,
                    ident TEXT,
                    UNIQUE(source, target, edge_type, ref_line)
                )
                "#,
                [],
            )
            .unwrap();

            conn.execute(
                r#"
                CREATE TABLE partition_metadata (
                    key TEXT PRIMARY KEY NOT NULL,
                    value TEXT NOT NULL
                )
                "#,
                [],
            )
            .unwrap();

            // Set v1.0 schema version
            conn.execute(
                "INSERT INTO partition_metadata (key, value) VALUES ('schema_version', '1.0')",
                [],
            )
            .unwrap();

            // Insert an edge with v1.0 schema
            conn.execute(
                "INSERT INTO edges (source, target, edge_type, ref_line, ident) VALUES (?1, ?2, ?3, ?4, ?5)",
                params!["a", "b", "USES", 10i64, "test_ident"],
            )
            .unwrap();
        }

        // Open the partition with PartitionConnection - this should trigger migration
        let conn = PartitionConnection::open(&db_path, "test_partition").unwrap();

        // Verify schema version is now 1.1
        let version = conn.get_metadata("schema_version").unwrap();
        assert_eq!(version, Some("1.1".to_string()));

        // Verify old edges can still be read (new columns should be None)
        let edges = conn.query_edges_by_source("a").unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].source, "a");
        assert_eq!(edges[0].target, "b");
        assert_eq!(edges[0].edge_type, EdgeType::Uses);
        assert_eq!(edges[0].ref_line, Some(10));
        assert_eq!(edges[0].ident, Some("test_ident".to_string()));
        assert_eq!(edges[0].version_spec, None); // Backward compat
        assert_eq!(edges[0].is_dev_dependency, None); // Backward compat

        // Insert a new edge with v1.1 schema
        let new_edge = Edge {
            source: "c".to_string(),
            target: "d".to_string(),
            edge_type: EdgeType::DependsOn,
            ref_line: None,
            ident: Some("dep".to_string()),
            version_spec: Some(">=2.0".to_string()),
            is_dev_dependency: Some(false),
        };
        conn.insert_edge(&new_edge).unwrap();

        // Verify the new edge is stored with all fields
        let new_edges = conn.query_edges_by_source("c").unwrap();
        assert_eq!(new_edges.len(), 1);
        assert_eq!(new_edges[0].version_spec, Some(">=2.0".to_string()));
        assert_eq!(new_edges[0].is_dev_dependency, Some(false));
    }
}
