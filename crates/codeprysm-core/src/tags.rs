//! Tag Parser for SCM Query Names
//!
//! This module parses declarative tag strings from tree-sitter SCM queries
//! into structured components for graph construction.
//!
//! Tag format: `@{def|ref}.{node_type}.{kind}[.{subtype}][.scope.{scope_value}]`
//!
//! Examples:
//! - `@definition.callable.function` → function definition
//! - `@definition.container.type.class` → class definition with subtype
//! - `@definition.callable.method.scope.test` → test method with scope
//! - `@reference.data.field` → field reference

use crate::graph::{CallableKind, ContainerKind, DataKind, NodeKind, NodeType};
use std::fmt;
use std::str::FromStr;
use thiserror::Error;

// ============================================================================
// Tag Category
// ============================================================================

/// Category of a tag - whether it defines or references an entity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TagCategory {
    /// Entity definition (creates a new node)
    Definition,
    /// Entity reference (creates a USES edge)
    Reference,
    /// Manifest extraction (captures component metadata and dependencies)
    /// Used by manifest SCM queries (package.json, Cargo.toml, etc.)
    Manifest,
}

impl TagCategory {
    /// Get the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            TagCategory::Definition => "definition",
            TagCategory::Reference => "reference",
            TagCategory::Manifest => "manifest",
        }
    }
}

impl FromStr for TagCategory {
    type Err = TagParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "definition" => Ok(TagCategory::Definition),
            "reference" => Ok(TagCategory::Reference),
            "manifest" => Ok(TagCategory::Manifest),
            _ => Err(TagParseError::UnknownCategory(s.to_string())),
        }
    }
}

impl fmt::Display for TagCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Tag Parse Result
// ============================================================================

/// Result of parsing a tag string.
///
/// Contains all semantic information extracted from a declarative tag.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagParseResult {
    /// Category: "definition" or "reference"
    pub category: TagCategory,
    /// High-level node type: FILE, Container, Callable, Data
    pub node_type: NodeType,
    /// Specific kind (function, method, class, field, etc.) - None for FILE
    pub kind: Option<NodeKind>,
    /// Language-specific subtype refinement (class, struct, interface, etc.)
    pub subtype: Option<String>,
    /// Semantic scope from overlay tags (test, benchmark, etc.)
    pub scope: Option<String>,
}

impl TagParseResult {
    /// Create a new TagParseResult for a file node (Container with kind="file")
    pub fn file(category: TagCategory) -> Self {
        Self {
            category,
            node_type: NodeType::Container,
            kind: Some(NodeKind::Container(ContainerKind::File)),
            subtype: None,
            scope: None,
        }
    }

    /// Create a new TagParseResult for a Container node
    pub fn container(
        category: TagCategory,
        kind: ContainerKind,
        subtype: Option<String>,
        scope: Option<String>,
    ) -> Self {
        Self {
            category,
            node_type: NodeType::Container,
            kind: Some(NodeKind::Container(kind)),
            subtype,
            scope,
        }
    }

    /// Create a new TagParseResult for a Callable node
    pub fn callable(
        category: TagCategory,
        kind: CallableKind,
        subtype: Option<String>,
        scope: Option<String>,
    ) -> Self {
        Self {
            category,
            node_type: NodeType::Callable,
            kind: Some(NodeKind::Callable(kind)),
            subtype,
            scope,
        }
    }

    /// Create a new TagParseResult for a Data node
    pub fn data(
        category: TagCategory,
        kind: DataKind,
        subtype: Option<String>,
        scope: Option<String>,
    ) -> Self {
        Self {
            category,
            node_type: NodeType::Data,
            kind: Some(NodeKind::Data(kind)),
            subtype,
            scope,
        }
    }

    /// Check if this is a definition tag
    pub fn is_definition(&self) -> bool {
        self.category == TagCategory::Definition
    }

    /// Check if this is a reference tag
    pub fn is_reference(&self) -> bool {
        self.category == TagCategory::Reference
    }

    /// Get the kind as a string, if present
    pub fn kind_str(&self) -> Option<&str> {
        self.kind.as_ref().map(|k| k.as_str())
    }
}

