//! Semantic text builder for code graph indexing.
//!
//! Creates rich natural language descriptions of code entities for semantic search.
//! This enables queries like "error handling" or "HTTP client" to find relevant code.
//!
//! ## Design
//!
//! The builder traverses the code graph to extract:
//! - Entity metadata (modifiers, visibility, decorators)
//! - Parent context (containing class/module)
//! - Children context (methods/fields for containers)
//! - References (what the entity uses/calls)
//! - Semantic keywords (detected patterns)
//!
//! ## Memory Considerations
//!
//! When using `LazyGraphManager` (streaming mode), each context lookup may load
//! a different partition. For highly interconnected codebases, this can increase
//! memory usage. Use `SemanticTextConfig::minimal()` to skip cross-partition
//! context lookups when memory is constrained.
//!
//! ### Context Depth
//!
//! The builder limits traversal to depth=1 (single level):
//! - Parent context: one level up in containment hierarchy
//! - Children context: immediate children only (max 5)
//! - References: direct outgoing edges only (max 5 per type)
//!
//! ## Example Output
//!
//! For a method:
//! ```text
//! public async method processRequest(data, config) in class RequestHandler
//! in file handlers.py. calls validate, transform. uses HttpResponse, Logger.
//! handles HTTP requests, error handling, validation
//! ```

use std::collections::HashSet;

use codeprysm_core::{EdgeType, Node, NodeType};

use crate::graph_context::GraphContext;

/// Default maximum number of children to include in description
const DEFAULT_MAX_CHILDREN: usize = 5;
/// Default maximum number of references to include
const DEFAULT_MAX_REFERENCES: usize = 5;
/// Maximum content preview length
const MAX_CONTENT_PREVIEW: usize = 300;

/// Configuration for semantic text generation.
///
/// Controls which context is included and limits for memory-bounded operation.
#[derive(Debug, Clone)]
pub struct SemanticTextConfig {
    /// Include parent context (containing class/module)
    ///
    /// When disabled, skips cross-partition parent lookups.
    /// Default: true
    pub include_parent_context: bool,

    /// Include children context (methods/fields for containers)
    ///
    /// When disabled, skips cross-partition children lookups.
    /// Default: true
    pub include_children_context: bool,

    /// Include inheritance context (extends/implements)
    ///
    /// When disabled, skips cross-partition inheritance lookups.
    /// Default: true
    pub include_inheritance_context: bool,

    /// Include references context (calls/uses)
    ///
    /// When disabled, skips cross-partition reference lookups.
    /// Default: true
    pub include_references_context: bool,

    /// Maximum number of children to include
    ///
    /// Limits cross-partition children resolution.
    /// Default: 5
    pub max_children: usize,

    /// Maximum number of references to include (per type)
    ///
    /// Limits cross-partition reference resolution.
    /// Default: 5
    pub max_references: usize,
}

impl Default for SemanticTextConfig {
    fn default() -> Self {
        Self {
            include_parent_context: true,
            include_children_context: true,
            include_inheritance_context: true,
            include_references_context: true,
            max_children: DEFAULT_MAX_CHILDREN,
            max_references: DEFAULT_MAX_REFERENCES,
        }
    }
}

impl SemanticTextConfig {
    /// Full context configuration (default).
    ///
    /// Includes all context lookups. Best for in-memory graphs (`PetCodeGraph`)
    /// where cross-partition lookups are fast.
    pub fn full() -> Self {
        Self::default()
    }

    /// Minimal context configuration for memory-bounded streaming.
    ///
    /// Skips all cross-partition context lookups (parent, children, inheritance,
    /// references). Use this with `LazyGraphManager` to minimize memory usage
    /// at the cost of less rich semantic descriptions.
    ///
    /// The resulting text will still include:
    /// - Entity metadata (modifiers, visibility, decorators)
    /// - File context
    /// - Semantic keywords
    /// - Code preview
    pub fn minimal() -> Self {
        Self {
            include_parent_context: false,
            include_children_context: false,
            include_inheritance_context: false,
            include_references_context: false,
            max_children: 0,
            max_references: 0,
        }
    }

    /// Streaming configuration with limited context.
    ///
    /// A balanced option for streaming mode that includes some context
    /// but with reduced limits to bound memory usage.
    ///
    /// - Parent context: enabled (single lookup)
    /// - Children context: disabled (could load many partitions)
    /// - Inheritance context: disabled
    /// - References context: enabled but limited to 5 per type
    pub fn streaming() -> Self {
        Self {
            include_parent_context: true,
            include_children_context: false,
            include_inheritance_context: false,
            include_references_context: true,
            max_children: 0,
            max_references: 5,
        }
    }
}

