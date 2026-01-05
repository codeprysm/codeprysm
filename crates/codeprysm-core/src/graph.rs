//! Graph Schema Definitions for Code Graph Model v2
//!
//! This module defines the schema for the Container/Callable/Data node model
//! with declarative tag-based categorization and semantic metadata.
//!
//! Schema Version: 2.0
//!
//! This module provides the `PetCodeGraph` implementation using petgraph for efficient
//! traversal and graph algorithms.

use petgraph::stable_graph::{EdgeIndex, NodeIndex, StableGraph};
use petgraph::visit::{EdgeRef, IntoEdgeReferences};
use petgraph::Direction;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Schema version constant
pub const GRAPH_SCHEMA_VERSION: &str = "2.0";

// ============================================================================
// Edge Types
// ============================================================================

/// Types of relationships between code entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum EdgeType {
    /// Hierarchical containment (File→Module, Module→Class, Class→Method)
    Contains,
    /// Dependencies (Callable→Callable, Callable→Data, Data→Data)
    Uses,
    /// Definition relationships (Container→Data, Callable→Data)
    Defines,
    /// Component dependency (Component→Component for local workspace dependencies)
    /// Used for: workspace:*, path dependencies, ProjectReference, replace directives
    DependsOn,
}

impl EdgeType {
    /// Get the string representation matching Python format
    pub fn as_str(&self) -> &'static str {
        match self {
            EdgeType::Contains => "CONTAINS",
            EdgeType::Uses => "USES",
            EdgeType::Defines => "DEFINES",
            EdgeType::DependsOn => "DEPENDS_ON",
        }
    }
}

// ============================================================================
// Node Types
// ============================================================================

/// High-level node type classification.
///
/// Note: The legacy `FILE` type has been removed. Files are now represented as
/// `Container` nodes with `kind="file"`. For backward compatibility, deserializing
/// "FILE" from JSON/SQLite is handled via custom deserialization logic that
/// converts it to `Container` with the appropriate kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum NodeType {
    /// Structural organization entity (namespace, module, class, file, etc.)
    Container,
    /// Executable code entity (function, method, constructor)
    Callable,
    /// State and value entity (constant, variable, field, parameter)
    Data,
}

impl NodeType {
    /// Get the string representation matching Python format
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeType::Container => "Container",
            NodeType::Callable => "Callable",
            NodeType::Data => "Data",
        }
    }
}

/// Custom deserializer to handle legacy "FILE" type.
/// Converts legacy "FILE" to Container (the kind must be set separately).
impl<'de> Deserialize<'de> for NodeType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        match s.as_str() {
            "Container" => Ok(NodeType::Container),
            "Callable" => Ok(NodeType::Callable),
            "Data" => Ok(NodeType::Data),
            // Legacy: "FILE" is now Container with kind="file"
            // Note: The kind field must be set to "file" by the caller
            "FILE" => Ok(NodeType::Container),
            _ => Err(serde::de::Error::unknown_variant(
                &s,
                &["Container", "Callable", "Data"],
            )),
        }
    }
}

// ============================================================================
// Kind Enums
// ============================================================================

/// Kinds of Container nodes - structural organization entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ContainerKind {
    /// Workspace root container (for multi-repo workspaces)
    Workspace,
    /// Repository root container (with git metadata)
    Repository,
    /// Source file container
    File,
    /// Namespace, package, or module
    Namespace,
    /// Module or compilation unit
    Module,
    /// Package declaration
    Package,
    /// Type definition (class, struct, interface, enum, etc.)
    Type,
    /// Component (npm package, Cargo crate, Go module, C# project, etc.)
    /// Represents a logical package with its own manifest file.
    Component,
}

impl ContainerKind {
    /// Get the string representation matching Python format
    pub fn as_str(&self) -> &'static str {
        match self {
            ContainerKind::Workspace => "workspace",
            ContainerKind::Repository => "repository",
            ContainerKind::File => "file",
            ContainerKind::Namespace => "namespace",
            ContainerKind::Module => "module",
            ContainerKind::Package => "package",
            ContainerKind::Type => "type",
            ContainerKind::Component => "component",
        }
    }
}

/// Kinds of Callable nodes - executable code entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CallableKind {
    /// Function or procedure
    Function,
    /// Class or instance method
    Method,
    /// Constructor or initializer
    Constructor,
    /// Macro (Rust, C/C++)
    Macro,
}

impl CallableKind {
    /// Get the string representation matching Python format
    pub fn as_str(&self) -> &'static str {
        match self {
            CallableKind::Function => "function",
            CallableKind::Method => "method",
            CallableKind::Constructor => "constructor",
            CallableKind::Macro => "macro",
        }
    }
}

/// Kinds of Data nodes - state and value entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DataKind {
    /// Constant or const value
    Constant,
    /// Variable or value binding
    Value,
    /// Class or struct field
    Field,
    /// Property (C#, Python @property, etc.)
    Property,
    /// Function/method parameter
    Parameter,
    /// Local variable within a callable
    Local,
}

impl DataKind {
    /// Get the string representation matching Python format
    pub fn as_str(&self) -> &'static str {
        match self {
            DataKind::Constant => "constant",
            DataKind::Value => "value",
            DataKind::Field => "field",
            DataKind::Property => "property",
            DataKind::Parameter => "parameter",
            DataKind::Local => "local",
        }
    }
}

/// Unified kind enum that can represent any node kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum NodeKind {
    Container(ContainerKind),
    Callable(CallableKind),
    Data(DataKind),
}

impl NodeKind {
    /// Get the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            NodeKind::Container(k) => k.as_str(),
            NodeKind::Callable(k) => k.as_str(),
            NodeKind::Data(k) => k.as_str(),
        }
    }

    /// Get the parent node type for this kind
    pub fn node_type(&self) -> NodeType {
        match self {
            NodeKind::Container(_) => NodeType::Container,
            NodeKind::Callable(_) => NodeType::Callable,
            NodeKind::Data(_) => NodeType::Data,
        }
    }
}

// ============================================================================
// Node Metadata
// ============================================================================

/// Optional metadata for code entities.
///
/// All fields are optional to support graceful degradation across languages.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeMetadata {
    /// Visibility: "public", "private", "protected", "internal"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visibility: Option<String>,

    /// Async callable (function/method/constructor) - execution modifier
    #[serde(rename = "async", skip_serializing_if = "Option::is_none")]
    pub is_async: Option<bool>,

    /// Static member
    #[serde(rename = "static", skip_serializing_if = "Option::is_none")]
    pub is_static: Option<bool>,

    /// Abstract class/method
    #[serde(rename = "abstract", skip_serializing_if = "Option::is_none")]
    pub is_abstract: Option<bool>,

    /// Virtual method
    #[serde(rename = "virtual", skip_serializing_if = "Option::is_none")]
    pub is_virtual: Option<bool>,

    /// Python decorators or C# attributes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decorators: Option<Vec<String>>,

    /// Other language-specific modifiers (e.g., final, sealed, inline)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub modifiers: Option<Vec<String>>,

    /// Semantic scope from overlay tags (e.g., "test", "benchmark", "example")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,

    // --- Git metadata (for Repository containers) ---
    /// Git remote URL (e.g., "https://github.com/org/repo.git")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_remote: Option<String>,

    /// Git branch name (e.g., "main", "feature/xyz")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_branch: Option<String>,

    /// Git commit SHA (e.g., "abc123def456...")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,

    // --- Component metadata (for Component containers) ---
    /// Whether this component is a workspace root (defines workspace members)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_workspace_root: Option<bool>,

    /// Whether this component is publishable to a registry
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_publishable: Option<bool>,

    /// Path to the manifest file relative to repo root (for quick lookup)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub manifest_path: Option<String>,
}