impl fmt::Display for TagParseResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "TagParseResult(category={:?}, node_type={:?}, kind={:?}, subtype={:?}, scope={:?})",
            self.category.as_str(),
            self.node_type.as_str(),
            self.kind_str(),
            self.subtype,
            self.scope
        )
    }
}

// ============================================================================
// Manifest Tag Parse Result
// ============================================================================

/// Type of manifest element being captured.
///
/// Used to distinguish between different types of data extracted from
/// manifest files (package.json, Cargo.toml, go.mod, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManifestElementType {
    /// Component name (package/crate/module name)
    /// Example: "name" field in package.json, [package].name in Cargo.toml
    ComponentName,

    /// Component version
    /// Example: "version" field in package.json, [package].version in Cargo.toml
    ComponentVersion,

    /// Dependency specification (with optional target name and version spec)
    /// Example: dependencies in package.json, [dependencies] in Cargo.toml
    Dependency,

    /// Workspace member path (relative path to a workspace package)
    /// Example: "workspaces" array in package.json, [workspace].members in Cargo.toml
    WorkspaceMember,

    /// Workspace root indicator (marks the root of a workspace/monorepo)
    /// Example: presence of "workspaces" in package.json, [workspace] in Cargo.toml
    WorkspaceRoot,
}

impl ManifestElementType {
    /// Get the string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            ManifestElementType::ComponentName => "component.name",
            ManifestElementType::ComponentVersion => "component.version",
            ManifestElementType::Dependency => "dependency",
            ManifestElementType::WorkspaceMember => "workspace.member",
            ManifestElementType::WorkspaceRoot => "workspace.root",
        }
    }
}

impl FromStr for ManifestElementType {
    type Err = TagParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "component.name" => Ok(ManifestElementType::ComponentName),
            "component.version" => Ok(ManifestElementType::ComponentVersion),
            "dependency" => Ok(ManifestElementType::Dependency),
            "workspace.member" => Ok(ManifestElementType::WorkspaceMember),
            "workspace.root" => Ok(ManifestElementType::WorkspaceRoot),
            _ => Err(TagParseError::UnknownManifestElement(s.to_string())),
        }
    }
}

impl fmt::Display for ManifestElementType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Result of parsing a manifest-specific tag string.
///
/// Contains semantic information for manifest extraction including
/// component metadata and dependency specifications.
///
/// Tag format: `@manifest.{element_type}[.scope.{scope_value}]`
///
/// Examples:
/// - `@manifest.component.name` → component name extraction
/// - `@manifest.dependency` → regular dependency
/// - `@manifest.dependency.scope.dev` → dev dependency
/// - `@manifest.workspace.member` → workspace member path
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestTagResult {
    /// Type of manifest element being captured
    pub element_type: ManifestElementType,
    /// Optional scope modifier (e.g., "dev" for dev dependencies, "build" for build deps)
    pub scope: Option<String>,
}

impl ManifestTagResult {
    /// Create a new ManifestTagResult
    pub fn new(element_type: ManifestElementType, scope: Option<String>) -> Self {
        Self {
            element_type,
            scope,
        }
    }

    /// Create a component name result
    pub fn component_name() -> Self {
        Self::new(ManifestElementType::ComponentName, None)
    }

    /// Create a component version result
    pub fn component_version() -> Self {
        Self::new(ManifestElementType::ComponentVersion, None)
    }

    /// Create a dependency result with optional scope
    pub fn dependency(scope: Option<String>) -> Self {
        Self::new(ManifestElementType::Dependency, scope)
    }

    /// Create a workspace member result
    pub fn workspace_member() -> Self {
        Self::new(ManifestElementType::WorkspaceMember, None)
    }

    /// Create a workspace root result
    pub fn workspace_root() -> Self {
        Self::new(ManifestElementType::WorkspaceRoot, None)
    }

    /// Check if this is a dev dependency
    pub fn is_dev_dependency(&self) -> bool {
        self.element_type == ManifestElementType::Dependency && self.scope.as_deref() == Some("dev")
    }