/// Builder for creating semantic text descriptions of code entities.
///
/// Uses graph traversal to build rich context for better semantic search.
///
/// # Type Parameters
///
/// * `G` - The graph context type. Can be:
///   - `&PetCodeGraph` for in-memory graphs
///   - `&LazyGraphManager` for streaming/lazy-loaded graphs
///
/// # Memory Usage
///
/// For streaming mode (`LazyGraphManager`), use `new_with_config()` with
/// `SemanticTextConfig::streaming()` or `SemanticTextConfig::minimal()` to
/// reduce cross-partition lookups and bound memory usage.
///
/// # Example
///
/// ```rust,ignore
/// use codeprysm_search::{SemanticTextBuilder, SemanticTextConfig};
/// use codeprysm_core::PetCodeGraph;
///
/// // Full context (default) for in-memory graphs
/// let graph = PetCodeGraph::new();
/// let builder = SemanticTextBuilder::new(&graph);
/// let text = builder.build(&node, "fn main() {}");
///
/// // Streaming config for memory-bounded operation
/// let lazy_manager = LazyGraphManager::open(&path)?;
/// let builder = SemanticTextBuilder::new_with_config(&lazy_manager, SemanticTextConfig::streaming());
/// ```
pub struct SemanticTextBuilder<G: GraphContext> {
    graph: G,
    config: SemanticTextConfig,
}

impl<G: GraphContext> SemanticTextBuilder<G> {
    /// Create a new semantic text builder with full context configuration.
    ///
    /// This is equivalent to `new_with_config(graph, SemanticTextConfig::full())`.
    ///
    /// The graph can be any type implementing `GraphContext`:
    /// - `&PetCodeGraph` for full in-memory access
    /// - `&LazyGraphManager` for memory-bounded streaming access
    pub fn new(graph: G) -> Self {
        Self {
            graph,
            config: SemanticTextConfig::full(),
        }
    }

    /// Create a new semantic text builder with custom configuration.
    ///
    /// Use this with `SemanticTextConfig::streaming()` or `SemanticTextConfig::minimal()`
    /// to reduce memory usage when using `LazyGraphManager`.
    ///
    /// # Example
    ///
    /// ```rust,ignore
    /// // Minimal context for maximum memory savings
    /// let builder = SemanticTextBuilder::new_with_config(
    ///     &lazy_manager,
    ///     SemanticTextConfig::minimal(),
    /// );
    ///
    /// // Streaming config with limited context
    /// let builder = SemanticTextBuilder::new_with_config(
    ///     &lazy_manager,
    ///     SemanticTextConfig::streaming(),
    /// );
    /// ```
    pub fn new_with_config(graph: G, config: SemanticTextConfig) -> Self {
        Self { graph, config }
    }

    /// Build semantic text for a node.
    ///
    /// The text is structured for optimal embedding:
    /// 1. Entity type and name with modifiers
    /// 2. Inheritance/implementation info (for containers) - if config.include_inheritance_context
    /// 3. Parameters (for callables)
    /// 4. Children context (for containers) - if config.include_children_context
    /// 5. Parent context (containing class/module) - if config.include_parent_context
    /// 6. File context
    /// 7. References (calls, uses) - if config.include_references_context
    /// 8. Semantic keywords
    /// 9. Code preview
    ///
    /// Context sections (2, 4, 5, 7) can be disabled via `SemanticTextConfig` to
    /// reduce cross-partition lookups in streaming mode.
    pub fn build(&self, node: &Node, content: &str) -> String {
        let mut parts = Vec::new();

        // 1. Build entity description with modifiers (always included)
        parts.push(self.build_entity_description(node));

        // 2. Add inheritance info for containers (configurable)
        if self.config.include_inheritance_context && node.node_type == NodeType::Container {
            if let Some(inheritance) = self.build_inheritance_context(node) {
                parts.push(inheritance);
            }
        }

        // 3. Add parameters for callables (always included - extracted from content, no graph lookup)
        if node.node_type == NodeType::Callable {
            if let Some(params) = self.extract_parameters(content) {
                parts.push(format!("({})", params));
            }
        }

        // 4. Add children context for containers (configurable)
        if self.config.include_children_context
            && node.node_type == NodeType::Container
            && !node.is_file()
        {
            if let Some(children_ctx) = self.build_children_context(node) {
                parts.push(children_ctx);
            }
        }

        // 5. Add parent context (configurable)
        if self.config.include_parent_context {
            if let Some(parent_ctx) = self.build_parent_context(node) {
                parts.push(parent_ctx);
            }
        }

        // 6. Add file context (always included - no graph lookup)
        parts.push(format!("in file {}", self.format_file_path(&node.file)));

        // 7. Add references context (configurable)
        if self.config.include_references_context {
            if let Some(refs_ctx) = self.build_references_context(node) {
                parts.push(refs_ctx);
            }
        }

        // 8. Add semantic keywords based on patterns (always included - no graph lookup)
        let keywords = self.extract_semantic_keywords(node, content);
        if !keywords.is_empty() {
            parts.push(format!("related to: {}", keywords.join(", ")));
        }

        // 9. Add content preview (always included - no graph lookup)
        let preview = self.truncate_content(content, MAX_CONTENT_PREVIEW);
        if !preview.is_empty() {
            parts.push(format!("code: {}", preview));
        }

        parts.join(". ")
    }