impl NodeMetadata {
    /// Create empty metadata
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if metadata has any values set
    pub fn is_empty(&self) -> bool {
        self.visibility.is_none()
            && self.is_async.is_none()
            && self.is_static.is_none()
            && self.is_abstract.is_none()
            && self.is_virtual.is_none()
            && self.decorators.is_none()
            && self.modifiers.is_none()
            && self.scope.is_none()
            && self.git_remote.is_none()
            && self.git_branch.is_none()
            && self.git_commit.is_none()
            && self.is_workspace_root.is_none()
            && self.is_publishable.is_none()
            && self.manifest_path.is_none()
    }

    /// Create git metadata for a repository container
    pub fn with_git(
        mut self,
        remote: Option<String>,
        branch: Option<String>,
        commit: Option<String>,
    ) -> Self {
        self.git_remote = remote;
        self.git_branch = branch;
        self.git_commit = commit;
        self
    }

    /// Create component metadata for a component container
    ///
    /// # Arguments
    /// * `is_workspace_root` - Whether this component defines workspace members
    /// * `is_publishable` - Whether this component is publishable to a registry
    /// * `manifest_path` - Path to the manifest file relative to repo root
    pub fn with_component(
        mut self,
        is_workspace_root: Option<bool>,
        is_publishable: Option<bool>,
        manifest_path: Option<String>,
    ) -> Self {
        self.is_workspace_root = is_workspace_root;
        self.is_publishable = is_publishable;
        self.manifest_path = manifest_path;
        self
    }
}

// ============================================================================
// Node
// ============================================================================

/// A node in the code graph representing a code entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Node {
    /// Hierarchical node ID (e.g., "file.py:Module:Class:Method")
    pub id: String,

    /// Entity name
    pub name: String,

    /// Node type: FILE, Container, Callable, or Data
    #[serde(rename = "type")]
    pub node_type: NodeType,

    /// Kind within the node type (e.g., "function", "class", "field")
    /// None for FILE nodes
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,

    /// Language-specific subtype (e.g., "struct", "interface", "class" for Container/type)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtype: Option<String>,

    /// Source file path
    pub file: String,

    /// Starting line number (1-indexed)
    pub line: usize,

    /// Ending line number (1-indexed)
    pub end_line: usize,

    /// Source code text
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,

    /// Semantic metadata
    #[serde(default, skip_serializing_if = "NodeMetadata::is_empty")]
    pub metadata: NodeMetadata,

    /// File content hash (only for FILE nodes)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash: Option<String>,
}

impl Node {
    /// Create a new Workspace container node (root for multi-repo workspaces)
    pub fn workspace(name: String) -> Self {
        Self {
            id: name.clone(),
            name: name.clone(),
            node_type: NodeType::Container,
            kind: Some(ContainerKind::Workspace.as_str().to_string()),
            subtype: None,
            file: String::new(), // Workspace has no file
            line: 0,
            end_line: 0,
            text: None,
            metadata: NodeMetadata::default(),
            hash: None,
        }
    }

    /// Create a new Repository container node (root of the graph hierarchy)
    pub fn repository(name: String, metadata: NodeMetadata) -> Self {
        Self {
            id: name.clone(),
            name: name.clone(),
            node_type: NodeType::Container,
            kind: Some(ContainerKind::Repository.as_str().to_string()),
            subtype: None,
            file: String::new(), // Repository has no file
            line: 0,
            end_line: 0,
            text: None,
            metadata,
            hash: None,
        }
    }

    /// Create a new Component container node (npm package, Cargo crate, Go module, etc.)
    ///
    /// # Arguments
    /// * `id` - Hierarchical node ID (e.g., "my-repo:packages/core")
    /// * `name` - Component name from manifest (e.g., "@myorg/core")
    /// * `manifest_path` - Path to the manifest file relative to repo root
    /// * `metadata` - Component metadata (is_workspace_root, is_publishable)
    pub fn component(
        id: String,
        name: String,
        manifest_path: String,
        metadata: NodeMetadata,
    ) -> Self {
        Self {
            id,
            name,
            node_type: NodeType::Container,
            kind: Some(ContainerKind::Component.as_str().to_string()),
            subtype: None,
            file: manifest_path, // The manifest file path
            line: 1,
            end_line: 1,
            text: None,
            metadata,
            hash: None,
        }
    }

    /// Create a new source file Container node (replaces legacy FILE node type)
    pub fn source_file(id: String, file_path: String, hash: String, line_count: usize) -> Self {
        Self {
            id,
            name: file_path.clone(),
            node_type: NodeType::Container,
            kind: Some(ContainerKind::File.as_str().to_string()),
            subtype: None,
            file: file_path,
            line: 1,
            end_line: line_count.max(1),
            text: None,
            metadata: NodeMetadata::default(),
            hash: Some(hash),
        }
    }

    /// Create a new Container node
    pub fn container(
        id: String,
        name: String,
        kind: ContainerKind,
        subtype: Option<String>,
        file: String,
        line: usize,
        end_line: usize,
    ) -> Self {
        Self {
            id,
            name,
            node_type: NodeType::Container,
            kind: Some(kind.as_str().to_string()),
            subtype,
            file,
            line,
            end_line,
            text: None,
            metadata: NodeMetadata::default(),
            hash: None,
        }
    }

    /// Create a new Callable node
    pub fn callable(
        id: String,
        name: String,
        kind: CallableKind,
        file: String,
        line: usize,
        end_line: usize,
    ) -> Self {
        Self {
            id,
            name,
            node_type: NodeType::Callable,
            kind: Some(kind.as_str().to_string()),
            subtype: None,
            file,
            line,
            end_line,
            text: None,
            metadata: NodeMetadata::default(),
            hash: None,
        }
    }

    /// Create a new Data node
    pub fn data(
        id: String,
        name: String,
        kind: DataKind,
        subtype: Option<String>,
        file: String,
        line: usize,
        end_line: usize,
    ) -> Self {
        Self {
            id,
            name,
            node_type: NodeType::Data,
            kind: Some(kind.as_str().to_string()),
            subtype,
            file,
            line,
            end_line,
            text: None,
            metadata: NodeMetadata::default(),
            hash: None,
        }
    }

    /// Set the source text
    pub fn with_text(mut self, text: String) -> Self {
        self.text = Some(text);
        self
    }

    /// Set the metadata
    pub fn with_metadata(mut self, metadata: NodeMetadata) -> Self {
        self.metadata = metadata;
        self
    }

    /// Check if this is a file node (Container with kind="file")
    pub fn is_file(&self) -> bool {
        self.node_type == NodeType::Container && self.kind.as_deref() == Some("file")
    }

    /// Check if this is a repository container node
    pub fn is_repository(&self) -> bool {
        self.node_type == NodeType::Container && self.kind.as_deref() == Some("repository")
    }

    /// Check if this is a workspace container node
    pub fn is_workspace(&self) -> bool {
        self.node_type == NodeType::Container && self.kind.as_deref() == Some("workspace")
    }

    /// Check if this is a component container node
    pub fn is_component(&self) -> bool {
        self.node_type == NodeType::Container && self.kind.as_deref() == Some("component")
    }

    /// Check if this is a Container node (any kind)
    pub fn is_container(&self) -> bool {
        self.node_type == NodeType::Container
    }

    /// Check if this is a Callable node
    pub fn is_callable(&self) -> bool {
        self.node_type == NodeType::Callable
    }

