//! Cross-Partition Edge Index and Storage
//!
//! This module handles edges that span partition boundaries. Cross-partition
//! edges are stored separately from regular partition edges and are always
//! loaded into memory for efficient cross-partition queries.
//!
//! # Architecture
//!
//! ```text
//! CrossRefIndex (in-memory)
//! ├── by_target: HashMap<node_id, Vec<CrossRef>>  # Find what calls this
//! └── by_source: HashMap<node_id, Vec<CrossRef>>  # Find what this calls
//!
//! CrossRefStore (SQLite: cross_refs.db)
//! └── Persists CrossRefIndex to disk
//! ```

use crate::graph::EdgeType;
use rusqlite::{params, Connection, Result as SqliteResult};
use std::collections::HashMap;
use std::path::Path;
use thiserror::Error;

/// Schema version for cross_refs.db
/// v1.1 adds version_spec and is_dev_dependency columns for DEPENDS_ON edges
pub const CROSS_REFS_SCHEMA_VERSION: &str = "1.1";

/// SQL to create the cross_refs table
const SCHEMA_CREATE_CROSS_REFS: &str = r#"
CREATE TABLE IF NOT EXISTS cross_refs (
    -- Auto-incrementing ID
    id INTEGER PRIMARY KEY AUTOINCREMENT,

    -- Source node info
    source_id TEXT NOT NULL,
    source_partition TEXT NOT NULL,

    -- Target node info
    target_id TEXT NOT NULL,
    target_partition TEXT NOT NULL,

    -- Edge metadata
    edge_type TEXT NOT NULL,
    ref_line INTEGER,
    ident TEXT,

    -- DependsOn edge metadata (v1.1)
    version_spec TEXT,
    is_dev_dependency INTEGER,

    -- Ensure no duplicate cross-refs
    UNIQUE(source_id, target_id, edge_type, ref_line)
)
"#;

/// SQL to create indexes for efficient queries
const SCHEMA_CREATE_CROSS_REFS_INDEXES: &str = r#"
-- Index for finding what references a target
CREATE INDEX IF NOT EXISTS idx_cross_refs_target ON cross_refs(target_id);

-- Index for finding what a source references
CREATE INDEX IF NOT EXISTS idx_cross_refs_source ON cross_refs(source_id);

-- Index for partition-based cleanup
CREATE INDEX IF NOT EXISTS idx_cross_refs_source_partition ON cross_refs(source_partition);
CREATE INDEX IF NOT EXISTS idx_cross_refs_target_partition ON cross_refs(target_partition);
"#;

/// SQL to create the metadata table
const SCHEMA_CREATE_METADATA: &str = r#"
CREATE TABLE IF NOT EXISTS cross_refs_metadata (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
)
"#;

/// Errors that can occur during cross-ref operations
#[derive(Debug, Error)]
pub enum CrossRefError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Schema version mismatch: expected {expected}, found {found}")]
    SchemaVersionMismatch { expected: String, found: String },

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),
}

/// A cross-partition edge reference
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CrossRef {
    /// Source node ID (e.g., "src/main.rs:main")
    pub source_id: String,
    /// Partition containing the source node
    pub source_partition: String,
    /// Target node ID (e.g., "src/lib.rs:helper")
    pub target_id: String,
    /// Partition containing the target node
    pub target_partition: String,
    /// Type of edge (typically USES for cross-partition, DEPENDS_ON for components)
    pub edge_type: EdgeType,
    /// Line number where the reference occurs
    pub ref_line: Option<usize>,
    /// Identifier text at the reference site
    pub ident: Option<String>,
    /// Version specification (for DEPENDS_ON edges)
    pub version_spec: Option<String>,
    /// Whether this is a development dependency (for DEPENDS_ON edges)
    pub is_dev_dependency: Option<bool>,
}

impl CrossRef {
    /// Create a new cross-reference
    pub fn new(
        source_id: String,
        source_partition: String,
        target_id: String,
        target_partition: String,
        edge_type: EdgeType,
        ref_line: Option<usize>,
        ident: Option<String>,
    ) -> Self {
        Self {
            source_id,
            source_partition,
            target_id,
            target_partition,
            edge_type,
            ref_line,
            ident,
            version_spec: None,
            is_dev_dependency: None,
        }
    }