    /// Build entity description with modifiers.
    ///
    /// Examples:
    /// - "public async method processRequest"
    /// - "private static field logger"
    /// - "abstract class BaseHandler"
    fn build_entity_description(&self, node: &Node) -> String {
        let mut desc_parts = Vec::new();

        // Add visibility
        if let Some(ref visibility) = node.metadata.visibility {
            desc_parts.push(visibility.clone());
        }

        // Add modifiers from metadata
        if node.metadata.is_static == Some(true) {
            desc_parts.push("static".to_string());
        }
        if node.metadata.is_async == Some(true) {
            desc_parts.push("async".to_string());
        }
        if node.metadata.is_abstract == Some(true) {
            desc_parts.push("abstract".to_string());
        }
        if node.metadata.is_virtual == Some(true) {
            desc_parts.push("virtual".to_string());
        }

        // Add additional modifiers
        if let Some(ref modifiers) = node.metadata.modifiers {
            for modifier in modifiers {
                if !desc_parts.contains(modifier) {
                    desc_parts.push(modifier.clone());
                }
            }
        }

        // Add type descriptor
        let type_desc = self.get_type_descriptor(node);
        desc_parts.push(type_desc);

        // Add name
        desc_parts.push(node.name.clone());

        // Add decorators if present
        if let Some(ref decorators) = node.metadata.decorators {
            if !decorators.is_empty() {
                let decorator_names: Vec<&str> = decorators
                    .iter()
                    .take(3)
                    .map(|d| {
                        // Clean decorator name (remove @ and arguments)
                        let clean = d.trim_start_matches('@');
                        if let Some(paren_idx) = clean.find('(') {
                            &clean[..paren_idx]
                        } else {
                            clean
                        }
                    })
                    .collect();
                desc_parts.push(format!("decorated with {}", decorator_names.join(", ")));
            }
        }

        desc_parts.join(" ")
    }

    /// Get human-readable type descriptor.
    fn get_type_descriptor(&self, node: &Node) -> String {
        match node.node_type {
            NodeType::Container => {
                let kind = node.kind.as_deref().unwrap_or("type");
                if kind == "file" {
                    "file".to_string()
                } else {
                    let subtype = node.subtype.as_deref().unwrap_or(kind);
                    subtype.to_string()
                }
            }
            NodeType::Callable => {
                let kind = node.kind.as_deref().unwrap_or("function");
                kind.to_string()
            }
            NodeType::Data => {
                let kind = node.kind.as_deref().unwrap_or("variable");
                kind.to_string()
            }
        }
    }