    /// Check if this is a Data node
    pub fn is_data(&self) -> bool {
        self.node_type == NodeType::Data
    }

    /// Get the container kind if this is a Container node
    pub fn container_kind(&self) -> Option<ContainerKind> {
        if self.node_type == NodeType::Container {
            self.kind.as_deref().and_then(parse_container_kind)
        } else {
            None
        }
    }
}

// ============================================================================
// Edge
// ============================================================================

/// An edge in the code graph representing a relationship between nodes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Edge {
    /// Source node ID
    pub source: String,

    /// Target node ID
    pub target: String,

    /// Relationship type
    #[serde(rename = "type")]
    pub edge_type: EdgeType,

    /// Line number where the reference occurs (for USES edges)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ref_line: Option<usize>,

    /// The identifier text at the reference site (for USES/DEPENDS_ON edges)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ident: Option<String>,

    // --- DependsOn edge metadata (for Component dependencies) ---
    /// Version specification (e.g., "workspace:*", "^1.0.0", "path:../core")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version_spec: Option<String>,

    /// Whether this is a development dependency (devDependencies, dev-dependencies, etc.)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_dev_dependency: Option<bool>,
}

impl Edge {
    /// Create a CONTAINS edge (parent contains child)
    pub fn contains(parent: String, child: String) -> Self {
        Self {
            source: parent,
            target: child,
            edge_type: EdgeType::Contains,
            ref_line: None,
            ident: None,
            version_spec: None,
            is_dev_dependency: None,
        }
    }

    /// Create a USES edge (source uses/references target)
    pub fn uses(
        source: String,
        target: String,
        ref_line: Option<usize>,
        ident: Option<String>,
    ) -> Self {
        Self {
            source,
            target,
            edge_type: EdgeType::Uses,
            ref_line,
            ident,
            version_spec: None,
            is_dev_dependency: None,
        }
    }

    /// Create a DEFINES edge (container defines member)
    pub fn defines(container: String, member: String) -> Self {
        Self {
            source: container,
            target: member,
            edge_type: EdgeType::Defines,
            ref_line: None,
            ident: None,
            version_spec: None,
            is_dev_dependency: None,
        }
    }

    /// Create a DEPENDS_ON edge (component depends on another component)
    ///
    /// # Arguments
    /// * `source` - The dependent component node ID
    /// * `target` - The dependency component node ID
    /// * `ident` - The dependency name as specified in manifest (e.g., "@myorg/core")
    /// * `version_spec` - Version specification (e.g., "workspace:*", "^1.0.0", "path:../core")
    /// * `is_dev` - Whether this is a development dependency
    pub fn depends_on(
        source: String,
        target: String,
        ident: Option<String>,
        version_spec: Option<String>,
        is_dev: Option<bool>,
    ) -> Self {
        Self {
            source,
            target,
            edge_type: EdgeType::DependsOn,
            ref_line: None,
            ident,
            version_spec,
            is_dev_dependency: is_dev,
        }
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Validate that a kind value is valid for the given node type.
pub fn validate_node_kind(node_type: NodeType, kind: &str) -> bool {
    match node_type {
        NodeType::Container => matches!(
            kind,
            "workspace"
                | "repository"
                | "file"
                | "namespace"
                | "module"
                | "package"
                | "type"
                | "component"
        ),
        NodeType::Callable => matches!(kind, "function" | "method" | "constructor" | "macro"),
        NodeType::Data => matches!(
            kind,
            "constant" | "value" | "field" | "property" | "parameter" | "local"
        ),
    }
}

/// Determine the node type from a kind value.
pub fn get_node_type_from_kind(kind: &str) -> Option<NodeType> {
    match kind {
        "workspace" | "repository" | "file" | "namespace" | "module" | "package" | "type"
        | "component" => Some(NodeType::Container),
        "function" | "method" | "constructor" | "macro" => Some(NodeType::Callable),
        "constant" | "value" | "field" | "property" | "parameter" | "local" => Some(NodeType::Data),
        _ => None,
    }
}

/// Parse a kind string into the appropriate Kind enum
pub fn parse_container_kind(kind: &str) -> Option<ContainerKind> {
    match kind {
        "workspace" => Some(ContainerKind::Workspace),
        "repository" => Some(ContainerKind::Repository),
        "file" => Some(ContainerKind::File),
        "namespace" => Some(ContainerKind::Namespace),
        "module" => Some(ContainerKind::Module),
        "package" => Some(ContainerKind::Package),
        "type" => Some(ContainerKind::Type),
        "component" => Some(ContainerKind::Component),
        _ => None,
    }
}

/// Parse a kind string into CallableKind
pub fn parse_callable_kind(kind: &str) -> Option<CallableKind> {
    match kind {
        "function" => Some(CallableKind::Function),
        "method" => Some(CallableKind::Method),
        "constructor" => Some(CallableKind::Constructor),
        "macro" => Some(CallableKind::Macro),
        _ => None,
    }
}

/// Parse a kind string into DataKind
pub fn parse_data_kind(kind: &str) -> Option<DataKind> {
    match kind {
        "constant" => Some(DataKind::Constant),
        "value" => Some(DataKind::Value),
        "field" => Some(DataKind::Field),
        "property" => Some(DataKind::Property),
        "parameter" => Some(DataKind::Parameter),
        "local" => Some(DataKind::Local),
        _ => None,
    }
}

// ============================================================================
// PetGraph-Based Code Graph (for efficient traversal and algorithms)
// ============================================================================

/// Edge data stored as edge weights in petgraph.
///
/// This struct carries the relationship information for edges in the graph,
/// enabling efficient traversal while preserving edge semantics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EdgeData {
    /// Relationship type (CONTAINS, USES, DEFINES, DEPENDS_ON)
    pub edge_type: EdgeType,
    /// Line number where the reference occurs (for USES edges)
    pub ref_line: Option<usize>,
    /// The identifier text at the reference site (for USES/DEPENDS_ON edges)
    pub ident: Option<String>,
    /// Version specification (for DEPENDS_ON edges)
    pub version_spec: Option<String>,
    /// Whether this is a development dependency (for DEPENDS_ON edges)
    pub is_dev_dependency: Option<bool>,
}

impl EdgeData {
    /// Create a CONTAINS edge data
    pub fn contains() -> Self {
        Self {
            edge_type: EdgeType::Contains,
            ref_line: None,
            ident: None,
            version_spec: None,
            is_dev_dependency: None,
        }
    }

    /// Create a USES edge data
    pub fn uses(ref_line: Option<usize>, ident: Option<String>) -> Self {
        Self {
            edge_type: EdgeType::Uses,
            ref_line,
            ident,
            version_spec: None,
            is_dev_dependency: None,
        }
    }

    /// Create a DEFINES edge data
    pub fn defines() -> Self {
        Self {
            edge_type: EdgeType::Defines,
            ref_line: None,
            ident: None,
            version_spec: None,
            is_dev_dependency: None,
        }
    }

