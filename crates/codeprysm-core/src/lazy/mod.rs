//! Lazy-Loading Graph Module
//!
//! This module provides lazy-loading capabilities for large code graphs:
//! - SQLite-backed partition storage
//! - On-demand partition loading into petgraph
//! - Memory-based eviction with LRU tracking
//! - Cross-partition edge indexing
//!
//! # Architecture
//!
//! ```text
//! LazyGraphManager
//! ├── PetCodeGraph (runtime graph, all loaded partitions)
//! ├── PartitionRegistry (tracks loaded partitions)
//! ├── MemoryBudgetCache (LRU eviction by bytes)
//! └── CrossRefIndex (cross-partition edges)
//!
//! Storage:
//! ├── manifest.json (file → partition mapping)
//! ├── partitions/*.db (SQLite partition files)
//! └── cross_refs.db (cross-partition edges)
//! ```

pub mod cache;
pub mod cross_refs;
pub mod manager;
pub mod partition;
pub mod partitioner;
pub mod schema;

// Re-exports
pub use cache::{
    estimate_memory, CacheMetrics, MemoryBudgetCache, PartitionStats as CachePartitionStats,
};
pub use cross_refs::{
    CrossRef, CrossRefError, CrossRefIndex, CrossRefStore, CROSS_REFS_SCHEMA_VERSION,
};
pub use manager::{LazyGraphError, LazyGraphManager, LazyGraphStats, Manifest, ManifestEntry};
pub use partition::{PartitionConnection, PartitionError, PartitionStats};
pub use partitioner::{GraphPartitioner, PartitionerError, PartitioningStats};
pub use schema::{
    EDGE_COLUMNS, NODE_COLUMNS, PARTITION_SCHEMA_VERSION, SCHEMA_CREATE_EDGES,
    SCHEMA_CREATE_INDEXES, SCHEMA_CREATE_METADATA, SCHEMA_CREATE_NODES,
};