    /// Build inheritance context for containers.
    ///
    /// Looks at USES edges from the container to find base classes/interfaces.
    fn build_inheritance_context(&self, node: &Node) -> Option<String> {
        let mut extends = Vec::new();
        let mut implements = Vec::new();

        // Look at outgoing USES edges from this node
        for (target, edge_data) in self.graph.get_outgoing_edges(&node.id) {
            if edge_data.edge_type == EdgeType::Uses {
                // Check if target is a type reference
                if target.node_type == NodeType::Container {
                    let target_kind = target.kind.as_deref().unwrap_or("");
                    if target_kind == "interface" || target.subtype.as_deref() == Some("interface")
                    {
                        implements.push(target.name.clone());
                    } else {
                        extends.push(target.name.clone());
                    }
                }
            }
        }

        let mut parts = Vec::new();
        if !extends.is_empty() {
            parts.push(format!("extends {}", extends.join(", ")));
        }
        if !implements.is_empty() {
            parts.push(format!("implements {}", implements.join(", ")));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" "))
        }
    }

    /// Build children context for containers.
    ///
    /// Lists methods and fields contained in the class/struct.
    /// The number of children is limited by `config.max_children` to bound
    /// cross-partition lookups in streaming mode.
    fn build_children_context(&self, node: &Node) -> Option<String> {
        let max_children = self.config.max_children;
        if max_children == 0 {
            return None;
        }

        let mut methods = Vec::new();
        let mut fields = Vec::new();
        let mut properties = Vec::new();

        for child in self.graph.get_children(&node.id) {
            match child.node_type {
                NodeType::Callable => {
                    if methods.len() < max_children {
                        methods.push(child.name.clone());
                    }
                }
                NodeType::Data => {
                    let kind = child.kind.as_deref().unwrap_or("");
                    if kind == "property" {
                        if properties.len() < max_children {
                            properties.push(child.name.clone());
                        }
                    } else if fields.len() < max_children {
                        fields.push(child.name.clone());
                    }
                }
                _ => {}
            }
        }

        let mut parts = Vec::new();
        if !methods.is_empty() {
            parts.push(format!("with methods {}", methods.join(", ")));
        }
        if !properties.is_empty() {
            parts.push(format!("properties {}", properties.join(", ")));
        }
        if !fields.is_empty() {
            parts.push(format!("fields {}", fields.join(", ")));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" and "))
        }
    }

    /// Build parent context.
    ///
    /// Shows the containing class/module/namespace.
    fn build_parent_context(&self, node: &Node) -> Option<String> {
        // Skip parent context for file nodes and top-level definitions
        if node.is_file() {
            return None;
        }

        if let Some(parent) = self.graph.get_parent(&node.id) {
            // Skip if parent is just a file
            if parent.is_file() {
                return None;
            }

            let parent_type = self.get_type_descriptor(&parent);
            Some(format!("in {} {}", parent_type, parent.name))
        } else {
            None
        }
    }

    /// Build references context.
    ///
    /// Lists what the entity calls/uses.
    /// The number of references per type is limited by `config.max_references`
    /// to bound cross-partition lookups in streaming mode.
    fn build_references_context(&self, node: &Node) -> Option<String> {
        let max_references = self.config.max_references;
        if max_references == 0 {
            return None;
        }

        let mut calls = Vec::new();
        let mut uses_types = Vec::new();
        let mut uses_data = Vec::new();

        for (target, edge_data) in self.graph.get_outgoing_edges(&node.id) {
            if edge_data.edge_type == EdgeType::Uses {
                match target.node_type {
                    NodeType::Callable => {
                        if calls.len() < max_references {
                            calls.push(target.name.clone());
                        }
                    }
                    NodeType::Container if !target.is_file() => {
                        if uses_types.len() < max_references {
                            uses_types.push(target.name.clone());
                        }
                    }
                    NodeType::Data => {
                        if uses_data.len() < max_references {
                            uses_data.push(target.name.clone());
                        }
                    }
                    _ => {}
                }
            }
        }

        let mut parts = Vec::new();
        if !calls.is_empty() {
            parts.push(format!("calls {}", calls.join(", ")));
        }
        if !uses_types.is_empty() {
            parts.push(format!("uses types {}", uses_types.join(", ")));
        }
        if !uses_data.is_empty() {
            parts.push(format!("uses {}", uses_data.join(", ")));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(". "))
        }
    }

    /// Extract semantic keywords based on naming patterns and content.
    ///
    /// This is an improvement over the Python implementation - we detect
    /// common patterns to add searchable keywords.
    fn extract_semantic_keywords(&self, node: &Node, content: &str) -> Vec<String> {
        let mut keywords = HashSet::new();
        let name_lower = node.name.to_lowercase();
        let content_lower = content.to_lowercase();

        // Pattern detection for names
        let patterns = [
            // Error/Exception handling
            (
                &["error", "exception", "fault", "fail"][..],
                "error handling",
            ),
            (&["handler", "handle"], "handler"),
            (&["catch", "throw", "raise"], "exception handling"),
            // HTTP/Network
            (&["http", "request", "response"], "HTTP"),
            (&["client", "connection", "socket"], "networking"),
            (&["api", "endpoint", "route"], "API"),
            (&["rest", "grpc", "graphql"], "API"),
            // Data/Storage
            (&["repository", "repo", "store", "storage"], "data storage"),
            (&["database", "db", "sql", "query"], "database"),
            (&["cache", "caching", "redis", "memcache"], "caching"),
            // Authentication/Security
            (
                &["auth", "authentication", "login", "logout"],
                "authentication",
            ),
            (&["token", "jwt", "oauth", "credential"], "authentication"),
            (&["permission", "authorize", "role", "acl"], "authorization"),
            (&["encrypt", "decrypt", "hash", "security"], "security"),
            // Async/Concurrency
            (&["async", "await", "task", "future"], "asynchronous"),
            (&["thread", "mutex", "lock", "concurrent"], "concurrency"),
            (
                &["queue", "worker", "job", "background"],
                "background processing",
            ),
            // Logging/Monitoring
            (&["log", "logger", "logging"], "logging"),
            (&["metric", "monitor", "trace", "telemetry"], "monitoring"),
            // Configuration
            (
                &["config", "configuration", "settings", "options"],
                "configuration",
            ),
            (&["env", "environment", "variable"], "configuration"),
            // Testing
            (&["test", "spec", "mock", "stub", "fixture"], "testing"),
            (&["assert", "expect", "should"], "testing"),
            // Serialization
            (
                &["serialize", "deserialize", "json", "xml"],
                "serialization",
            ),
            (&["parse", "parser", "format", "formatter"], "parsing"),
            // Events
            (
                &["event", "listener", "subscriber", "publish"],
                "event handling",
            ),
            (&["callback", "hook", "trigger"], "callbacks"),
            // Validation
            (&["valid", "validate", "validator", "check"], "validation"),
            (&["sanitize", "clean", "normalize"], "data processing"),
            // Factory/Builder patterns
            (&["factory", "builder", "creator"], "factory pattern"),
            (&["singleton", "instance"], "singleton pattern"),
            // Collections/Data structures
            (&["list", "array", "collection", "set"], "collections"),
            (&["map", "dict", "dictionary", "hash"], "collections"),
            (&["tree", "graph", "node"], "data structures"),
            // I/O
            (&["file", "read", "write", "stream"], "file I/O"),
            (&["input", "output", "io"], "I/O"),
            // Lifecycle
            (&["init", "initialize", "setup", "start"], "initialization"),
            (&["dispose", "cleanup", "close", "shutdown"], "cleanup"),
            (&["create", "delete", "update", "remove"], "CRUD"),
        ];

        for (triggers, keyword) in patterns {
            for trigger in triggers {
                if name_lower.contains(trigger) || content_lower.contains(trigger) {
                    keywords.insert(keyword.to_string());
                    break;
                }
            }
        }

        // Add scope-based keywords
        if let Some(ref scope) = node.metadata.scope {
            match scope.as_str() {
                "test" => {
                    keywords.insert("testing".to_string());
                }
                "benchmark" => {
                    keywords.insert("performance".to_string());
                }
                "example" => {
                    keywords.insert("documentation".to_string());
                }
                _ => {}
            }
        }

        keywords.into_iter().collect()
    }

    /// Extract parameter names from function/method content.
    fn extract_parameters(&self, content: &str) -> Option<String> {
        // Find the first parentheses pair for parameters
        let start = content.find('(')?;
        let end = content.find(')')?;

        if end <= start + 1 {
            return None; // Empty parameters
        }

        let params_str = &content[start + 1..end];

        // Parse parameters (handle various formats)
        let params: Vec<&str> = params_str
            .split(',')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .filter_map(|p| {
                // Skip 'self', 'this', 'cls'
                let p_lower = p.to_lowercase();
                if p_lower == "self" || p_lower == "this" || p_lower == "cls" {
                    return None;
                }

                // Extract just the parameter name (handle typed parameters)
                // Patterns: "name: type", "type name", "name = default"
                let name = if let Some(colon_idx) = p.find(':') {
                    // Python/TypeScript style: "name: type"
                    p[..colon_idx].trim()
                } else if let Some(eq_idx) = p.find('=') {
                    // Default value: "name = value"
                    p[..eq_idx].trim()
                } else {
                    // Get last word (handles "type name" pattern in C#/Java)
                    p.split_whitespace().last().unwrap_or(p)
                };

                if name.is_empty() {
                    None
                } else {
                    Some(name)
                }
            })
            .take(10)
            .collect();

        if params.is_empty() {
            None
        } else {
            Some(params.join(", "))
        }
    }

    /// Format file path for readability.
    fn format_file_path(&self, path: &str) -> String {
        // Keep last 3 path components for context
        let parts: Vec<&str> = path.split('/').collect();
        if parts.len() <= 3 {
            path.to_string()
        } else {
            parts[parts.len() - 3..].join("/")
        }
    }

    /// Truncate content to a maximum length, preserving word boundaries.
    ///
    /// Uses character-safe truncation to avoid panics on multi-byte UTF-8 content.
    fn truncate_content(&self, content: &str, max_len: usize) -> String {
        // Clean content - normalize whitespace
        let cleaned: String = content.split_whitespace().collect::<Vec<&str>>().join(" ");

        if cleaned.len() <= max_len {
            cleaned
        } else {
            // Find the last valid UTF-8 character boundary at or before max_len bytes
            let truncate_at = find_utf8_truncation_point(&cleaned, max_len);

            if truncate_at == 0 {
                return String::new();
            }

            let truncated = &cleaned[..truncate_at];

            // Find a good break point at word boundary
            if let Some(last_space) = truncated.rfind(' ') {
                format!("{}...", &truncated[..last_space])
            } else {
                format!("{}...", truncated)
            }
        }
    }
}