    /// Create a DEPENDS_ON edge data (component dependency)
    ///
    /// # Arguments
    /// * `ident` - The dependency name as specified in manifest
    /// * `version_spec` - Version specification (e.g., "workspace:*", "^1.0.0")
    /// * `is_dev` - Whether this is a development dependency
    pub fn depends_on(
        ident: Option<String>,
        version_spec: Option<String>,
        is_dev: Option<bool>,
    ) -> Self {
        Self {
            edge_type: EdgeType::DependsOn,
            ref_line: None,
            ident,
            version_spec,
            is_dev_dependency: is_dev,
        }
    }
}

impl From<&Edge> for EdgeData {
    fn from(edge: &Edge) -> Self {
        Self {
            edge_type: edge.edge_type,
            ref_line: edge.ref_line,
            ident: edge.ident.clone(),
            version_spec: edge.version_spec.clone(),
            is_dev_dependency: edge.is_dev_dependency,
        }
    }
}

/// A petgraph-based code graph for efficient traversal and graph algorithms.
///
/// This implementation uses `petgraph::StableGraph` which:
/// - Supports O(1) neighbor access via adjacency lists
/// - Provides stable indices (node/edge removal doesn't invalidate others)
/// - Enables built-in graph algorithms (BFS, DFS, topological sort, etc.)
///
/// Use this for runtime operations that require graph traversal.
#[derive(Debug, Clone)]
pub struct PetCodeGraph {
    /// The underlying petgraph instance
    graph: StableGraph<Node, EdgeData, petgraph::Directed>,

    /// Map from node ID (string) to petgraph NodeIndex for O(1) lookup
    node_index_map: HashMap<String, NodeIndex>,

    /// Schema version for compatibility
    schema_version: String,
}

impl Default for PetCodeGraph {
    fn default() -> Self {
        Self::new()
    }
}

impl PetCodeGraph {
    /// Create a new empty petgraph-based code graph
    pub fn new() -> Self {
        Self {
            graph: StableGraph::new(),
            node_index_map: HashMap::new(),
            schema_version: GRAPH_SCHEMA_VERSION.to_string(),
        }
    }

    /// Get the schema version
    pub fn schema_version(&self) -> &str {
        &self.schema_version
    }

    // ------------------------------------------------------------------------
    // Node Operations
    // ------------------------------------------------------------------------

    /// Add a node to the graph, returning its NodeIndex.
    ///
    /// If a node with the same ID already exists, it will be replaced.
    pub fn add_node(&mut self, node: Node) -> NodeIndex {
        let node_id = node.id.clone();

        // Remove existing node if present (replace semantics)
        if let Some(&existing_idx) = self.node_index_map.get(&node_id) {
            self.graph.remove_node(existing_idx);
        }

        let idx = self.graph.add_node(node);
        self.node_index_map.insert(node_id, idx);
        idx
    }

    /// Get a node by its string ID
    pub fn get_node(&self, id: &str) -> Option<&Node> {
        self.node_index_map
            .get(id)
            .and_then(|&idx| self.graph.node_weight(idx))
    }

    /// Get a mutable node by its string ID
    pub fn get_node_mut(&mut self, id: &str) -> Option<&mut Node> {
        self.node_index_map
            .get(id)
            .copied()
            .and_then(|idx| self.graph.node_weight_mut(idx))
    }

    /// Get a node by its NodeIndex
    pub fn get_node_by_index(&self, idx: NodeIndex) -> Option<&Node> {
        self.graph.node_weight(idx)
    }

    /// Get the NodeIndex for a node ID
    pub fn get_node_index(&self, id: &str) -> Option<NodeIndex> {
        self.node_index_map.get(id).copied()
    }

    /// Check if the graph contains a node with the given ID
    pub fn contains_node(&self, id: &str) -> bool {
        self.node_index_map.contains_key(id)
    }

    /// Remove a node and all its incident edges
    pub fn remove_node(&mut self, id: &str) -> Option<Node> {
        if let Some(idx) = self.node_index_map.remove(id) {
            self.graph.remove_node(idx)
        } else {
            None
        }
    }

    /// Get the number of nodes
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Iterate over all nodes
    pub fn iter_nodes(&self) -> impl Iterator<Item = &Node> {
        self.graph.node_weights()
    }

    /// Get nodes by type
    pub fn nodes_by_type(&self, node_type: NodeType) -> impl Iterator<Item = &Node> {
        self.graph
            .node_weights()
            .filter(move |n| n.node_type == node_type)
    }

    // ------------------------------------------------------------------------
    // Edge Operations
    // ------------------------------------------------------------------------

    /// Add an edge between two nodes by their string IDs.
    ///
    /// Returns `Some(EdgeIndex)` if both nodes exist, `None` otherwise.
    pub fn add_edge(
        &mut self,
        source_id: &str,
        target_id: &str,
        data: EdgeData,
    ) -> Option<EdgeIndex> {
        let source_idx = self.node_index_map.get(source_id)?;
        let target_idx = self.node_index_map.get(target_id)?;
        Some(self.graph.add_edge(*source_idx, *target_idx, data))
    }

    /// Add an edge using an Edge struct.
    ///
    /// Returns `Some(EdgeIndex)` if both nodes exist, `None` otherwise.
    pub fn add_edge_from_struct(&mut self, edge: &Edge) -> Option<EdgeIndex> {
        self.add_edge(
            &edge.source,
            &edge.target,
            EdgeData {
                edge_type: edge.edge_type,
                ref_line: edge.ref_line,
                ident: edge.ident.clone(),
                version_spec: edge.version_spec.clone(),
                is_dev_dependency: edge.is_dev_dependency,
            },
        )
    }

    /// Add an edge using NodeIndices directly
    pub fn add_edge_by_index(
        &mut self,
        source: NodeIndex,
        target: NodeIndex,
        data: EdgeData,
    ) -> EdgeIndex {
        self.graph.add_edge(source, target, data)
    }

    /// Get all incoming edges for a node (edges where this node is the target)
    pub fn incoming_edges(&self, id: &str) -> impl Iterator<Item = (&Node, &EdgeData)> {
        let idx = self.node_index_map.get(id).copied();
        self.graph
            .edges_directed(
                idx.unwrap_or(NodeIndex::new(usize::MAX)),
                Direction::Incoming,
            )
            .filter_map(move |edge_ref| {
                let source_node = self.graph.node_weight(edge_ref.source())?;
                Some((source_node, edge_ref.weight()))
            })
    }

    /// Get all outgoing edges from a node (edges where this node is the source)
    pub fn outgoing_edges(&self, id: &str) -> impl Iterator<Item = (&Node, &EdgeData)> {
        let idx = self.node_index_map.get(id).copied();
        self.graph
            .edges_directed(
                idx.unwrap_or(NodeIndex::new(usize::MAX)),
                Direction::Outgoing,
            )
            .filter_map(move |edge_ref| {
                let target_node = self.graph.node_weight(edge_ref.target())?;
                Some((target_node, edge_ref.weight()))
            })
    }

    /// Get the number of edges
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Iterate over all edges, returning Edge structs.
    ///
    /// Note: This creates Edge structs on-the-fly. For performance-critical code,
    /// consider using `edges_by_type()` or `outgoing_edges()` instead.
    pub fn iter_edges(&self) -> impl Iterator<Item = Edge> + '_ {
        self.graph.edge_references().filter_map(move |edge_ref| {
            let source = self.graph.node_weight(edge_ref.source())?;
            let target = self.graph.node_weight(edge_ref.target())?;
            let edge_data = edge_ref.weight();
            Some(Edge {
                source: source.id.clone(),
                target: target.id.clone(),
                edge_type: edge_data.edge_type,
                ref_line: edge_data.ref_line,
                ident: edge_data.ident.clone(),
                version_spec: edge_data.version_spec.clone(),
                is_dev_dependency: edge_data.is_dev_dependency,
            })
        })
    }

    /// Get edges by type
    pub fn edges_by_type(
        &self,
        edge_type: EdgeType,
    ) -> impl Iterator<Item = (&Node, &Node, &EdgeData)> {
        self.graph.edge_references().filter_map(move |edge_ref| {
            if edge_ref.weight().edge_type == edge_type {
                let source = self.graph.node_weight(edge_ref.source())?;
                let target = self.graph.node_weight(edge_ref.target())?;
                Some((source, target, edge_ref.weight()))
            } else {
                None
            }
        })
    }