    /// Check if this is a build dependency
    pub fn is_build_dependency(&self) -> bool {
        self.element_type == ManifestElementType::Dependency
            && self.scope.as_deref() == Some("build")
    }
}

impl fmt::Display for ManifestTagResult {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ManifestTagResult(element_type={}, scope={:?})",
            self.element_type.as_str(),
            self.scope
        )
    }
}

// ============================================================================
// Tag Parse Errors
// ============================================================================

/// Errors that can occur during tag parsing.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum TagParseError {
    /// Tag string is too short (missing required parts)
    #[error("Invalid tag format: {0} (expected at least 'category.node_type')")]
    TooShort(String),

    /// Unknown category (not definition, reference, or manifest)
    #[error("Unknown tag category: {0} (expected 'definition', 'reference', or 'manifest')")]
    UnknownCategory(String),

    /// Unknown node type
    #[error(
        "Unknown node type: {0} in tag {1} (expected 'file', 'container', 'callable', or 'data')"
    )]
    UnknownNodeType(String, String),

    /// Unknown kind for node type
    #[error("Unknown kind '{kind}' for node type '{node_type}' in tag {tag}")]
    UnknownKind {
        kind: String,
        node_type: String,
        tag: String,
    },

    /// Unknown manifest element type
    #[error(
        "Unknown manifest element type: {0} (expected 'component.name', 'component.version', 'dependency', 'workspace.member', or 'workspace.root')"
    )]
    UnknownManifestElement(String),
}

// ============================================================================
// Tag Parsing
// ============================================================================

/// Parse a kind string into the appropriate NodeKind enum.
fn parse_kind(node_type: NodeType, kind_str: &str) -> Option<NodeKind> {
    match node_type {
        NodeType::Container => match kind_str {
            "namespace" => Some(NodeKind::Container(ContainerKind::Namespace)),
            "module" => Some(NodeKind::Container(ContainerKind::Module)),
            "package" => Some(NodeKind::Container(ContainerKind::Package)),
            "type" => Some(NodeKind::Container(ContainerKind::Type)),
            _ => None,
        },
        NodeType::Callable => match kind_str {
            "function" => Some(NodeKind::Callable(CallableKind::Function)),
            "method" => Some(NodeKind::Callable(CallableKind::Method)),
            "constructor" => Some(NodeKind::Callable(CallableKind::Constructor)),
            "macro" => Some(NodeKind::Callable(CallableKind::Macro)),
            _ => None,
        },
        NodeType::Data => match kind_str {
            "constant" => Some(NodeKind::Data(DataKind::Constant)),
            "value" => Some(NodeKind::Data(DataKind::Value)),
            "field" => Some(NodeKind::Data(DataKind::Field)),
            "property" => Some(NodeKind::Data(DataKind::Property)),
            "parameter" => Some(NodeKind::Data(DataKind::Parameter)),
            "local" => Some(NodeKind::Data(DataKind::Local)),
            _ => None,
        },
    }
}

