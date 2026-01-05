//! SQLite Schema Definitions for Partition Storage
//!
//! This module defines the SQLite schema for storing code graph partitions.
//! Each partition is a self-contained SQLite database containing nodes and edges
//! for a subset of the repository (typically by directory).

/// Schema version for partition databases
/// v1.1 adds version_spec and is_dev_dependency columns for DEPENDS_ON edges
pub const PARTITION_SCHEMA_VERSION: &str = "1.1";

/// SQL to create the nodes table
///
/// Stores code entities (Container, Callable, Data) with their metadata.
/// Note: Files are stored as Container with kind="file". Legacy "FILE" type is converted on read.
/// The `id` field is the hierarchical node ID (e.g., "file.py:Module:Class:Method").
pub const SCHEMA_CREATE_NODES: &str = r#"
CREATE TABLE IF NOT EXISTS nodes (
    -- Primary identification
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,

    -- Type classification (Container, Callable, Data). Legacy FILE stored here converts to Container.
    node_type TEXT NOT NULL,

    -- Kind within type (e.g., "function", "type", "field")
    kind TEXT,

    -- Language-specific subtype (e.g., "struct", "interface", "class")
    subtype TEXT,

    -- Source location
    file TEXT NOT NULL,
    line INTEGER NOT NULL,
    end_line INTEGER NOT NULL,

    -- Optional source code text
    text TEXT,

    -- File content hash (only for file nodes - Container with kind="file")
    hash TEXT,

    -- Semantic metadata (JSON blob for flexibility)
    metadata_json TEXT
)
"#;

/// SQL to create the edges table
///
/// Stores relationships between nodes within this partition.
/// Cross-partition edges are stored separately in cross_refs.db.
pub const SCHEMA_CREATE_EDGES: &str = r#"
CREATE TABLE IF NOT EXISTS edges (
    -- Auto-incrementing ID for stable references
    id INTEGER PRIMARY KEY AUTOINCREMENT,

    -- Source and target node IDs
    source TEXT NOT NULL,
    target TEXT NOT NULL,

    -- Relationship type (CONTAINS, USES, DEFINES, DEPENDS_ON)
    edge_type TEXT NOT NULL,

    -- Line number where the reference occurs (for USES edges)
    ref_line INTEGER,

    -- The identifier text at the reference site
    ident TEXT,

    -- DependsOn edge metadata (v1.1)
    version_spec TEXT,
    is_dev_dependency INTEGER,

    -- Ensure no duplicate edges
    UNIQUE(source, target, edge_type, ref_line)
)
"#;

/// SQL to create indexes for efficient queries
pub const SCHEMA_CREATE_INDEXES: &str = r#"
-- Index on file path for file-based queries
CREATE INDEX IF NOT EXISTS idx_nodes_file ON nodes(file);

-- Index on node type for type filtering
CREATE INDEX IF NOT EXISTS idx_nodes_type ON nodes(node_type);

-- Index on kind for kind filtering
CREATE INDEX IF NOT EXISTS idx_nodes_kind ON nodes(kind);

-- Index on source for outgoing edge queries
CREATE INDEX IF NOT EXISTS idx_edges_source ON edges(source);

-- Index on target for incoming edge queries
CREATE INDEX IF NOT EXISTS idx_edges_target ON edges(target);

-- Index on edge type for relationship filtering
CREATE INDEX IF NOT EXISTS idx_edges_type ON edges(edge_type);

-- Composite index for reference lookups
CREATE INDEX IF NOT EXISTS idx_edges_source_type ON edges(source, edge_type);
CREATE INDEX IF NOT EXISTS idx_edges_target_type ON edges(target, edge_type);
"#;

/// SQL to create the metadata table
///
/// Stores partition-level metadata like schema version and stats.
pub const SCHEMA_CREATE_METADATA: &str = r#"
CREATE TABLE IF NOT EXISTS partition_metadata (
    key TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
)
"#;

/// Column names for node queries (in order for row mapping)
pub const NODE_COLUMNS: &str =
    "id, name, node_type, kind, subtype, file, line, end_line, text, hash, metadata_json";

/// Column names for edge queries (in order for row mapping)
pub const EDGE_COLUMNS: &str =
    "id, source, target, edge_type, ref_line, ident, version_spec, is_dev_dependency";

/// Migration SQL from v1.0 to v1.1
///
/// Adds version_spec and is_dev_dependency columns to edges table.
/// These columns are nullable to maintain backward compatibility.
pub const MIGRATION_V1_0_TO_V1_1: &str = r#"
ALTER TABLE edges ADD COLUMN version_spec TEXT;
ALTER TABLE edges ADD COLUMN is_dev_dependency INTEGER;
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn test_schema_creates_tables() {
        let conn = Connection::open_in_memory().unwrap();

        // Create all tables
        conn.execute(SCHEMA_CREATE_NODES, []).unwrap();
        conn.execute(SCHEMA_CREATE_EDGES, []).unwrap();
        conn.execute(SCHEMA_CREATE_METADATA, []).unwrap();

        // Verify tables exist
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"nodes".to_string()));
        assert!(tables.contains(&"edges".to_string()));
        assert!(tables.contains(&"partition_metadata".to_string()));
    }

    #[test]
    fn test_schema_creates_indexes() {
        let conn = Connection::open_in_memory().unwrap();

        // Create tables first
        conn.execute(SCHEMA_CREATE_NODES, []).unwrap();
        conn.execute(SCHEMA_CREATE_EDGES, []).unwrap();

        // Create indexes
        conn.execute_batch(SCHEMA_CREATE_INDEXES).unwrap();

        // Verify indexes exist
        let indexes: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='index' AND name LIKE 'idx_%' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(indexes.contains(&"idx_nodes_file".to_string()));
        assert!(indexes.contains(&"idx_nodes_type".to_string()));
        assert!(indexes.contains(&"idx_edges_source".to_string()));
        assert!(indexes.contains(&"idx_edges_target".to_string()));
    }
}