    // ------------------------------------------------------------------------
    // Traversal Operations
    // ------------------------------------------------------------------------

    /// Get all neighbor nodes (both incoming and outgoing)
    pub fn neighbors(&self, id: &str) -> impl Iterator<Item = &Node> {
        let idx = self.node_index_map.get(id).copied();
        self.graph
            .neighbors_undirected(idx.unwrap_or(NodeIndex::new(usize::MAX)))
            .filter_map(|neighbor_idx| self.graph.node_weight(neighbor_idx))
    }

    /// Get children (outgoing CONTAINS edges)
    pub fn children(&self, id: &str) -> impl Iterator<Item = &Node> {
        self.outgoing_edges(id)
            .filter(|(_, edge_data)| edge_data.edge_type == EdgeType::Contains)
            .map(|(node, _)| node)
    }

    /// Get parent (incoming CONTAINS edge) - typically only one
    pub fn parent(&self, id: &str) -> Option<&Node> {
        self.incoming_edges(id)
            .find(|(_, edge_data)| edge_data.edge_type == EdgeType::Contains)
            .map(|(node, _)| node)
    }

    // ------------------------------------------------------------------------
    // File Operations
    // ------------------------------------------------------------------------

    /// Remove all nodes from a file and their incident edges
    pub fn remove_file_nodes(&mut self, file_path: &str) {
        // Collect node IDs to remove
        let ids_to_remove: Vec<String> = self
            .graph
            .node_weights()
            .filter(|n| n.file == file_path)
            .map(|n| n.id.clone())
            .collect();

        // Remove nodes (edges are automatically removed by petgraph)
        for id in ids_to_remove {
            self.remove_node(&id);
        }
    }

    // ------------------------------------------------------------------------
    // Low-level Access (for advanced use cases)
    // ------------------------------------------------------------------------

    /// Get a reference to the underlying petgraph
    pub fn inner(&self) -> &StableGraph<Node, EdgeData, petgraph::Directed> {
        &self.graph
    }

    /// Get a mutable reference to the underlying petgraph
    pub fn inner_mut(&mut self) -> &mut StableGraph<Node, EdgeData, petgraph::Directed> {
        &mut self.graph
    }

    /// Get a reference to the node index map
    pub fn node_index_map(&self) -> &HashMap<String, NodeIndex> {
        &self.node_index_map
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edge_type_serialization() {
        let edge_type = EdgeType::Contains;
        let json = serde_json::to_string(&edge_type).unwrap();
        assert_eq!(json, "\"CONTAINS\"");

        let edge_type = EdgeType::Uses;
        let json = serde_json::to_string(&edge_type).unwrap();
        assert_eq!(json, "\"USES\"");
    }

    #[test]
    fn test_node_type_serialization() {
        // Test serialization
        let node_type = NodeType::Container;
        let json = serde_json::to_string(&node_type).unwrap();
        assert_eq!(json, "\"Container\"");

        let node_type = NodeType::Callable;
        let json = serde_json::to_string(&node_type).unwrap();
        assert_eq!(json, "\"Callable\"");

        let node_type = NodeType::Data;
        let json = serde_json::to_string(&node_type).unwrap();
        assert_eq!(json, "\"Data\"");
    }

    #[test]
    fn test_node_type_legacy_file_deserialization() {
        // Test that legacy "FILE" deserializes to Container (for backward compatibility)
        let node_type: NodeType = serde_json::from_str("\"FILE\"").unwrap();
        assert_eq!(node_type, NodeType::Container);

        // Normal deserialization should still work
        let node_type: NodeType = serde_json::from_str("\"Container\"").unwrap();
        assert_eq!(node_type, NodeType::Container);
    }

    #[test]
    fn test_container_kind_serialization() {
        let kind = ContainerKind::Type;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"type\"");
    }

    #[test]
    fn test_node_metadata_empty() {
        let metadata = NodeMetadata::default();
        assert!(metadata.is_empty());

        let metadata = NodeMetadata {
            visibility: Some("public".to_string()),
            ..Default::default()
        };
        assert!(!metadata.is_empty());
    }

    #[test]
    fn test_node_creation() {
        let node = Node::callable(
            "test.py:my_func".to_string(),
            "my_func".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            10,
            20,
        );

        assert_eq!(node.id, "test.py:my_func");
        assert_eq!(node.name, "my_func");
        assert_eq!(node.node_type, NodeType::Callable);
        assert_eq!(node.kind, Some("function".to_string()));
        assert_eq!(node.file, "test.py");
        assert_eq!(node.line, 10);
        assert_eq!(node.end_line, 20);
    }

    #[test]
    fn test_file_node() {
        let node = Node::source_file(
            "test.py".to_string(),
            "test.py".to_string(),
            "abc123".to_string(),
            100,
        );

        assert!(node.is_file());
        assert_eq!(node.hash, Some("abc123".to_string()));
        // Files are now Containers with kind="file"
        assert_eq!(node.kind.as_deref(), Some("file"));
        assert_eq!(node.node_type, NodeType::Container);
    }

    #[test]
    fn test_edge_creation() {
        let edge = Edge::contains("parent".to_string(), "child".to_string());
        assert_eq!(edge.edge_type, EdgeType::Contains);
        assert!(edge.ref_line.is_none());

        let edge = Edge::uses(
            "caller".to_string(),
            "callee".to_string(),
            Some(42),
            Some("func_name".to_string()),
        );
        assert_eq!(edge.edge_type, EdgeType::Uses);
        assert_eq!(edge.ref_line, Some(42));
        assert_eq!(edge.ident, Some("func_name".to_string()));
    }

    #[test]
    fn test_validate_node_kind() {
        assert!(validate_node_kind(NodeType::Container, "type"));
        assert!(validate_node_kind(NodeType::Container, "namespace"));
        assert!(!validate_node_kind(NodeType::Container, "function"));

        assert!(validate_node_kind(NodeType::Callable, "function"));
        assert!(validate_node_kind(NodeType::Callable, "method"));
        assert!(!validate_node_kind(NodeType::Callable, "type"));

        assert!(validate_node_kind(NodeType::Data, "field"));
        assert!(validate_node_kind(NodeType::Data, "parameter"));
        assert!(!validate_node_kind(NodeType::Data, "function"));
    }

    #[test]
    fn test_get_node_type_from_kind() {
        assert_eq!(get_node_type_from_kind("type"), Some(NodeType::Container));
        assert_eq!(
            get_node_type_from_kind("function"),
            Some(NodeType::Callable)
        );
        assert_eq!(get_node_type_from_kind("field"), Some(NodeType::Data));
        assert_eq!(get_node_type_from_kind("unknown"), None);
    }

    #[test]
    fn test_nodes_by_type() {
        let mut graph = PetCodeGraph::new();

        // File is now a Container with kind="file"
        graph.add_node(Node::source_file(
            "a.py".to_string(),
            "a.py".to_string(),
            "x".to_string(),
            100,
        ));
        graph.add_node(Node::callable(
            "a.py:f".to_string(),
            "f".to_string(),
            CallableKind::Function,
            "a.py".to_string(),
            1,
            1,
        ));
        graph.add_node(Node::container(
            "a.py:C".to_string(),
            "C".to_string(),
            ContainerKind::Type,
            None,
            "a.py".to_string(),
            1,
            1,
        ));

        // Files are now Containers, so use is_file() to filter
        let files: Vec<_> = graph.iter_nodes().filter(|n| n.is_file()).collect();
        assert_eq!(files.len(), 1);

        let callables: Vec<_> = graph.nodes_by_type(NodeType::Callable).collect();
        assert_eq!(callables.len(), 1);

        // Containers now includes file + class = 2
        let containers: Vec<_> = graph.nodes_by_type(NodeType::Container).collect();
        assert_eq!(containers.len(), 2);
    }