    /// Create a new cross-reference with dependency metadata (for DEPENDS_ON edges)
    pub fn with_dependency(
        source_id: String,
        source_partition: String,
        target_id: String,
        target_partition: String,
        ident: Option<String>,
        version_spec: Option<String>,
        is_dev_dependency: Option<bool>,
    ) -> Self {
        Self {
            source_id,
            source_partition,
            target_id,
            target_partition,
            edge_type: EdgeType::DependsOn,
            ref_line: None,
            ident,
            version_spec,
            is_dev_dependency,
        }
    }
}

/// In-memory index for cross-partition edges
///
/// This index is always fully loaded to enable efficient cross-partition queries.
/// Cross-partition edges typically represent <5% of total edges.
#[derive(Debug, Default)]
pub struct CrossRefIndex {
    /// Edges indexed by target node ID (find incoming references)
    by_target: HashMap<String, Vec<CrossRef>>,
    /// Edges indexed by source node ID (find outgoing references)
    by_source: HashMap<String, Vec<CrossRef>>,
}

impl CrossRefIndex {
    /// Create a new empty index
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a cross-reference to the index
    pub fn add(&mut self, cross_ref: CrossRef) {
        // Index by target
        self.by_target
            .entry(cross_ref.target_id.clone())
            .or_default()
            .push(cross_ref.clone());

        // Index by source
        self.by_source
            .entry(cross_ref.source_id.clone())
            .or_default()
            .push(cross_ref);
    }

    /// Add multiple cross-references
    pub fn add_all(&mut self, cross_refs: impl IntoIterator<Item = CrossRef>) {
        for cross_ref in cross_refs {
            self.add(cross_ref);
        }
    }

    /// Get all cross-references targeting a specific node
    pub fn get_by_target(&self, target_id: &str) -> Option<&Vec<CrossRef>> {
        self.by_target.get(target_id)
    }

    /// Get all cross-references from a specific source node
    pub fn get_by_source(&self, source_id: &str) -> Option<&Vec<CrossRef>> {
        self.by_source.get(source_id)
    }

    /// Remove all cross-references where source is from a specific partition
    pub fn remove_by_source_partition(&mut self, partition: &str) {
        // Collect source IDs to remove
        let source_ids_to_remove: Vec<String> = self
            .by_source
            .iter()
            .filter(|(_, refs)| refs.iter().any(|r| r.source_partition == partition))
            .map(|(id, _)| id.clone())
            .collect();

        // Remove from by_source
        for source_id in &source_ids_to_remove {
            self.by_source.remove(source_id);
        }

        // Remove from by_target
        for refs in self.by_target.values_mut() {
            refs.retain(|r| r.source_partition != partition);
        }

        // Remove empty entries
        self.by_target.retain(|_, refs| !refs.is_empty());
    }

    /// Remove all cross-references involving a specific partition (source or target)
    pub fn remove_by_partition(&mut self, partition: &str) {
        // Remove from by_source
        self.by_source.retain(|_, refs| {
            refs.retain(|r| r.source_partition != partition && r.target_partition != partition);
            !refs.is_empty()
        });

        // Remove from by_target
        self.by_target.retain(|_, refs| {
            refs.retain(|r| r.source_partition != partition && r.target_partition != partition);
            !refs.is_empty()
        });
    }

    /// Get total number of cross-references
    pub fn len(&self) -> usize {
        self.by_source.values().map(|v| v.len()).sum()
    }

    /// Check if the index is empty
    pub fn is_empty(&self) -> bool {
        self.by_source.is_empty()
    }

    /// Get all unique source partitions
    pub fn source_partitions(&self) -> impl Iterator<Item = &str> {
        self.by_source
            .values()
            .flat_map(|refs| refs.iter().map(|r| r.source_partition.as_str()))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
    }

    /// Get all unique target partitions
    pub fn target_partitions(&self) -> impl Iterator<Item = &str> {
        self.by_target
            .values()
            .flat_map(|refs| refs.iter().map(|r| r.target_partition.as_str()))
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
    }