/// Parse a declarative tag string into components.
///
/// Supports the tag naming convention:
/// `@{def|ref}.{node_type}.{kind}[.{subtype}][.scope.{scope_value}]`
///
/// The "scope." prefix explicitly marks the scope segment, allowing clear
/// distinction from subtypes. Scope values are dynamic and not validated -
/// any suffix is accepted.
///
/// # Arguments
///
/// * `tag` - Tag string from tree-sitter query (e.g., "definition.callable.function.scope.test")
///
/// # Returns
///
/// * `Ok(TagParseResult)` with category, node_type, kind, subtype, and scope
/// * `Err(TagParseError)` if the tag format is invalid
///
/// # Examples
///
/// ```
/// use codeprysm_core::tags::parse_tag_string;
///
/// // Simple function definition
/// let result = parse_tag_string("definition.callable.function").unwrap();
/// assert!(result.is_definition());
///
/// // Class definition with subtype
/// let result = parse_tag_string("definition.container.type.class").unwrap();
/// assert_eq!(result.subtype, Some("class".to_string()));
///
/// // Method with scope
/// let result = parse_tag_string("definition.callable.method.scope.test").unwrap();
/// assert_eq!(result.scope, Some("test".to_string()));
///
/// // File node (Container with kind="file")
/// let result = parse_tag_string("definition.file").unwrap();
/// assert_eq!(result.kind_str(), Some("file"));
/// ```
pub fn parse_tag_string(tag: &str) -> Result<TagParseResult, TagParseError> {
    // Remove @ prefix if present
    let tag = tag.strip_prefix('@').unwrap_or(tag);

    // Split into parts
    let parts: Vec<&str> = tag.split('.').collect();

    // Validate minimum length
    if parts.len() < 2 {
        return Err(TagParseError::TooShort(tag.to_string()));
    }

    // Parse category
    let category = parts[0].parse::<TagCategory>()?;

    // Special case for FILE nodes
    if parts[1] == "file" {
        return Ok(TagParseResult::file(category));
    }

    // Map node_type string to NodeType enum
    let node_type = match parts[1] {
        "container" => NodeType::Container,
        "callable" => NodeType::Callable,
        "data" => NodeType::Data,
        other => {
            return Err(TagParseError::UnknownNodeType(
                other.to_string(),
                tag.to_string(),
            ));
        }
    };

    // Extract kind (required for non-FILE nodes)
    let kind_str = parts.get(2).copied();
    let kind = if let Some(k) = kind_str {
        match parse_kind(node_type, k) {
            Some(kind) => Some(kind),
            None => {
                // Log warning but don't fail - allows forward compatibility
                tracing::warn!(
                    "Unknown kind '{}' for node type '{}' in tag {}",
                    k,
                    node_type.as_str(),
                    tag
                );
                None
            }
        }
    } else {
        None
    };

    // Extract subtype and scope from remaining parts
    // Format: [.subtype][.scope.X]
    let remaining: Vec<&str> = parts.iter().skip(3).copied().collect();
    let (subtype, scope) = parse_subtype_and_scope(&remaining);

    Ok(TagParseResult {
        category,
        node_type,
        kind,
        subtype,
        scope,
    })
}

/// Parse subtype and scope from remaining tag parts.
///
/// The "scope." marker explicitly identifies the scope segment.
/// Everything before "scope" is the subtype (if any).
/// Everything after "scope" is the scope value.
fn parse_subtype_and_scope(remaining: &[&str]) -> (Option<String>, Option<String>) {
    if remaining.is_empty() {
        return (None, None);
    }

    // Look for "scope" marker
    if let Some(scope_idx) = remaining.iter().position(|&s| s == "scope") {
        // Everything before "scope" is subtype (if any)
        let subtype = if scope_idx > 0 {
            Some(remaining[0].to_string())
        } else {
            None
        };

        // Everything after "scope" is the scope value
        let scope = if scope_idx + 1 < remaining.len() {
            Some(remaining[scope_idx + 1].to_string())
        } else {
            None
        };

        (subtype, scope)
    } else {
        // No scope marker, first element is subtype
        (Some(remaining[0].to_string()), None)
    }
}