/// Find the byte index of the last complete UTF-8 character that fits within max_bytes.
///
/// This ensures we never slice in the middle of a multi-byte character.
/// Returns the byte position after the last complete character that fits.
fn find_utf8_truncation_point(s: &str, max_bytes: usize) -> usize {
    if s.len() <= max_bytes {
        return s.len();
    }

    // Find the last character whose END position is <= max_bytes
    // (i.e., the character completely fits within the budget)
    s.char_indices()
        .take_while(|(i, c)| *i + c.len_utf8() <= max_bytes)
        .last()
        .map(|(i, c)| i + c.len_utf8())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeprysm_core::{EdgeData, NodeMetadata, PetCodeGraph};

    fn create_test_graph() -> PetCodeGraph {
        let mut graph = PetCodeGraph::new();

        // Create a class node with metadata
        let class_node = Node {
            id: "test.py:MyClass".to_string(),
            name: "MyClass".to_string(),
            node_type: NodeType::Container,
            kind: Some("type".to_string()),
            subtype: Some("class".to_string()),
            file: "test.py".to_string(),
            line: 1,
            end_line: 50,
            text: None,
            metadata: NodeMetadata {
                visibility: Some("public".to_string()),
                ..Default::default()
            },
            hash: None,
        };
        graph.add_node(class_node);

        // Create a method node
        let method_node = Node {
            id: "test.py:MyClass:process".to_string(),
            name: "process".to_string(),
            node_type: NodeType::Callable,
            kind: Some("method".to_string()),
            subtype: None,
            file: "test.py".to_string(),
            line: 10,
            end_line: 20,
            text: None,
            metadata: NodeMetadata {
                visibility: Some("public".to_string()),
                is_async: Some(true),
                ..Default::default()
            },
            hash: None,
        };
        graph.add_node(method_node);

        // Add CONTAINS edge
        graph.add_edge(
            "test.py:MyClass",
            "test.py:MyClass:process",
            EdgeData::contains(),
        );

        graph
    }

    #[test]
    fn test_build_entity_description() {
        let graph = create_test_graph();
        let builder = SemanticTextBuilder::new(&graph);

        // Test class description
        let class_node = graph.get_node("test.py:MyClass").unwrap();
        let desc = builder.build_entity_description(class_node);
        assert!(desc.contains("public"));
        assert!(desc.contains("class"));
        assert!(desc.contains("MyClass"));

        // Test async method description
        let method_node = graph.get_node("test.py:MyClass:process").unwrap();
        let desc = builder.build_entity_description(method_node);
        assert!(desc.contains("public"));
        assert!(desc.contains("async"));
        assert!(desc.contains("method"));
        assert!(desc.contains("process"));
    }

    #[test]
    fn test_extract_semantic_keywords() {
        let graph = PetCodeGraph::new();
        let builder = SemanticTextBuilder::new(&graph);

        // Create a node for testing
        let node = Node {
            id: "test.py:ErrorHandler".to_string(),
            name: "ErrorHandler".to_string(),
            node_type: NodeType::Container,
            kind: Some("type".to_string()),
            subtype: Some("class".to_string()),
            file: "test.py".to_string(),
            line: 1,
            end_line: 50,
            text: None,
            metadata: NodeMetadata::default(),
            hash: None,
        };

        let keywords = builder.extract_semantic_keywords(&node, "def handle_exception():");
        assert!(keywords.contains(&"error handling".to_string()));
        assert!(keywords.contains(&"handler".to_string()));
    }

    #[test]
    fn test_full_semantic_text_for_exception_method() {
        let graph = PetCodeGraph::new();
        let builder = SemanticTextBuilder::new(&graph);

        // Create a TraceException-like node
        let node = Node {
            id: "test:TraceException".to_string(),
            name: "TraceException".to_string(),
            node_type: NodeType::Callable,
            kind: Some("method".to_string()),
            subtype: None,
            file: "src/Services/Logger.cs".to_string(),
            line: 442,
            end_line: 450,
            text: None,
            metadata: NodeMetadata::default(),
            hash: None,
        };

        let content = r#"public void TraceException(string originMethod, string originFile, ExceptionCategory exceptionCategory, Exception ex)
        {
            this.logger.TraceException(originMethod, originFile, exceptionCategory, ex);
        }"#;

        let result = builder.build(&node, content);
        println!("\n=== Semantic text for TraceException ===\n{}\n", result);

        // Verify it contains expected content
        assert!(
            result.contains("TraceException"),
            "Should contain method name"
        );
        assert!(result.contains("method"), "Should contain entity type");
        // This is the key test - exception should trigger error handling keyword
        assert!(
            result.contains("exception handling") || result.contains("error handling"),
            "Should detect exception-related keywords. Got: {}",
            result
        );
    }

    #[test]
    fn test_extract_parameters() {
        let graph = PetCodeGraph::new();
        let builder = SemanticTextBuilder::new(&graph);

        // Python style
        let content = "def process(self, data: str, config: dict = None):";
        let params = builder.extract_parameters(content);
        assert!(params.is_some());
        let params_str = params.unwrap();
        assert!(params_str.contains("data"));
        assert!(params_str.contains("config"));
        assert!(!params_str.contains("self"));

        // Empty params
        let content = "def run():";
        assert!(builder.extract_parameters(content).is_none());
    }

    #[test]
    fn test_parent_context() {
        let graph = create_test_graph();
        let builder = SemanticTextBuilder::new(&graph);

        let method_node = graph.get_node("test.py:MyClass:process").unwrap();
        let parent_ctx = builder.build_parent_context(method_node);
        assert!(parent_ctx.is_some());
        assert!(parent_ctx.unwrap().contains("MyClass"));
    }

    #[test]
    fn test_full_build_output_format() {
        let graph = PetCodeGraph::new();
        let builder = SemanticTextBuilder::new(&graph);

        let node = Node {
            id: "test:MyMethod".to_string(),
            name: "MyMethod".to_string(),
            node_type: NodeType::Callable,
            kind: Some("method".to_string()),
            subtype: None,
            file: "src/Common/Utilities/test.cs".to_string(),
            line: 10,
            end_line: 10,
            text: None,
            metadata: NodeMetadata::default(),
            hash: None,
        };

        let content = "public void MyMethod()";
        let result = builder.build(&node, content);

        println!("\n=== Output format test ===");
        println!("Result: {}", result);
        println!("Contains period separators: {}", result.contains(". "));
        println!("Contains 'in file': {}", result.contains("in file"));
        println!("Contains 'code:': {}", result.contains("code:"));

        // Assert the format
        assert!(result.contains(". "), "Should use period separators");
        assert!(result.contains("in file"), "Should have 'in file' prefix");
    }

    // ========================================================================
    // UTF-8 Truncation Tests
    // ========================================================================

    #[test]
    fn test_find_utf8_truncation_point_ascii() {
        let s = "hello world";
        assert_eq!(find_utf8_truncation_point(s, 5), 5);
        assert_eq!(find_utf8_truncation_point(s, 100), 11);
        assert_eq!(find_utf8_truncation_point(s, 0), 0);
    }

    #[test]
    fn test_find_utf8_truncation_point_chinese() {
        // Each Chinese character is 3 bytes in UTF-8
        let s = "你好世界"; // 12 bytes total (4 chars * 3 bytes)
                            // char_indices: (0,'你'), (3,'好'), (6,'世'), (9,'界')
        assert_eq!(find_utf8_truncation_point(s, 3), 3); // First char (0+3=3) fits exactly
        assert_eq!(find_utf8_truncation_point(s, 4), 3); // 2nd char (3+3=6) doesn't fit in 4
        assert_eq!(find_utf8_truncation_point(s, 5), 3); // 2nd char (3+3=6) doesn't fit in 5
        assert_eq!(find_utf8_truncation_point(s, 6), 6); // 2nd char (3+3=6) fits exactly
        assert_eq!(find_utf8_truncation_point(s, 12), 12); // All chars fit
        assert_eq!(find_utf8_truncation_point(s, 2), 0); // Can't even fit first char (0+3=3 > 2)
    }

    #[test]
    fn test_find_utf8_truncation_point_emoji() {
        // Most emoji are 4 bytes in UTF-8
        let s = "🎉🎊🎁"; // 12 bytes total (3 emoji * 4 bytes)
                          // char_indices: (0,'🎉'), (4,'🎊'), (8,'🎁')
        assert_eq!(find_utf8_truncation_point(s, 4), 4); // First emoji (0+4=4) fits exactly
        assert_eq!(find_utf8_truncation_point(s, 5), 4); // 2nd emoji (4+4=8) doesn't fit in 5
        assert_eq!(find_utf8_truncation_point(s, 8), 8); // 2nd emoji (4+4=8) fits exactly
        assert_eq!(find_utf8_truncation_point(s, 3), 0); // Can't even fit first emoji (0+4=4 > 3)
    }

    #[test]
    fn test_find_utf8_truncation_point_mixed() {
        let s = "hello你好"; // 5 ASCII + 6 UTF-8 = 11 bytes
                             // char_indices: (0,'h'), (1,'e'), (2,'l'), (3,'l'), (4,'o'), (5,'你'), (8,'好')
        assert_eq!(find_utf8_truncation_point(s, 5), 5); // "hello" (last char at 4+1=5)
        assert_eq!(find_utf8_truncation_point(s, 8), 8); // "hello你" (5+3=8 fits exactly)
        assert_eq!(find_utf8_truncation_point(s, 7), 5); // 你 (5+3=8) doesn't fit in 7, stays at "hello"
        assert_eq!(find_utf8_truncation_point(s, 11), 11); // All fits
    }

    #[test]
    fn test_truncate_content_chinese() {
        let graph = PetCodeGraph::new();
        let builder = SemanticTextBuilder::new(&graph);

        // Create a string with Chinese characters near 300 byte boundary
        // Each Chinese char is 3 bytes, so 100 chars = 300 bytes
        let chinese_chars = "你好世界程序代码测试"; // 10 chars = 30 bytes
        let repeated = chinese_chars.repeat(11); // 110 chars = 330 bytes (exceeds 300)

        // This should not panic and should truncate
        let result = builder.truncate_content(&repeated, 300);
        assert!(!result.is_empty());
        // Verify it's valid UTF-8 (implicit - Rust strings are always valid UTF-8)
        assert!(result.ends_with("..."), "Should be truncated: {}", result);
    }

    #[test]
    fn test_truncate_content_arabic() {
        let graph = PetCodeGraph::new();
        let builder = SemanticTextBuilder::new(&graph);

        // Arabic characters (2-4 bytes each)
        let arabic = "مرحبا بالعالم"; // "Hello world" in Arabic
        let repeated = arabic.repeat(30);

        // This should not panic
        let result = builder.truncate_content(&repeated, 300);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_truncate_content_emoji() {
        let graph = PetCodeGraph::new();
        let builder = SemanticTextBuilder::new(&graph);

        // 4-byte emoji
        let emoji = "🎉🎊🎁🎈🎆🎇✨🌟💫⭐";
        let repeated = emoji.repeat(20);

        // This should not panic
        let result = builder.truncate_content(&repeated, 300);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_truncate_content_combining_chars() {
        let graph = PetCodeGraph::new();
        let builder = SemanticTextBuilder::new(&graph);

        // Combining characters: 'e' + combining acute accent = 'é'
        // The combining character (U+0301) is 2 bytes
        let combining = "e\u{0301}"; // é as two codepoints
        let repeated = combining.repeat(150); // ~450 bytes

        // This should not panic
        let result = builder.truncate_content(&repeated, 300);
        assert!(!result.is_empty());
    }

    #[test]
    fn test_truncate_content_edge_cases() {
        let graph = PetCodeGraph::new();
        let builder = SemanticTextBuilder::new(&graph);

        // Empty string
        assert_eq!(builder.truncate_content("", 300), "");

        // String shorter than max_len
        assert_eq!(builder.truncate_content("short", 300), "short");

        // String exactly at max_len (with spaces normalized)
        let exact = "a ".repeat(150); // 300 chars but will be normalized
        let result = builder.truncate_content(&exact, 300);
        assert!(!result.contains("...") || result.len() <= 303); // Either fits or truncated

        // String with only multi-byte characters
        let all_chinese = "中".repeat(100); // 300 bytes exactly
        let result = builder.truncate_content(&all_chinese, 300);
        // Should not panic and produce valid output
        assert!(!result.is_empty() || all_chinese.is_empty());
    }

    #[test]
    fn test_truncate_content_boundary_panic_case() {
        let graph = PetCodeGraph::new();
        let builder = SemanticTextBuilder::new(&graph);

        // This is the actual problematic case from the bug report:
        // Content with multi-byte character that would cross the 300 byte boundary
        let mut content = "a".repeat(299);
        content.push_str("中"); // 3-byte character at positions 299-301 (total 302 bytes)

        // This should NOT panic (it did before the fix)
        let result = builder.truncate_content(&content, 300);
        // The Chinese char (3 bytes) starting at 299 would end at 302, exceeding 300
        // So we should truncate before it
        assert!(result.ends_with("..."), "Should be truncated: {}", result);
        // The Chinese character should NOT be in the result since it doesn't fit
        assert!(
            !result.contains("中"),
            "Should not contain Chinese char: {}",
            result
        );
    }

    #[test]
    fn test_truncate_content_exact_boundary() {
        let graph = PetCodeGraph::new();
        let builder = SemanticTextBuilder::new(&graph);

        // Multi-byte char that fits exactly at boundary
        let mut content = "a".repeat(297);
        content.push_str("中"); // 3-byte character at positions 297-299 (total 300 bytes exactly)

        let result = builder.truncate_content(&content, 300);
        // The content is exactly 300 bytes, should not be truncated
        assert!(
            !result.ends_with("..."),
            "Should not be truncated: {}",
            result
        );
        assert!(
            result.contains("中"),
            "Should contain Chinese char: {}",
            result
        );
    }

    // ========================================================================
    // SemanticTextConfig Tests
    // ========================================================================

    #[test]
    fn test_config_full_default() {
        let config = SemanticTextConfig::full();
        assert!(config.include_parent_context);
        assert!(config.include_children_context);
        assert!(config.include_inheritance_context);
        assert!(config.include_references_context);
        assert_eq!(config.max_children, DEFAULT_MAX_CHILDREN);
        assert_eq!(config.max_references, DEFAULT_MAX_REFERENCES);
    }

    #[test]
    fn test_config_minimal() {
        let config = SemanticTextConfig::minimal();
        assert!(!config.include_parent_context);
        assert!(!config.include_children_context);
        assert!(!config.include_inheritance_context);
        assert!(!config.include_references_context);
        assert_eq!(config.max_children, 0);
        assert_eq!(config.max_references, 0);
    }

    #[test]
    fn test_config_streaming() {
        let config = SemanticTextConfig::streaming();
        // Streaming enables parent and references but not children/inheritance
        assert!(config.include_parent_context);
        assert!(!config.include_children_context);
        assert!(!config.include_inheritance_context);
        assert!(config.include_references_context);
        assert_eq!(config.max_children, 0);
        assert_eq!(config.max_references, 5);
    }

    #[test]
    fn test_minimal_config_skips_context() {
        let graph = create_test_graph();
        let builder = SemanticTextBuilder::new_with_config(&graph, SemanticTextConfig::minimal());

        let method_node = graph.get_node("test.py:MyClass:process").unwrap();
        let content = "def process(): pass";
        let result = builder.build(method_node, content);

        // Should NOT contain parent context (minimal mode skips it)
        assert!(
            !result.contains("in class MyClass"),
            "Minimal config should skip parent context: {}",
            result
        );
        // Should still have entity description and file context
        assert!(result.contains("method"), "Should have entity type");
        assert!(result.contains("process"), "Should have entity name");
        assert!(result.contains("in file"), "Should have file context");
    }

    #[test]
    fn test_full_config_includes_context() {
        let graph = create_test_graph();
        let builder = SemanticTextBuilder::new_with_config(&graph, SemanticTextConfig::full());

        let method_node = graph.get_node("test.py:MyClass:process").unwrap();
        let content = "def process(): pass";
        let result = builder.build(method_node, content);

        // Full config should include parent context
        assert!(
            result.contains("in class MyClass"),
            "Full config should include parent context: {}",
            result
        );
    }
}