    /// Clear all cross-references
    pub fn clear(&mut self) {
        self.by_source.clear();
        self.by_target.clear();
    }

    /// Iterate over all cross-references
    pub fn iter(&self) -> impl Iterator<Item = &CrossRef> {
        self.by_source.values().flat_map(|refs| refs.iter())
    }
}

/// SQLite-backed storage for cross-partition edges
///
/// The store persists the `CrossRefIndex` to disk and loads it fully
/// on startup. This is efficient because cross-partition edges are
/// typically a small fraction of total edges.
pub struct CrossRefStore {
    conn: Connection,
}

impl CrossRefStore {
    /// Open an existing cross_refs.db
    pub fn open(path: &Path) -> Result<Self, CrossRefError> {
        let conn = Connection::open(path)?;
        Self::configure_connection(&conn)?;

        let store = Self { conn };

        // Verify schema version
        if let Some(version) = store.get_metadata("schema_version")? {
            if version != CROSS_REFS_SCHEMA_VERSION {
                return Err(CrossRefError::SchemaVersionMismatch {
                    expected: CROSS_REFS_SCHEMA_VERSION.to_string(),
                    found: version,
                });
            }
        }

        Ok(store)
    }

    /// Create a new cross_refs.db with schema
    pub fn create(path: &Path) -> Result<Self, CrossRefError> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(path)?;
        Self::configure_connection(&conn)?;

        // Create schema
        conn.execute(SCHEMA_CREATE_CROSS_REFS, [])?;
        conn.execute(SCHEMA_CREATE_METADATA, [])?;
        conn.execute_batch(SCHEMA_CREATE_CROSS_REFS_INDEXES)?;

        let store = Self { conn };

        // Store schema version
        store.set_metadata("schema_version", CROSS_REFS_SCHEMA_VERSION)?;