    #[test]
    fn test_repository_node() {
        let metadata = NodeMetadata::default().with_git(
            Some("https://github.com/org/repo.git".to_string()),
            Some("main".to_string()),
            Some("abc123".to_string()),
        );
        let node = Node::repository("my-repo".to_string(), metadata);

        assert!(node.is_repository());
        assert!(node.is_container());
        assert!(!node.is_file());
        assert_eq!(node.id, "my-repo");
        assert_eq!(node.name, "my-repo");
        assert_eq!(node.kind, Some("repository".to_string()));
        assert_eq!(
            node.metadata.git_remote,
            Some("https://github.com/org/repo.git".to_string())
        );
        assert_eq!(node.metadata.git_branch, Some("main".to_string()));
        assert_eq!(node.metadata.git_commit, Some("abc123".to_string()));
        assert_eq!(node.container_kind(), Some(ContainerKind::Repository));
    }

    #[test]
    fn test_source_file_node() {
        let node = Node::source_file(
            "src/main.rs".to_string(),
            "src/main.rs".to_string(),
            "sha256:abc123".to_string(),
            100,
        );

        assert!(node.is_file());
        assert!(node.is_container());
        assert!(!node.is_repository());
        assert_eq!(node.node_type, NodeType::Container);
        assert_eq!(node.kind, Some("file".to_string()));
        assert_eq!(node.hash, Some("sha256:abc123".to_string()));
        assert_eq!(node.line, 1);
        assert_eq!(node.end_line, 100);
        assert_eq!(node.container_kind(), Some(ContainerKind::File));
    }

    #[test]
    fn test_is_file() {
        // File nodes are Container with kind="file"
        let file = Node::source_file(
            "test.py".to_string(),
            "test.py".to_string(),
            "abc".to_string(),
            50,
        );
        assert!(file.is_file());

        // Non-file containers should not be files
        let class = Node::container(
            "test.py:MyClass".to_string(),
            "MyClass".to_string(),
            ContainerKind::Type,
            Some("class".to_string()),
            "test.py".to_string(),
            1,
            10,
        );
        assert!(!class.is_file());
    }

    #[test]
    fn test_container_kind_parsing() {
        assert_eq!(
            parse_container_kind("repository"),
            Some(ContainerKind::Repository)
        );
        assert_eq!(parse_container_kind("file"), Some(ContainerKind::File));
        assert_eq!(
            parse_container_kind("namespace"),
            Some(ContainerKind::Namespace)
        );
        assert_eq!(parse_container_kind("module"), Some(ContainerKind::Module));
        assert_eq!(
            parse_container_kind("package"),
            Some(ContainerKind::Package)
        );
        assert_eq!(parse_container_kind("type"), Some(ContainerKind::Type));
        assert_eq!(parse_container_kind("invalid"), None);
    }

    #[test]
    fn test_validate_container_kinds() {
        assert!(validate_node_kind(NodeType::Container, "repository"));
        assert!(validate_node_kind(NodeType::Container, "file"));
        assert!(validate_node_kind(NodeType::Container, "type"));
        assert!(!validate_node_kind(NodeType::Container, "invalid"));
    }

    #[test]
    fn test_get_node_type_from_new_kinds() {
        assert_eq!(
            get_node_type_from_kind("repository"),
            Some(NodeType::Container)
        );
        assert_eq!(get_node_type_from_kind("file"), Some(NodeType::Container));
    }

    #[test]
    fn test_git_metadata() {
        let metadata = NodeMetadata::default().with_git(
            Some("origin".to_string()),
            Some("develop".to_string()),
            Some("deadbeef".to_string()),
        );

        assert!(!metadata.is_empty());
        assert_eq!(metadata.git_remote, Some("origin".to_string()));
        assert_eq!(metadata.git_branch, Some("develop".to_string()));
        assert_eq!(metadata.git_commit, Some("deadbeef".to_string()));
    }

    // ========================================================================
    // PetCodeGraph Tests
    // ========================================================================

    #[test]
    fn test_pet_code_graph_new() {
        let graph = PetCodeGraph::new();
        assert_eq!(graph.node_count(), 0);
        assert_eq!(graph.edge_count(), 0);
        assert_eq!(graph.schema_version(), GRAPH_SCHEMA_VERSION);
    }

    #[test]
    fn test_pet_code_graph_add_node() {
        let mut graph = PetCodeGraph::new();

        let node = Node::callable(
            "test.py:my_func".to_string(),
            "my_func".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            1,
            10,
        );

        let idx = graph.add_node(node);
        assert_eq!(graph.node_count(), 1);
        assert!(graph.contains_node("test.py:my_func"));

        let retrieved = graph.get_node("test.py:my_func").unwrap();
        assert_eq!(retrieved.name, "my_func");

        // Check index lookup
        let by_index = graph.get_node_by_index(idx).unwrap();
        assert_eq!(by_index.id, "test.py:my_func");
    }