/// Parse a manifest-specific tag string into components.
///
/// Supports the manifest tag naming convention:
/// `@manifest.{element_type}[.scope.{scope_value}]`
///
/// Where element_type can be:
/// - `component.name` - Component/package name
/// - `component.version` - Component version
/// - `dependency` - Dependency specification
/// - `workspace.member` - Workspace member path
/// - `workspace.root` - Workspace root indicator
///
/// # Arguments
///
/// * `tag` - Tag string from tree-sitter query (e.g., "manifest.component.name")
///
/// # Returns
///
/// * `Ok(ManifestTagResult)` with element_type and optional scope
/// * `Err(TagParseError)` if the tag format is invalid
///
/// # Examples
///
/// ```
/// use codeprysm_core::tags::parse_manifest_tag_string;
///
/// // Component name extraction
/// let result = parse_manifest_tag_string("manifest.component.name").unwrap();
/// assert_eq!(result.element_type.as_str(), "component.name");
///
/// // Dev dependency
/// let result = parse_manifest_tag_string("manifest.dependency.scope.dev").unwrap();
/// assert!(result.is_dev_dependency());
///
/// // Workspace member
/// let result = parse_manifest_tag_string("manifest.workspace.member").unwrap();
/// assert_eq!(result.element_type.as_str(), "workspace.member");
/// ```
pub fn parse_manifest_tag_string(tag: &str) -> Result<ManifestTagResult, TagParseError> {
    // Remove @ prefix if present
    let tag = tag.strip_prefix('@').unwrap_or(tag);

    // Split into parts
    let parts: Vec<&str> = tag.split('.').collect();

    // Validate minimum length: manifest.{something}
    if parts.len() < 2 {
        return Err(TagParseError::TooShort(tag.to_string()));
    }

    // Validate it's a manifest tag
    if parts[0] != "manifest" {
        return Err(TagParseError::UnknownCategory(parts[0].to_string()));
    }

    // Parse the element type and scope
    // Patterns:
    // - manifest.dependency → Dependency with no scope
    // - manifest.dependency.scope.dev → Dependency with dev scope
    // - manifest.component.name → ComponentName
    // - manifest.component.version → ComponentVersion
    // - manifest.workspace.member → WorkspaceMember
    // - manifest.workspace.root → WorkspaceRoot

    let remaining = &parts[1..];

    // Try to match known element types
    let (element_type, scope_start_idx) = if remaining.len() >= 2 {
        match (remaining[0], remaining.get(1).copied()) {
            ("component", Some("name")) => (ManifestElementType::ComponentName, 2),
            ("component", Some("version")) => (ManifestElementType::ComponentVersion, 2),
            ("workspace", Some("member")) => (ManifestElementType::WorkspaceMember, 2),
            ("workspace", Some("root")) => (ManifestElementType::WorkspaceRoot, 2),
            ("dependency", _) => (ManifestElementType::Dependency, 1),
            _ => {
                // Unknown compound element type, try to parse as single element
                let element_str = remaining[0..2.min(remaining.len())].join(".");
                return Err(TagParseError::UnknownManifestElement(element_str));
            }
        }
    } else if remaining.len() == 1 {
        // Single element: only "dependency" is valid
        match remaining[0] {
            "dependency" => (ManifestElementType::Dependency, 1),
            other => return Err(TagParseError::UnknownManifestElement(other.to_string())),
        }
    } else {
        return Err(TagParseError::TooShort(tag.to_string()));
    };

    // Extract scope if present
    // Format: [...].scope.{scope_value}
    let scope_remaining: Vec<&str> = remaining.iter().skip(scope_start_idx).copied().collect();
    let scope = parse_manifest_scope(&scope_remaining);

    Ok(ManifestTagResult::new(element_type, scope))
}

/// Parse scope from remaining manifest tag parts.
///
/// Looks for "scope" marker followed by a scope value.
fn parse_manifest_scope(remaining: &[&str]) -> Option<String> {
    if remaining.len() >= 2 && remaining[0] == "scope" {
        Some(remaining[1].to_string())
    } else {
        None
    }
}