        Ok(store)
    }

    /// Create an in-memory cross_refs database (for testing)
    pub fn in_memory() -> Result<Self, CrossRefError> {
        let conn = Connection::open_in_memory()?;
        Self::configure_connection(&conn)?;

        // Create schema
        conn.execute(SCHEMA_CREATE_CROSS_REFS, [])?;
        conn.execute(SCHEMA_CREATE_METADATA, [])?;
        conn.execute_batch(SCHEMA_CREATE_CROSS_REFS_INDEXES)?;

        let store = Self { conn };

        store.set_metadata("schema_version", CROSS_REFS_SCHEMA_VERSION)?;

        Ok(store)
    }

    /// Configure connection with optimal settings
    fn configure_connection(conn: &Connection) -> SqliteResult<()> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.pragma_update(None, "cache_size", -16000)?; // 16MB cache (smaller than partitions)
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "temp_store", "MEMORY")?;
        Ok(())
    }

    /// Get a metadata value
    fn get_metadata(&self, key: &str) -> Result<Option<String>, CrossRefError> {
        let result = self
            .conn
            .query_row(
                "SELECT value FROM cross_refs_metadata WHERE key = ?1",
                [key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }

    /// Set a metadata value
    fn set_metadata(&self, key: &str, value: &str) -> Result<(), CrossRefError> {
        self.conn.execute(
            "INSERT OR REPLACE INTO cross_refs_metadata (key, value) VALUES (?1, ?2)",
            params![key, value],
        )?;
        Ok(())
    }

    /// Load all cross-references into a CrossRefIndex
    pub fn load_all(&self) -> Result<CrossRefIndex, CrossRefError> {
        let mut index = CrossRefIndex::new();

        let mut stmt = self.conn.prepare(
            "SELECT source_id, source_partition, target_id, target_partition, edge_type, ref_line, ident, version_spec, is_dev_dependency FROM cross_refs",
        )?;

        let rows = stmt.query_map([], |row| {
            let edge_type_str: String = row.get(4)?;
            let edge_type = match edge_type_str.as_str() {
                "CONTAINS" => EdgeType::Contains,
                "USES" => EdgeType::Uses,
                "DEFINES" => EdgeType::Defines,
                "DEPENDS_ON" => EdgeType::DependsOn,
                _ => EdgeType::Uses, // Default fallback
            };

            Ok(CrossRef {
                source_id: row.get(0)?,
                source_partition: row.get(1)?,
                target_id: row.get(2)?,
                target_partition: row.get(3)?,
                edge_type,
                ref_line: row.get::<_, Option<i64>>(5)?.map(|v| v as usize),
                ident: row.get(6)?,
                version_spec: row.get(7)?,
                is_dev_dependency: row.get::<_, Option<i64>>(8)?.map(|v| v != 0),
            })
        })?;

        for row in rows {
            index.add(row?);
        }

        Ok(index)
    }

    /// Save a CrossRefIndex to the database (replaces all existing data)
    pub fn save_all(&self, index: &CrossRefIndex) -> Result<(), CrossRefError> {
        let tx = self.conn.unchecked_transaction()?;

        // Clear existing data
        tx.execute("DELETE FROM cross_refs", [])?;

        // Insert all cross-refs
        let mut stmt = tx.prepare(
            "INSERT INTO cross_refs (source_id, source_partition, target_id, target_partition, edge_type, ref_line, ident, version_spec, is_dev_dependency) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;

        for cross_ref in index.iter() {
            stmt.execute(params![
                cross_ref.source_id,
                cross_ref.source_partition,
                cross_ref.target_id,
                cross_ref.target_partition,
                cross_ref.edge_type.as_str(),
                cross_ref.ref_line.map(|v| v as i64),
                cross_ref.ident,
                cross_ref.version_spec,
                cross_ref
                    .is_dev_dependency
                    .map(|b| if b { 1i64 } else { 0i64 }),
            ])?;
        }

        drop(stmt);
        tx.commit()?;

        Ok(())
    }

    /// Add cross-references (appends to existing data)
    pub fn add_refs(&self, refs: &[CrossRef]) -> Result<(), CrossRefError> {
        let tx = self.conn.unchecked_transaction()?;

        let mut stmt = tx.prepare(
            "INSERT OR IGNORE INTO cross_refs (source_id, source_partition, target_id, target_partition, edge_type, ref_line, ident, version_spec, is_dev_dependency) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )?;

        for cross_ref in refs {
            stmt.execute(params![
                cross_ref.source_id,
                cross_ref.source_partition,
                cross_ref.target_id,
                cross_ref.target_partition,
                cross_ref.edge_type.as_str(),
                cross_ref.ref_line.map(|v| v as i64),
                cross_ref.ident,
                cross_ref.version_spec,
                cross_ref
                    .is_dev_dependency
                    .map(|b| if b { 1i64 } else { 0i64 }),
            ])?;
        }

        drop(stmt);
        tx.commit()?;

        Ok(())
    }

    /// Remove all cross-references involving a specific partition
    pub fn remove_refs_by_partition(&self, partition: &str) -> Result<usize, CrossRefError> {
        let deleted = self.conn.execute(
            "DELETE FROM cross_refs WHERE source_partition = ?1 OR target_partition = ?1",
            [partition],
        )?;
        Ok(deleted)
    }

    /// Remove cross-references where source is from a specific partition
    pub fn remove_refs_by_source_partition(&self, partition: &str) -> Result<usize, CrossRefError> {
        let deleted = self.conn.execute(
            "DELETE FROM cross_refs WHERE source_partition = ?1",
            [partition],
        )?;
        Ok(deleted)
    }

    /// Get count of cross-references
    pub fn count(&self) -> Result<usize, CrossRefError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM cross_refs", [], |row| row.get(0))?;
        Ok(count as usize)
    }
}

// Import OptionalExtension trait for .optional() method
use rusqlite::OptionalExtension;

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_cross_ref(n: usize) -> CrossRef {
        CrossRef::new(
            format!("src/mod{}.rs:func{}", n, n),
            format!("partition_{}", n % 3),
            format!("src/lib.rs:helper{}", n % 5),
            "partition_lib".to_string(),
            EdgeType::Uses,
            Some(10 + n),
            Some(format!("helper{}", n % 5)),
        )
    }

    // =========================================================================
    // CrossRefIndex Tests
    // =========================================================================

    #[test]
    fn test_index_add_and_get() {
        let mut index = CrossRefIndex::new();

        let cross_ref = sample_cross_ref(1);
        index.add(cross_ref.clone());

        // Get by target
        let by_target = index.get_by_target("src/lib.rs:helper1").unwrap();
        assert_eq!(by_target.len(), 1);
        assert_eq!(by_target[0], cross_ref);

        // Get by source
        let by_source = index.get_by_source("src/mod1.rs:func1").unwrap();
        assert_eq!(by_source.len(), 1);
        assert_eq!(by_source[0], cross_ref);
    }

    #[test]
    fn test_index_multiple_refs_to_same_target() {
        let mut index = CrossRefIndex::new();

        // Add multiple refs to the same target
        for i in 0..5 {
            index.add(CrossRef::new(
                format!("src/mod{}.rs:caller{}", i, i),
                format!("partition_{}", i),
                "src/lib.rs:shared_func".to_string(),
                "partition_lib".to_string(),
                EdgeType::Uses,
                Some(10 + i),
                Some("shared_func".to_string()),
            ));
        }

        let refs = index.get_by_target("src/lib.rs:shared_func").unwrap();
        assert_eq!(refs.len(), 5);
    }

    #[test]
    fn test_index_remove_by_source_partition() {
        let mut index = CrossRefIndex::new();

        // Add refs from different partitions
        for i in 0..10 {
            index.add(sample_cross_ref(i));
        }

        let initial_count = index.len();
        assert!(initial_count > 0);

        // Remove partition_0 (should be n=0,3,6,9)
        index.remove_by_source_partition("partition_0");

        // Verify removal
        for source_refs in index.by_source.values() {
            for r in source_refs {
                assert_ne!(r.source_partition, "partition_0");
            }
        }
    }

    #[test]
    fn test_index_remove_by_partition() {
        let mut index = CrossRefIndex::new();

        // Add refs
        index.add(CrossRef::new(
            "a:func".to_string(),
            "part_a".to_string(),
            "b:target".to_string(),
            "part_b".to_string(),
            EdgeType::Uses,
            None,
            None,
        ));
        index.add(CrossRef::new(
            "c:func".to_string(),
            "part_c".to_string(),
            "a:target".to_string(),
            "part_a".to_string(),
            EdgeType::Uses,
            None,
            None,
        ));
        index.add(CrossRef::new(
            "d:func".to_string(),
            "part_d".to_string(),
            "e:target".to_string(),
            "part_e".to_string(),
            EdgeType::Uses,
            None,
            None,
        ));

        assert_eq!(index.len(), 3);

        // Remove partition_a (should remove first two)
        index.remove_by_partition("part_a");

        assert_eq!(index.len(), 1);
    }

    #[test]
    fn test_index_len_and_is_empty() {
        let mut index = CrossRefIndex::new();
        assert!(index.is_empty());
        assert_eq!(index.len(), 0);

        index.add(sample_cross_ref(1));
        assert!(!index.is_empty());
        assert_eq!(index.len(), 1);

        index.add(sample_cross_ref(2));
        assert_eq!(index.len(), 2);
    }

    #[test]
    fn test_index_clear() {
        let mut index = CrossRefIndex::new();

        for i in 0..5 {
            index.add(sample_cross_ref(i));
        }

        assert!(!index.is_empty());
        index.clear();
        assert!(index.is_empty());
    }

    #[test]
    fn test_index_iter() {
        let mut index = CrossRefIndex::new();

        for i in 0..5 {
            index.add(sample_cross_ref(i));
        }

        let collected: Vec<_> = index.iter().collect();
        assert_eq!(collected.len(), 5);
    }

    // =========================================================================
    // CrossRefStore Tests
    // =========================================================================

    #[test]
    fn test_store_create_and_open() {
        let store = CrossRefStore::in_memory().unwrap();
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn test_store_save_and_load() {
        let store = CrossRefStore::in_memory().unwrap();

        // Create index with data
        let mut index = CrossRefIndex::new();
        for i in 0..10 {
            index.add(sample_cross_ref(i));
        }

        // Save to store
        store.save_all(&index).unwrap();
        assert_eq!(store.count().unwrap(), 10);

        // Load back
        let loaded = store.load_all().unwrap();
        assert_eq!(loaded.len(), 10);

        // Verify data integrity
        for i in 0..10 {
            let target = format!("src/lib.rs:helper{}", i % 5);
            let refs = loaded.get_by_target(&target);
            assert!(refs.is_some());
        }
    }

    #[test]
    fn test_store_add_refs() {
        let store = CrossRefStore::in_memory().unwrap();

        // Add first batch
        let refs1: Vec<_> = (0..5).map(sample_cross_ref).collect();
        store.add_refs(&refs1).unwrap();
        assert_eq!(store.count().unwrap(), 5);

        // Add second batch
        let refs2: Vec<_> = (5..10).map(sample_cross_ref).collect();
        store.add_refs(&refs2).unwrap();
        assert_eq!(store.count().unwrap(), 10);
    }

    #[test]
    fn test_store_remove_by_partition() {
        let store = CrossRefStore::in_memory().unwrap();

        // Add refs from different partitions
        let refs: Vec<_> = (0..10).map(sample_cross_ref).collect();
        store.add_refs(&refs).unwrap();

        let initial = store.count().unwrap();
        assert_eq!(initial, 10);

        // Remove partition_lib (target for all)
        let removed = store.remove_refs_by_partition("partition_lib").unwrap();
        assert_eq!(removed, 10);
        assert_eq!(store.count().unwrap(), 0);
    }

    #[test]
    fn test_store_remove_by_source_partition() {
        let store = CrossRefStore::in_memory().unwrap();

        // Add refs
        let refs: Vec<_> = (0..9).map(sample_cross_ref).collect();
        store.add_refs(&refs).unwrap();
        assert_eq!(store.count().unwrap(), 9);

        // Remove partition_0 (n=0,3,6 -> 3 refs)
        let removed = store
            .remove_refs_by_source_partition("partition_0")
            .unwrap();
        assert_eq!(removed, 3);
        assert_eq!(store.count().unwrap(), 6);
    }

    #[test]
    fn test_store_roundtrip_edge_types() {
        let store = CrossRefStore::in_memory().unwrap();

        let mut index = CrossRefIndex::new();
        index.add(CrossRef::new(
            "a:x".to_string(),
            "p1".to_string(),
            "b:y".to_string(),
            "p2".to_string(),
            EdgeType::Contains,
            None,
            None,
        ));
        index.add(CrossRef::new(
            "c:x".to_string(),
            "p1".to_string(),
            "d:y".to_string(),
            "p2".to_string(),
            EdgeType::Uses,
            Some(42),
            Some("ident".to_string()),
        ));
        index.add(CrossRef::new(
            "e:x".to_string(),
            "p1".to_string(),
            "f:y".to_string(),
            "p2".to_string(),
            EdgeType::Defines,
            None,
            None,
        ));

        store.save_all(&index).unwrap();
        let loaded = store.load_all().unwrap();

        assert_eq!(loaded.len(), 3);

        // Verify edge types preserved
        let contains_refs = loaded.get_by_source("a:x").unwrap();
        assert_eq!(contains_refs[0].edge_type, EdgeType::Contains);

        let uses_refs = loaded.get_by_source("c:x").unwrap();
        assert_eq!(uses_refs[0].edge_type, EdgeType::Uses);
        assert_eq!(uses_refs[0].ref_line, Some(42));
        assert_eq!(uses_refs[0].ident, Some("ident".to_string()));

        let defines_refs = loaded.get_by_source("e:x").unwrap();
        assert_eq!(defines_refs[0].edge_type, EdgeType::Defines);
    }

    #[test]
    fn test_store_duplicate_handling() {
        let store = CrossRefStore::in_memory().unwrap();

        let cross_ref = sample_cross_ref(1);

        // Add same ref twice via add_refs (uses INSERT OR IGNORE)
        store.add_refs(std::slice::from_ref(&cross_ref)).unwrap();
        store.add_refs(std::slice::from_ref(&cross_ref)).unwrap();

        // Should only have one entry
        assert_eq!(store.count().unwrap(), 1);
    }
}