    #[test]
    fn test_pet_code_graph_remove_node() {
        let mut graph = PetCodeGraph::new();

        graph.add_node(Node::callable(
            "test.py:func1".to_string(),
            "func1".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            1,
            5,
        ));
        graph.add_node(Node::callable(
            "test.py:func2".to_string(),
            "func2".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            6,
            10,
        ));

        assert_eq!(graph.node_count(), 2);

        let removed = graph.remove_node("test.py:func1");
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().name, "func1");
        assert_eq!(graph.node_count(), 1);
        assert!(!graph.contains_node("test.py:func1"));
        assert!(graph.contains_node("test.py:func2"));
    }

    #[test]
    fn test_pet_code_graph_add_edge() {
        let mut graph = PetCodeGraph::new();

        graph.add_node(Node::source_file(
            "test.py".to_string(),
            "test.py".to_string(),
            "abc".to_string(),
            100,
        ));
        graph.add_node(Node::callable(
            "test.py:func".to_string(),
            "func".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            1,
            10,
        ));

        let edge_idx = graph.add_edge("test.py", "test.py:func", EdgeData::contains());
        assert!(edge_idx.is_some());
        assert_eq!(graph.edge_count(), 1);

        // Adding edge with non-existent nodes returns None
        let invalid_edge = graph.add_edge("nonexistent", "test.py:func", EdgeData::contains());
        assert!(invalid_edge.is_none());
    }

    #[test]
    fn test_pet_code_graph_incoming_outgoing_edges() {
        let mut graph = PetCodeGraph::new();

        graph.add_node(Node::source_file(
            "test.py".to_string(),
            "test.py".to_string(),
            "abc".to_string(),
            100,
        ));
        graph.add_node(Node::callable(
            "test.py:func1".to_string(),
            "func1".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            1,
            5,
        ));
        graph.add_node(Node::callable(
            "test.py:func2".to_string(),
            "func2".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            6,
            10,
        ));

        graph.add_edge("test.py", "test.py:func1", EdgeData::contains());
        graph.add_edge("test.py", "test.py:func2", EdgeData::contains());
        graph.add_edge(
            "test.py:func1",
            "test.py:func2",
            EdgeData::uses(Some(3), Some("func2".to_string())),
        );

        // Check outgoing edges from test.py
        let outgoing: Vec<_> = graph.outgoing_edges("test.py").collect();
        assert_eq!(outgoing.len(), 2);

        // Check incoming edges to func2
        let incoming: Vec<_> = graph.incoming_edges("test.py:func2").collect();
        assert_eq!(incoming.len(), 2); // from test.py (CONTAINS) and func1 (USES)

        // Verify USES edge has metadata
        let uses_edge = incoming
            .iter()
            .find(|(_, e)| e.edge_type == EdgeType::Uses)
            .unwrap();
        assert_eq!(uses_edge.1.ref_line, Some(3));
        assert_eq!(uses_edge.1.ident, Some("func2".to_string()));
    }

    #[test]
    fn test_pet_code_graph_children_parent() {
        let mut graph = PetCodeGraph::new();

        graph.add_node(Node::container(
            "test.py:MyClass".to_string(),
            "MyClass".to_string(),
            ContainerKind::Type,
            Some("class".to_string()),
            "test.py".to_string(),
            1,
            50,
        ));
        graph.add_node(Node::callable(
            "test.py:MyClass:method1".to_string(),
            "method1".to_string(),
            CallableKind::Method,
            "test.py".to_string(),
            2,
            10,
        ));
        graph.add_node(Node::callable(
            "test.py:MyClass:method2".to_string(),
            "method2".to_string(),
            CallableKind::Method,
            "test.py".to_string(),
            11,
            20,
        ));

        graph.add_edge(
            "test.py:MyClass",
            "test.py:MyClass:method1",
            EdgeData::contains(),
        );
        graph.add_edge(
            "test.py:MyClass",
            "test.py:MyClass:method2",
            EdgeData::contains(),
        );

        // Check children
        let children: Vec<_> = graph.children("test.py:MyClass").collect();
        assert_eq!(children.len(), 2);

        // Check parent
        let parent = graph.parent("test.py:MyClass:method1").unwrap();
        assert_eq!(parent.id, "test.py:MyClass");
    }

    #[test]
    fn test_pet_code_graph_neighbors() {
        let mut graph = PetCodeGraph::new();

        graph.add_node(Node::callable(
            "a".to_string(),
            "a".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            1,
            5,
        ));
        graph.add_node(Node::callable(
            "b".to_string(),
            "b".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            6,
            10,
        ));
        graph.add_node(Node::callable(
            "c".to_string(),
            "c".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            11,
            15,
        ));

        graph.add_edge("a", "b", EdgeData::uses(None, None));
        graph.add_edge("c", "a", EdgeData::uses(None, None));

        // a's neighbors are b (outgoing) and c (incoming)
        let neighbors: Vec<_> = graph.neighbors("a").collect();
        assert_eq!(neighbors.len(), 2);
    }

    #[test]
    fn test_pet_code_graph_remove_file_nodes() {
        let mut graph = PetCodeGraph::new();

        graph.add_node(Node::source_file(
            "test.py".to_string(),
            "test.py".to_string(),
            "abc".to_string(),
            100,
        ));
        graph.add_node(Node::callable(
            "test.py:func1".to_string(),
            "func1".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            1,
            5,
        ));
        graph.add_node(Node::callable(
            "other.py:func".to_string(),
            "func".to_string(),
            CallableKind::Function,
            "other.py".to_string(),
            1,
            5,
        ));

        graph.add_edge("test.py", "test.py:func1", EdgeData::contains());
        graph.add_edge(
            "test.py:func1",
            "other.py:func",
            EdgeData::uses(Some(3), None),
        );

        assert_eq!(graph.node_count(), 3);
        assert_eq!(graph.edge_count(), 2);

        graph.remove_file_nodes("test.py");

        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.edge_count(), 0); // petgraph removes edges automatically
        assert!(!graph.contains_node("test.py"));
        assert!(!graph.contains_node("test.py:func1"));
        assert!(graph.contains_node("other.py:func"));
    }

    #[test]
    fn test_pet_code_graph_edges_by_type() {
        let mut graph = PetCodeGraph::new();

        graph.add_node(Node::container(
            "class".to_string(),
            "MyClass".to_string(),
            ContainerKind::Type,
            None,
            "test.py".to_string(),
            1,
            50,
        ));
        graph.add_node(Node::callable(
            "method".to_string(),
            "method".to_string(),
            CallableKind::Method,
            "test.py".to_string(),
            2,
            10,
        ));
        graph.add_node(Node::data(
            "field".to_string(),
            "my_field".to_string(),
            DataKind::Field,
            None,
            "test.py".to_string(),
            3,
            3,
        ));

        graph.add_edge("class", "method", EdgeData::contains());
        graph.add_edge("class", "field", EdgeData::defines());
        graph.add_edge(
            "method",
            "field",
            EdgeData::uses(Some(5), Some("my_field".to_string())),
        );

        let contains_edges: Vec<_> = graph.edges_by_type(EdgeType::Contains).collect();
        assert_eq!(contains_edges.len(), 1);

        let uses_edges: Vec<_> = graph.edges_by_type(EdgeType::Uses).collect();
        assert_eq!(uses_edges.len(), 1);

        let defines_edges: Vec<_> = graph.edges_by_type(EdgeType::Defines).collect();
        assert_eq!(defines_edges.len(), 1);
    }

    #[test]
    fn test_pet_code_graph_node_replace_semantics() {
        let mut graph = PetCodeGraph::new();

        // Add initial node
        graph.add_node(Node::callable(
            "test.py:func".to_string(),
            "func".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            1,
            5,
        ));

        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.get_node("test.py:func").unwrap().end_line, 5);

        // Replace with updated node (same ID)
        graph.add_node(Node::callable(
            "test.py:func".to_string(),
            "func".to_string(),
            CallableKind::Function,
            "test.py".to_string(),
            1,
            10, // Different end_line
        ));

        // Should still have 1 node, but with updated content
        assert_eq!(graph.node_count(), 1);
        assert_eq!(graph.get_node("test.py:func").unwrap().end_line, 10);
    }

    #[test]
    fn test_edge_data_constructors() {
        let contains = EdgeData::contains();
        assert_eq!(contains.edge_type, EdgeType::Contains);
        assert!(contains.ref_line.is_none());
        assert!(contains.ident.is_none());

        let uses = EdgeData::uses(Some(42), Some("foo".to_string()));
        assert_eq!(uses.edge_type, EdgeType::Uses);
        assert_eq!(uses.ref_line, Some(42));
        assert_eq!(uses.ident, Some("foo".to_string()));

        let defines = EdgeData::defines();
        assert_eq!(defines.edge_type, EdgeType::Defines);
        assert!(defines.ref_line.is_none());
        assert!(defines.ident.is_none());
    }

    #[test]
    fn test_edge_data_from_edge() {
        let edge = Edge::uses(
            "source".to_string(),
            "target".to_string(),
            Some(10),
            Some("call".to_string()),
        );

        let edge_data = EdgeData::from(&edge);
        assert_eq!(edge_data.edge_type, EdgeType::Uses);
        assert_eq!(edge_data.ref_line, Some(10));
        assert_eq!(edge_data.ident, Some("call".to_string()));
    }

    // ========================================================================
    // Phase 1.3: Component & DependsOn Serialization Tests
    // ========================================================================

    #[test]
    fn test_edge_type_depends_on_serialization() {
        // Test serialization
        let edge_type = EdgeType::DependsOn;
        let json = serde_json::to_string(&edge_type).unwrap();
        assert_eq!(json, "\"DEPENDS_ON\"");

        // Test deserialization
        let parsed: EdgeType = serde_json::from_str("\"DEPENDS_ON\"").unwrap();
        assert_eq!(parsed, EdgeType::DependsOn);
    }

    #[test]
    fn test_container_kind_component_serialization() {
        // Test serialization
        let kind = ContainerKind::Component;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"component\"");

        // Test deserialization
        let parsed: ContainerKind = serde_json::from_str("\"component\"").unwrap();
        assert_eq!(parsed, ContainerKind::Component);
    }

    #[test]
    fn test_depends_on_edge_serialization_round_trip() {
        let edge = Edge::depends_on(
            "pkg/frontend".to_string(),
            "pkg/core".to_string(),
            Some("@myorg/core".to_string()),
            Some("workspace:*".to_string()),
            Some(true),
        );

        // Serialize
        let json = serde_json::to_string_pretty(&edge).unwrap();

        // Verify JSON contains expected fields
        assert!(json.contains("\"DEPENDS_ON\""), "Should contain edge type");
        assert!(
            json.contains("\"version_spec\""),
            "Should contain version_spec"
        );
        assert!(
            json.contains("\"workspace:*\""),
            "Should contain version value"
        );
        assert!(
            json.contains("\"is_dev_dependency\""),
            "Should contain is_dev_dependency"
        );
        assert!(json.contains("true"), "Should contain dev dep value");

        // Deserialize and verify round-trip
        let parsed: Edge = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.source, "pkg/frontend");
        assert_eq!(parsed.target, "pkg/core");
        assert_eq!(parsed.edge_type, EdgeType::DependsOn);
        assert_eq!(parsed.ident, Some("@myorg/core".to_string()));
        assert_eq!(parsed.version_spec, Some("workspace:*".to_string()));
        assert_eq!(parsed.is_dev_dependency, Some(true));
    }

    #[test]
    fn test_depends_on_edge_minimal_serialization() {
        // DependsOn with no optional fields
        let edge = Edge::depends_on("pkg/a".to_string(), "pkg/b".to_string(), None, None, None);

        let json = serde_json::to_string(&edge).unwrap();

        // Optional fields should be skipped
        assert!(
            !json.contains("version_spec"),
            "Should skip None version_spec"
        );
        assert!(
            !json.contains("is_dev_dependency"),
            "Should skip None is_dev_dependency"
        );
        assert!(!json.contains("ident"), "Should skip None ident");

        // Round-trip
        let parsed: Edge = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.edge_type, EdgeType::DependsOn);
        assert_eq!(parsed.version_spec, None);
        assert_eq!(parsed.is_dev_dependency, None);
    }

    #[test]
    fn test_component_node_serialization_round_trip() {
        let metadata = NodeMetadata::default().with_component(
            Some(true), // is_workspace_root
            Some(true), // is_publishable
            Some("packages/core/package.json".to_string()),
        );

        let node = Node::component(
            "my-repo:packages/core".to_string(),
            "@myorg/core".to_string(),
            "packages/core/package.json".to_string(),
            metadata,
        );

        // Serialize
        let json = serde_json::to_string_pretty(&node).unwrap();

        // Verify JSON structure
        assert!(json.contains("\"component\""), "Should have component kind");
        assert!(
            json.contains("\"is_workspace_root\""),
            "Should have workspace root field"
        );
        assert!(
            json.contains("\"is_publishable\""),
            "Should have publishable field"
        );
        assert!(
            json.contains("\"manifest_path\""),
            "Should have manifest path"
        );

        // Round-trip
        let parsed: Node = serde_json::from_str(&json).unwrap();
        assert!(parsed.is_component());
        assert_eq!(parsed.id, "my-repo:packages/core");
        assert_eq!(parsed.name, "@myorg/core");
        assert_eq!(parsed.kind, Some("component".to_string()));
        assert_eq!(parsed.metadata.is_workspace_root, Some(true));
        assert_eq!(parsed.metadata.is_publishable, Some(true));
        assert_eq!(
            parsed.metadata.manifest_path,
            Some("packages/core/package.json".to_string())
        );
    }

    #[test]
    fn test_node_metadata_component_fields() {
        let metadata = NodeMetadata::default().with_component(
            Some(false), // not workspace root
            Some(true),  // publishable
            Some("Cargo.toml".to_string()),
        );

        assert!(!metadata.is_empty());
        assert_eq!(metadata.is_workspace_root, Some(false));
        assert_eq!(metadata.is_publishable, Some(true));
        assert_eq!(metadata.manifest_path, Some("Cargo.toml".to_string()));

        // Serialize and verify
        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains("\"is_workspace_root\":false"));
        assert!(json.contains("\"is_publishable\":true"));
        assert!(json.contains("\"manifest_path\":\"Cargo.toml\""));

        // Round-trip
        let parsed: NodeMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.is_workspace_root, Some(false));
        assert_eq!(parsed.is_publishable, Some(true));
        assert_eq!(parsed.manifest_path, Some("Cargo.toml".to_string()));
    }

    #[test]
    fn test_edge_data_depends_on_constructor() {
        let edge_data = EdgeData::depends_on(
            Some("my-dep".to_string()),
            Some("^1.0.0".to_string()),
            Some(false),
        );

        assert_eq!(edge_data.edge_type, EdgeType::DependsOn);
        assert_eq!(edge_data.ident, Some("my-dep".to_string()));
        assert_eq!(edge_data.version_spec, Some("^1.0.0".to_string()));
        assert_eq!(edge_data.is_dev_dependency, Some(false));
        assert!(edge_data.ref_line.is_none()); // DependsOn doesn't use ref_line
    }

    #[test]
    fn test_edge_data_from_depends_on_edge() {
        let edge = Edge::depends_on(
            "source".to_string(),
            "target".to_string(),
            Some("dep-name".to_string()),
            Some("path:../lib".to_string()),
            Some(true),
        );

        let edge_data = EdgeData::from(&edge);
        assert_eq!(edge_data.edge_type, EdgeType::DependsOn);
        assert_eq!(edge_data.ident, Some("dep-name".to_string()));
        assert_eq!(edge_data.version_spec, Some("path:../lib".to_string()));
        assert_eq!(edge_data.is_dev_dependency, Some(true));
    }

    #[test]
    fn test_validate_node_kind_component() {
        assert!(validate_node_kind(NodeType::Container, "component"));
        assert!(!validate_node_kind(NodeType::Callable, "component"));
        assert!(!validate_node_kind(NodeType::Data, "component"));
    }

    #[test]
    fn test_get_node_type_from_kind_component() {
        assert_eq!(
            get_node_type_from_kind("component"),
            Some(NodeType::Container)
        );
    }

    #[test]
    fn test_parse_container_kind_component() {
        assert_eq!(
            parse_container_kind("component"),
            Some(ContainerKind::Component)
        );
    }

    #[test]
    fn test_workspace_node() {
        let node = Node::workspace("my-workspace".to_string());

        assert!(node.is_workspace());
        assert!(node.is_container());
        assert!(!node.is_component());
        assert!(!node.is_file());
        assert_eq!(node.id, "my-workspace");
        assert_eq!(node.kind, Some("workspace".to_string()));
        assert_eq!(node.container_kind(), Some(ContainerKind::Workspace));
    }

    #[test]
    fn test_container_kind_workspace_serialization() {
        let kind = ContainerKind::Workspace;
        let json = serde_json::to_string(&kind).unwrap();
        assert_eq!(json, "\"workspace\"");

        let parsed: ContainerKind = serde_json::from_str("\"workspace\"").unwrap();
        assert_eq!(parsed, ContainerKind::Workspace);
    }
}
