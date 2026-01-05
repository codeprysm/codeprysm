//! CodePrysm Core - Code graph generation using tree-sitter AST parsing
//!
//! This crate provides the core functionality for code graph generation:
//! - Tree-sitter AST parsing for multiple languages
//! - Merkle tree-based change detection for incremental updates
//! - Graph schema and construction
//! - Tag parsing for declarative SCM queries
//! - Incremental updates for efficient repository synchronization

// Implemented modules
pub mod builder;
pub mod discovery;
pub mod embedded_queries;
pub mod graph;
pub mod incremental;
pub mod lazy;
pub mod manifest;
pub mod merkle;
pub mod parser;
pub mod tags;

// Embedded queries re-exports
pub use embedded_queries::{get_query, has_embedded_query, supported_languages};

// Re-exports for convenience
pub use graph::{
    CallableKind, ContainerKind, DataKind, Edge, EdgeData, EdgeType, Node, NodeKind, NodeMetadata,
    NodeType, PetCodeGraph, GRAPH_SCHEMA_VERSION,
};
pub use merkle::{compute_file_hash, ChangeSet, ExclusionFilter, MerkleTreeManager, TreeStats};
pub use parser::{
    generate_node_id, parse_node_id, CodeParser, ContainmentContext, ContainmentEntry,
    ExtractedTag, ManifestLanguage, MetadataExtractor, ParserError, QueryManager,
    SupportedLanguage, TagExtractor,
};
pub use tags::{parse_tag_string, TagCategory, TagParseError, TagParseResult};

// Builder re-exports
pub use builder::{
    BuilderConfig, BuilderError, ComponentBuilder, DiscoveredComponent, GraphBuilder,
};

// Incremental updater re-exports
pub use incremental::{IncrementalUpdater, UpdateResult, UpdaterError};

// Discovery re-exports
pub use discovery::{DiscoveredRoot, DiscoveryConfig, DiscoveryError, RootDiscovery, RootType};

// Manifest re-exports
pub use manifest::{DependencyType, LocalDependency, ManifestError, ManifestInfo, ManifestParser};
