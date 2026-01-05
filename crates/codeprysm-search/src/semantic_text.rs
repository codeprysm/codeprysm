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
//! ## Example Output
//!
//! For a method:
//! ```text
//! public async method processRequest(data, config) in class RequestHandler
//! in file handlers.py. calls validate, transform. uses HttpResponse, Logger.
//! handles HTTP requests, error handling, validation
//! ```

use std::collections::HashSet;

use codeprysm_core::{EdgeType, Node, NodeType, PetCodeGraph};

/// Maximum number of children to include in description
const MAX_CHILDREN: usize = 5;
/// Maximum number of references to include
const MAX_REFERENCES: usize = 5;
/// Maximum content preview length
const MAX_CONTENT_PREVIEW: usize = 300;

/// Builder for creating semantic text descriptions of code entities.
///
/// Uses graph traversal to build rich context for better semantic search.
pub struct SemanticTextBuilder<'a> {
    graph: &'a PetCodeGraph,
}

impl<'a> SemanticTextBuilder<'a> {
    /// Create a new semantic text builder with access to the full graph.
    pub fn new(graph: &'a PetCodeGraph) -> Self {
        Self { graph }
    }

    /// Build semantic text for a node.
    ///
    /// The text is structured for optimal embedding:
    /// 1. Entity type and name with modifiers
    /// 2. Inheritance/implementation info (for containers)
    /// 3. Parameters (for callables)
    /// 4. Children context (for containers)
    /// 5. Parent context (containing class/module)
    /// 6. File context
    /// 7. References (calls, uses)
    /// 8. Semantic keywords
    /// 9. Code preview
    pub fn build(&self, node: &Node, content: &str) -> String {
        let mut parts = Vec::new();

        // 1. Build entity description with modifiers
        parts.push(self.build_entity_description(node));

        // 2. Add inheritance info for containers
        if node.node_type == NodeType::Container {
            if let Some(inheritance) = self.build_inheritance_context(node) {
                parts.push(inheritance);
            }
        }

        // 3. Add parameters for callables (extracted from content)
        if node.node_type == NodeType::Callable {
            if let Some(params) = self.extract_parameters(content) {
                parts.push(format!("({})", params));
            }
        }

        // 4. Add children context for containers
        if node.node_type == NodeType::Container && !node.is_file() {
            if let Some(children_ctx) = self.build_children_context(node) {
                parts.push(children_ctx);
            }
        }

        // 5. Add parent context
        if let Some(parent_ctx) = self.build_parent_context(node) {
            parts.push(parent_ctx);
        }

        // 6. Add file context
        parts.push(format!("in file {}", self.format_file_path(&node.file)));

        // 7. Add references context
        if let Some(refs_ctx) = self.build_references_context(node) {
            parts.push(refs_ctx);
        }

        // 8. Add semantic keywords based on patterns
        let keywords = self.extract_semantic_keywords(node, content);
        if !keywords.is_empty() {
            parts.push(format!("related to: {}", keywords.join(", ")));
        }

        // 9. Add content preview (truncated)
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
        for (target, edge_data) in self.graph.outgoing_edges(&node.id) {
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
    fn build_children_context(&self, node: &Node) -> Option<String> {
        let mut methods = Vec::new();
        let mut fields = Vec::new();
        let mut properties = Vec::new();

        for child in self.graph.children(&node.id) {
            match child.node_type {
                NodeType::Callable => {
                    if methods.len() < MAX_CHILDREN {
                        methods.push(child.name.clone());
                    }
                }
                NodeType::Data => {
                    let kind = child.kind.as_deref().unwrap_or("");
                    if kind == "property" {
                        if properties.len() < MAX_CHILDREN {
                            properties.push(child.name.clone());
                        }
                    } else if fields.len() < MAX_CHILDREN {
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

        if let Some(parent) = self.graph.parent(&node.id) {
            // Skip if parent is just a file
            if parent.is_file() {
                return None;
            }

            let parent_type = self.get_type_descriptor(parent);
            Some(format!("in {} {}", parent_type, parent.name))
        } else {
            None
        }
    }

    /// Build references context.
    ///
    /// Lists what the entity calls/uses.
    fn build_references_context(&self, node: &Node) -> Option<String> {
        let mut calls = Vec::new();
        let mut uses_types = Vec::new();
        let mut uses_data = Vec::new();

        for (target, edge_data) in self.graph.outgoing_edges(&node.id) {
            if edge_data.edge_type == EdgeType::Uses {
                match target.node_type {
                    NodeType::Callable => {
                        if calls.len() < MAX_REFERENCES {
                            calls.push(target.name.clone());
                        }
                    }
                    NodeType::Container if !target.is_file() => {
                        if uses_types.len() < MAX_REFERENCES {
                            uses_types.push(target.name.clone());
                        }
                    }
                    NodeType::Data => {
                        if uses_data.len() < MAX_REFERENCES {
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
    fn truncate_content(&self, content: &str, max_len: usize) -> String {
        // Clean content - normalize whitespace
        let cleaned: String = content.split_whitespace().collect::<Vec<&str>>().join(" ");

        if cleaned.len() <= max_len {
            cleaned
        } else {
            // Find a good break point
            let truncated = &cleaned[..max_len];
            if let Some(last_space) = truncated.rfind(' ') {
                format!("{}...", &truncated[..last_space])
            } else {
                format!("{}...", truncated)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use codeprysm_core::{EdgeData, NodeMetadata};

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
}