/// Check if a tag string is a manifest tag.
///
/// Returns true if the tag starts with "manifest." or "@manifest."
pub fn is_manifest_tag(tag: &str) -> bool {
    let tag = tag.strip_prefix('@').unwrap_or(tag);
    tag.starts_with("manifest.")
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tag_category_from_str() {
        assert_eq!(
            "definition".parse::<TagCategory>().ok(),
            Some(TagCategory::Definition)
        );
        assert_eq!(
            "reference".parse::<TagCategory>().ok(),
            Some(TagCategory::Reference)
        );
        assert!("unknown".parse::<TagCategory>().is_err());
    }

    #[test]
    fn test_parse_file_definition() {
        let result = parse_tag_string("definition.file").unwrap();
        assert_eq!(result.category, TagCategory::Definition);
        // Files are now Container with kind="file"
        assert_eq!(result.node_type, NodeType::Container);
        assert_eq!(result.kind, Some(NodeKind::Container(ContainerKind::File)));
        assert!(result.subtype.is_none());
        assert!(result.scope.is_none());
    }

    #[test]
    fn test_parse_callable_function() {
        let result = parse_tag_string("definition.callable.function").unwrap();
        assert_eq!(result.category, TagCategory::Definition);
        assert_eq!(result.node_type, NodeType::Callable);
        assert_eq!(
            result.kind,
            Some(NodeKind::Callable(CallableKind::Function))
        );
        assert!(result.subtype.is_none());
        assert!(result.scope.is_none());
    }

    #[test]
    fn test_parse_callable_method_with_scope() {
        let result = parse_tag_string("definition.callable.method.scope.test").unwrap();
        assert_eq!(result.category, TagCategory::Definition);
        assert_eq!(result.node_type, NodeType::Callable);
        assert_eq!(result.kind, Some(NodeKind::Callable(CallableKind::Method)));
        assert!(result.subtype.is_none());
        assert_eq!(result.scope, Some("test".to_string()));
    }

    #[test]
    fn test_parse_container_type_class() {
        let result = parse_tag_string("definition.container.type.class").unwrap();
        assert_eq!(result.category, TagCategory::Definition);
        assert_eq!(result.node_type, NodeType::Container);
        assert_eq!(result.kind, Some(NodeKind::Container(ContainerKind::Type)));
        assert_eq!(result.subtype, Some("class".to_string()));
        assert!(result.scope.is_none());
    }

    #[test]
    fn test_parse_container_type_class_with_scope() {
        let result = parse_tag_string("definition.container.type.class.scope.test").unwrap();
        assert_eq!(result.category, TagCategory::Definition);
        assert_eq!(result.node_type, NodeType::Container);
        assert_eq!(result.kind, Some(NodeKind::Container(ContainerKind::Type)));
        assert_eq!(result.subtype, Some("class".to_string()));
        assert_eq!(result.scope, Some("test".to_string()));
    }

    #[test]
    fn test_parse_reference_data_field() {
        let result = parse_tag_string("reference.data.field").unwrap();
        assert_eq!(result.category, TagCategory::Reference);
        assert_eq!(result.node_type, NodeType::Data);
        assert_eq!(result.kind, Some(NodeKind::Data(DataKind::Field)));
        assert!(result.subtype.is_none());
        assert!(result.scope.is_none());
    }

    #[test]
    fn test_parse_with_at_prefix() {
        let result = parse_tag_string("@definition.callable.function").unwrap();
        assert_eq!(result.category, TagCategory::Definition);
        assert_eq!(result.node_type, NodeType::Callable);
    }

    #[test]
    fn test_parse_all_callable_kinds() {
        let cases = [
            ("definition.callable.function", CallableKind::Function),
            ("definition.callable.method", CallableKind::Method),
            ("definition.callable.constructor", CallableKind::Constructor),
            ("definition.callable.macro", CallableKind::Macro),
        ];

        for (tag, expected_kind) in cases {
            let result = parse_tag_string(tag).unwrap();
            assert_eq!(result.kind, Some(NodeKind::Callable(expected_kind)));
        }
    }

    #[test]
    fn test_parse_all_container_kinds() {
        let cases = [
            ("definition.container.namespace", ContainerKind::Namespace),
            ("definition.container.module", ContainerKind::Module),
            ("definition.container.package", ContainerKind::Package),
            ("definition.container.type", ContainerKind::Type),
        ];

        for (tag, expected_kind) in cases {
            let result = parse_tag_string(tag).unwrap();
            assert_eq!(result.kind, Some(NodeKind::Container(expected_kind)));
        }
    }

    #[test]
    fn test_parse_all_data_kinds() {
        let cases = [
            ("definition.data.constant", DataKind::Constant),
            ("definition.data.value", DataKind::Value),
            ("definition.data.field", DataKind::Field),
            ("definition.data.property", DataKind::Property),
            ("definition.data.parameter", DataKind::Parameter),
            ("definition.data.local", DataKind::Local),
        ];

        for (tag, expected_kind) in cases {
            let result = parse_tag_string(tag).unwrap();
            assert_eq!(result.kind, Some(NodeKind::Data(expected_kind)));
        }
    }

    #[test]
    fn test_parse_error_too_short() {
        let result = parse_tag_string("definition");
        assert!(matches!(result, Err(TagParseError::TooShort(_))));
    }

    #[test]
    fn test_parse_error_unknown_category() {
        let result = parse_tag_string("unknown.callable.function");
        assert!(matches!(result, Err(TagParseError::UnknownCategory(_))));
    }

    #[test]
    fn test_parse_error_unknown_node_type() {
        let result = parse_tag_string("definition.unknown.function");
        assert!(matches!(result, Err(TagParseError::UnknownNodeType(_, _))));
    }

    #[test]
    fn test_parse_unknown_kind_returns_none() {
        // Unknown kinds should return None but not error (forward compatibility)
        let result = parse_tag_string("definition.callable.unknown_kind").unwrap();
        assert_eq!(result.category, TagCategory::Definition);
        assert_eq!(result.node_type, NodeType::Callable);
        assert!(result.kind.is_none());
    }

    #[test]
    fn test_is_definition_and_is_reference() {
        let def = parse_tag_string("definition.callable.function").unwrap();
        assert!(def.is_definition());
        assert!(!def.is_reference());

        let ref_ = parse_tag_string("reference.data.field").unwrap();
        assert!(!ref_.is_definition());
        assert!(ref_.is_reference());
    }

    #[test]
    fn test_kind_str() {
        let result = parse_tag_string("definition.callable.function").unwrap();
        assert_eq!(result.kind_str(), Some("function"));

        // Files are now Container with kind="file"
        let file = parse_tag_string("definition.file").unwrap();
        assert_eq!(file.kind_str(), Some("file"));
    }

    #[test]
    fn test_display() {
        let result = parse_tag_string("definition.callable.function").unwrap();
        let display = format!("{}", result);
        assert!(display.contains("definition"));
        assert!(display.contains("Callable"));
        assert!(display.contains("function"));
    }

    // ========================================================================
    // Manifest Tag Tests
    // ========================================================================

    #[test]
    fn test_tag_category_manifest() {
        assert_eq!(
            "manifest".parse::<TagCategory>().ok(),
            Some(TagCategory::Manifest)
        );
        assert_eq!(TagCategory::Manifest.as_str(), "manifest");
    }

    #[test]
    fn test_parse_manifest_component_name() {
        let result = parse_manifest_tag_string("manifest.component.name").unwrap();
        assert_eq!(result.element_type, ManifestElementType::ComponentName);
        assert!(result.scope.is_none());
    }

    #[test]
    fn test_parse_manifest_component_version() {
        let result = parse_manifest_tag_string("manifest.component.version").unwrap();
        assert_eq!(result.element_type, ManifestElementType::ComponentVersion);
        assert!(result.scope.is_none());
    }

    #[test]
    fn test_parse_manifest_dependency() {
        let result = parse_manifest_tag_string("manifest.dependency").unwrap();
        assert_eq!(result.element_type, ManifestElementType::Dependency);
        assert!(result.scope.is_none());
        assert!(!result.is_dev_dependency());
    }

    #[test]
    fn test_parse_manifest_dependency_with_dev_scope() {
        let result = parse_manifest_tag_string("manifest.dependency.scope.dev").unwrap();
        assert_eq!(result.element_type, ManifestElementType::Dependency);
        assert_eq!(result.scope, Some("dev".to_string()));
        assert!(result.is_dev_dependency());
        assert!(!result.is_build_dependency());
    }

    #[test]
    fn test_parse_manifest_dependency_with_build_scope() {
        let result = parse_manifest_tag_string("manifest.dependency.scope.build").unwrap();
        assert_eq!(result.element_type, ManifestElementType::Dependency);
        assert_eq!(result.scope, Some("build".to_string()));
        assert!(!result.is_dev_dependency());
        assert!(result.is_build_dependency());
    }

    #[test]
    fn test_parse_manifest_workspace_member() {
        let result = parse_manifest_tag_string("manifest.workspace.member").unwrap();
        assert_eq!(result.element_type, ManifestElementType::WorkspaceMember);
        assert!(result.scope.is_none());
    }

    #[test]
    fn test_parse_manifest_workspace_root() {
        let result = parse_manifest_tag_string("manifest.workspace.root").unwrap();
        assert_eq!(result.element_type, ManifestElementType::WorkspaceRoot);
        assert!(result.scope.is_none());
    }

    #[test]
    fn test_parse_manifest_with_at_prefix() {
        let result = parse_manifest_tag_string("@manifest.component.name").unwrap();
        assert_eq!(result.element_type, ManifestElementType::ComponentName);
    }

    #[test]
    fn test_parse_manifest_error_too_short() {
        let result = parse_manifest_tag_string("manifest");
        assert!(matches!(result, Err(TagParseError::TooShort(_))));
    }

    #[test]
    fn test_parse_manifest_error_unknown_element() {
        let result = parse_manifest_tag_string("manifest.unknown");
        assert!(matches!(
            result,
            Err(TagParseError::UnknownManifestElement(_))
        ));

        let result = parse_manifest_tag_string("manifest.component.unknown");
        assert!(matches!(
            result,
            Err(TagParseError::UnknownManifestElement(_))
        ));
    }

    #[test]
    fn test_parse_manifest_error_wrong_category() {
        // This should fail because it's not a manifest tag
        let result = parse_manifest_tag_string("definition.component.name");
        assert!(matches!(result, Err(TagParseError::UnknownCategory(_))));
    }

    #[test]
    fn test_is_manifest_tag() {
        assert!(is_manifest_tag("manifest.component.name"));
        assert!(is_manifest_tag("@manifest.dependency"));
        assert!(is_manifest_tag("manifest.workspace.member"));

        assert!(!is_manifest_tag("definition.callable.function"));
        assert!(!is_manifest_tag("reference.data.field"));
        assert!(!is_manifest_tag("unknown.something"));
    }

    #[test]
    fn test_manifest_element_type_display() {
        assert_eq!(
            ManifestElementType::ComponentName.as_str(),
            "component.name"
        );
        assert_eq!(
            ManifestElementType::ComponentVersion.as_str(),
            "component.version"
        );
        assert_eq!(ManifestElementType::Dependency.as_str(), "dependency");
        assert_eq!(
            ManifestElementType::WorkspaceMember.as_str(),
            "workspace.member"
        );
        assert_eq!(
            ManifestElementType::WorkspaceRoot.as_str(),
            "workspace.root"
        );
    }

    #[test]
    fn test_manifest_element_type_from_str() {
        assert_eq!(
            "component.name".parse::<ManifestElementType>().ok(),
            Some(ManifestElementType::ComponentName)
        );
        assert_eq!(
            "dependency".parse::<ManifestElementType>().ok(),
            Some(ManifestElementType::Dependency)
        );
        assert!("unknown".parse::<ManifestElementType>().is_err());
    }

    #[test]
    fn test_manifest_tag_result_constructors() {
        let name = ManifestTagResult::component_name();
        assert_eq!(name.element_type, ManifestElementType::ComponentName);
        assert!(name.scope.is_none());

        let version = ManifestTagResult::component_version();
        assert_eq!(version.element_type, ManifestElementType::ComponentVersion);

        let dep = ManifestTagResult::dependency(Some("dev".to_string()));
        assert_eq!(dep.element_type, ManifestElementType::Dependency);
        assert!(dep.is_dev_dependency());

        let member = ManifestTagResult::workspace_member();
        assert_eq!(member.element_type, ManifestElementType::WorkspaceMember);

        let root = ManifestTagResult::workspace_root();
        assert_eq!(root.element_type, ManifestElementType::WorkspaceRoot);
    }

    #[test]
    fn test_manifest_tag_result_display() {
        let result = ManifestTagResult::dependency(Some("dev".to_string()));
        let display = format!("{}", result);
        assert!(display.contains("dependency"));
        assert!(display.contains("dev"));
    }
}
