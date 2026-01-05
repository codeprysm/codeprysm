//! Tree-Sitter Parser for Code Graph Generation
//!
//! This module provides tree-sitter based parsing for extracting code entities
//! and their relationships from source files.
//!
//! ## Supported Languages
//!
//! - Python (.py)
//! - JavaScript (.js, .mjs, .cjs)
//! - TypeScript (.ts, .tsx)
//! - Rust (.rs)
//! - Go (.go)
//! - C (.c, .h)
//! - C++ (.cpp, .hpp, .cc, .cxx)
//! - C# (.cs)

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use thiserror::Error;
use tree_sitter::{Language, Parser, Query, QueryCursor, StreamingIterator, Tree};

// ============================================================================
// Supported Languages
// ============================================================================

/// Supported programming languages for parsing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SupportedLanguage {
    Python,
    JavaScript,
    TypeScript,
    Tsx,
    Rust,
    Go,
    C,
    Cpp,
    CSharp,
}

impl SupportedLanguage {
    /// Get the language name as used in SCM query file names.
    pub fn as_str(&self) -> &'static str {
        match self {
            SupportedLanguage::Python => "python",
            SupportedLanguage::JavaScript => "javascript",
            SupportedLanguage::TypeScript => "typescript",
            SupportedLanguage::Tsx => "typescript", // TSX uses TypeScript queries
            SupportedLanguage::Rust => "rust",
            SupportedLanguage::Go => "go",
            SupportedLanguage::C => "c",
            SupportedLanguage::Cpp => "cpp",
            SupportedLanguage::CSharp => "csharp",
        }
    }

    /// Get the tree-sitter Language for this language.
    pub fn tree_sitter_language(&self) -> Language {
        match self {
            SupportedLanguage::Python => tree_sitter_python::LANGUAGE.into(),
            SupportedLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            SupportedLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            SupportedLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            SupportedLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
            SupportedLanguage::Go => tree_sitter_go::LANGUAGE.into(),
            SupportedLanguage::C => tree_sitter_c::LANGUAGE.into(),
            SupportedLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
            SupportedLanguage::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        }
    }

    /// Detect language from file extension.
    ///
    /// Returns `None` if the extension is not recognized.
    pub fn from_extension(ext: &str) -> Option<Self> {
        get_extension_map()
            .get(ext.to_lowercase().as_str())
            .copied()
    }

    /// Detect language from file path.
    ///
    /// Returns `None` if the file extension is not recognized.
    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|e| e.to_str())
            .and_then(Self::from_extension)
    }

    /// Get all supported file extensions.
    pub fn all_extensions() -> &'static [&'static str] {
        &[
            "py", "js", "mjs", "cjs", "ts", "tsx", "rs", "go", "c", "h", "cpp", "hpp", "cc", "cxx",
            "cs",
        ]
    }
}

impl std::fmt::Display for SupportedLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Static extension to language mapping.
static EXTENSION_MAP: OnceLock<HashMap<&'static str, SupportedLanguage>> = OnceLock::new();

fn get_extension_map() -> &'static HashMap<&'static str, SupportedLanguage> {
    EXTENSION_MAP.get_or_init(|| {
        let mut map = HashMap::new();
        // Python
        map.insert("py", SupportedLanguage::Python);
        // JavaScript
        map.insert("js", SupportedLanguage::JavaScript);
        map.insert("mjs", SupportedLanguage::JavaScript);
        map.insert("cjs", SupportedLanguage::JavaScript);
        // TypeScript
        map.insert("ts", SupportedLanguage::TypeScript);
        map.insert("tsx", SupportedLanguage::Tsx);
        // Rust
        map.insert("rs", SupportedLanguage::Rust);
        // Go
        map.insert("go", SupportedLanguage::Go);
        // C
        map.insert("c", SupportedLanguage::C);
        map.insert("h", SupportedLanguage::C);
        // C++
        map.insert("cpp", SupportedLanguage::Cpp);
        map.insert("hpp", SupportedLanguage::Cpp);
        map.insert("cc", SupportedLanguage::Cpp);
        map.insert("cxx", SupportedLanguage::Cpp);
        // C#
        map.insert("cs", SupportedLanguage::CSharp);
        map
    })
}

// Initialize extension map on module load
#[doc(hidden)]
pub fn _init_extension_map() {
    let _ = get_extension_map();
}

// ============================================================================
// Manifest Languages
// ============================================================================

/// Manifest file languages for component extraction.
///
/// These are distinct from `SupportedLanguage` because manifest files
/// have different grammars (JSON, TOML, XML, etc.) and use specialized
/// SCM queries focused on extracting component names and dependencies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ManifestLanguage {
    /// JSON manifests (package.json, vcpkg.json)
    Json,
    /// TOML manifests (Cargo.toml, pyproject.toml)
    Toml,
    /// Go module files (go.mod)
    GoMod,
    /// XML manifests (.csproj, .vbproj, .fsproj)
    Xml,
    /// CMake files (CMakeLists.txt)
    CMake,
}

impl ManifestLanguage {
    /// Get the language name as used in SCM query file names.
    ///
    /// Maps to `{name}-manifest-tags.scm` query files.
    pub fn as_str(&self) -> &'static str {
        match self {
            ManifestLanguage::Json => "json",
            ManifestLanguage::Toml => "toml",
            ManifestLanguage::GoMod => "gomod",
            ManifestLanguage::Xml => "xml",
            ManifestLanguage::CMake => "cmake",
        }
    }

    /// Get the tree-sitter Language for this manifest type.
    pub fn tree_sitter_language(&self) -> Language {
        match self {
            ManifestLanguage::Json => tree_sitter_json::LANGUAGE.into(),
            ManifestLanguage::Toml => tree_sitter_toml_ng::LANGUAGE.into(),
            ManifestLanguage::GoMod => tree_sitter_gomod_orchard::LANGUAGE.into(),
            ManifestLanguage::Xml => tree_sitter_xml::LANGUAGE_XML.into(),
            ManifestLanguage::CMake => tree_sitter_cmake::LANGUAGE.into(),
        }
    }

    /// Detect manifest language from filename.
    ///
    /// Returns `None` if the filename is not a recognized manifest file.
    ///
    /// # Recognized Manifest Files
    ///
    /// | Filename Pattern | Language | Component Type |
    /// |-----------------|----------|----------------|
    /// | `package.json` | Json | npm/Node.js |
    /// | `vcpkg.json` | Json | vcpkg (C/C++) |
    /// | `Cargo.toml` | Toml | Rust crate |
    /// | `pyproject.toml` | Toml | Python package |
    /// | `go.mod` | GoMod | Go module |
    /// | `*.csproj` | Xml | C# project |
    /// | `*.vbproj` | Xml | VB.NET project |
    /// | `*.fsproj` | Xml | F# project |
    /// | `CMakeLists.txt` | CMake | CMake project |
    pub fn from_filename(filename: &str) -> Option<Self> {
        match filename {
            // JSON manifests
            "package.json" => Some(ManifestLanguage::Json),
            "vcpkg.json" => Some(ManifestLanguage::Json),
            // TOML manifests
            "Cargo.toml" => Some(ManifestLanguage::Toml),
            "pyproject.toml" => Some(ManifestLanguage::Toml),
            // Go module
            "go.mod" => Some(ManifestLanguage::GoMod),
            // CMake
            "CMakeLists.txt" => Some(ManifestLanguage::CMake),
            // XML-based project files
            _ => {
                if filename.ends_with(".csproj")
                    || filename.ends_with(".vbproj")
                    || filename.ends_with(".fsproj")
                {
                    Some(ManifestLanguage::Xml)
                } else {
                    None
                }
            }
        }
    }

    /// Detect manifest language from file path.
    ///
    /// Extracts the filename from the path and calls `from_filename`.
    pub fn from_path(path: &Path) -> Option<Self> {
        path.file_name()
            .and_then(|n| n.to_str())
            .and_then(Self::from_filename)
    }

    /// Check if a file path is a manifest file.
    pub fn is_manifest_file(path: &Path) -> bool {
        Self::from_path(path).is_some()
    }

    /// Get all recognized manifest filenames (exact matches).
    pub fn exact_filenames() -> &'static [&'static str] {
        &[
            "package.json",
            "vcpkg.json",
            "Cargo.toml",
            "pyproject.toml",
            "go.mod",
            "CMakeLists.txt",
        ]
    }

    /// Get all recognized manifest file extensions.
    pub fn manifest_extensions() -> &'static [&'static str] {
        &["csproj", "vbproj", "fsproj"]
    }
}

impl std::fmt::Display for ManifestLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Parser Errors
// ============================================================================

/// Errors that can occur during parsing.
#[derive(Debug, Error)]
pub enum ParserError {
    /// Failed to create parser
    #[error("Failed to create parser: {0}")]
    ParserCreation(String),

    /// Failed to set language
    #[error("Failed to set language: {0}")]
    LanguageSet(String),

    /// Failed to parse source code
    #[error("Failed to parse source code")]
    ParseFailed,

    /// Failed to load query file
    #[error("Failed to load query file: {0}")]
    QueryLoad(String),

    /// Failed to compile query
    #[error("Failed to compile query: {0}")]
    QueryCompile(String),

    /// Unsupported language
    #[error("Unsupported language for file: {0}")]
    UnsupportedLanguage(String),

    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}

// ============================================================================
// Code Parser
// ============================================================================

/// A tree-sitter based code parser with query support.
pub struct CodeParser {
    parser: Parser,
    language: SupportedLanguage,
}

impl CodeParser {
    /// Create a new parser for the specified language.
    pub fn new(language: SupportedLanguage) -> Result<Self, ParserError> {
        let mut parser = Parser::new();
        parser
            .set_language(&language.tree_sitter_language())
            .map_err(|e| ParserError::LanguageSet(e.to_string()))?;

        Ok(Self { parser, language })
    }

    /// Create a parser for the given file path.
    ///
    /// Detects language from file extension.
    pub fn for_path(path: &Path) -> Result<Self, ParserError> {
        let language = SupportedLanguage::from_path(path)
            .ok_or_else(|| ParserError::UnsupportedLanguage(path.display().to_string()))?;
        Self::new(language)
    }

    /// Get the language this parser is configured for.
    pub fn language(&self) -> SupportedLanguage {
        self.language
    }

    /// Parse source code into a syntax tree.
    pub fn parse(&mut self, source: &str) -> Result<Tree, ParserError> {
        self.parser
            .parse(source, None)
            .ok_or(ParserError::ParseFailed)
    }

    /// Parse source code with an existing tree for incremental parsing.
    pub fn parse_with_old_tree(
        &mut self,
        source: &str,
        old_tree: Option<&Tree>,
    ) -> Result<Tree, ParserError> {
        self.parser
            .parse(source, old_tree)
            .ok_or(ParserError::ParseFailed)
    }
}

// ============================================================================
// Query Manager
// ============================================================================

/// Manages tree-sitter queries for extracting code entities.
pub struct QueryManager {
    /// Base query for tag extraction
    query: Query,
    /// Language this query is for
    language: SupportedLanguage,
}

impl QueryManager {
    /// Create a new query manager from SCM query source.
    pub fn new(language: SupportedLanguage, query_source: &str) -> Result<Self, ParserError> {
        let ts_language = language.tree_sitter_language();
        let query = Query::new(&ts_language, query_source)
            .map_err(|e| ParserError::QueryCompile(format!("{:?}", e)))?;

        Ok(Self { query, language })
    }

    /// Load query from a file path.
    pub fn from_file(language: SupportedLanguage, path: &Path) -> Result<Self, ParserError> {
        let query_source = std::fs::read_to_string(path)?;
        Self::new(language, &query_source)
    }

    /// Load query from the default queries directory.
    ///
    /// Looks for `{language}-tags.scm` in the queries directory and
    /// concatenates any overlay files from `overlays/{language}-*.scm`.
    pub fn from_queries_dir(
        language: SupportedLanguage,
        queries_dir: &Path,
    ) -> Result<Self, ParserError> {
        let lang_str = language.as_str();
        let base_path = queries_dir.join(format!("{}-tags.scm", lang_str));

        if !base_path.exists() {
            return Err(ParserError::QueryLoad(format!(
                "Query file not found: {}",
                base_path.display()
            )));
        }

        // Load base query
        let mut query_source = std::fs::read_to_string(&base_path)?;

        // Load and concatenate overlay files
        let overlays_dir = queries_dir.join("overlays");
        if overlays_dir.exists() {
            if let Ok(entries) = std::fs::read_dir(&overlays_dir) {
                let mut overlay_files: Vec<_> = entries
                    .filter_map(|e| e.ok())
                    .filter(|e| {
                        e.file_name().to_str().is_some_and(|name| {
                            name.starts_with(&format!("{}-", lang_str)) && name.ends_with(".scm")
                        })
                    })
                    .collect();

                // Sort for deterministic ordering
                overlay_files.sort_by_key(|e| e.file_name());

                for entry in overlay_files {
                    if let Ok(overlay_source) = std::fs::read_to_string(entry.path()) {
                        query_source.push_str("\n\n");
                        query_source.push_str(&overlay_source);
                    }
                }
            }
        }

        Self::new(language, &query_source)
    }

    /// Load query from embedded queries (compiled into the binary).
    ///
    /// This is the preferred method for production use as it doesn't require
    /// external query files.
    pub fn from_embedded(language: SupportedLanguage) -> Result<Self, ParserError> {
        let query_source = crate::embedded_queries::get_query(language).ok_or_else(|| {
            ParserError::QueryLoad(format!(
                "No embedded query available for language: {:?}",
                language
            ))
        })?;
        Self::new(language, &query_source)
    }

    /// Get the underlying tree-sitter query.
    pub fn query(&self) -> &Query {
        &self.query
    }

    /// Get the language this query is for.
    pub fn language(&self) -> SupportedLanguage {
        self.language
    }

    /// Get the capture names defined in this query.
    pub fn capture_names(&self) -> &[&str] {
        self.query.capture_names()
    }

    /// Get the capture index for a capture name.
    pub fn capture_index(&self, name: &str) -> Option<u32> {
        self.query.capture_index_for_name(name)
    }
}

// ============================================================================
// Extracted Tag
// ============================================================================

/// A tag extracted from source code via tree-sitter query.
#[derive(Debug, Clone)]
pub struct ExtractedTag {
    /// The tag name (capture name from query, e.g., "definition.callable.function")
    pub tag: String,
    /// The captured text (entity name)
    pub name: String,
    /// Start line (0-indexed)
    pub start_line: usize,
    /// End line (0-indexed)
    pub end_line: usize,
    /// Start column (0-indexed)
    pub start_col: usize,
    /// End column (0-indexed)
    pub end_col: usize,
    /// Start byte offset
    pub start_byte: usize,
    /// End byte offset
    pub end_byte: usize,
    /// Parent node's start line (for containment tracking with name. captures)
    pub parent_start_line: Option<usize>,
    /// Parent node's end line (for containment tracking with name. captures)
    pub parent_end_line: Option<usize>,
    /// For Rust: The type being implemented (from impl blocks)
    /// This allows methods in `impl Foo { }` to be associated with struct Foo
    pub impl_target: Option<String>,
}

impl ExtractedTag {
    /// Get the line number (1-indexed) for display.
    pub fn line_number(&self) -> usize {
        self.start_line + 1
    }

    /// Get the end line number (1-indexed) for display.
    pub fn end_line_number(&self) -> usize {
        self.end_line + 1
    }

    /// Get the containment start line (0-indexed).
    /// Uses parent line if available, otherwise falls back to tag's own line.
    pub fn containment_start_line(&self) -> usize {
        self.parent_start_line.unwrap_or(self.start_line)
    }

    /// Get the containment end line (0-indexed).
    /// Uses parent line if available, otherwise falls back to tag's own line.
    pub fn containment_end_line(&self) -> usize {
        self.parent_end_line.unwrap_or(self.end_line)
    }
}

// ============================================================================
// Containment Context
// ============================================================================

/// Entry in the containment stack.
#[derive(Debug, Clone)]
pub struct ContainmentEntry {
    /// Full node ID of the container
    pub node_id: String,
    /// Type of the container ("Container" or "Callable")
    pub node_type: String,
    /// Starting line (0-indexed)
    pub start_line: usize,
    /// Ending line (0-indexed)
    pub end_line: usize,
    /// Name of the container entity
    pub entity_name: String,
}

/// Tracks containment context during graph generation.
///
/// Maintains a stack of currently open containers based on line ranges,
/// enabling determination of parent-child relationships for nested entities.
///
/// # Example
///
/// ```
/// use codeprysm_core::parser::ContainmentContext;
///
/// let mut ctx = ContainmentContext::new();
///
/// // Processing a class definition at lines 10-50
/// ctx.push_container("file.py:MyClass".to_string(), "Container".to_string(), 10, 50, "MyClass".to_string());
///
/// // Inside the class, push a method at lines 15-25
/// ctx.update(15);
/// ctx.push_container("file.py:MyClass:method".to_string(), "Callable".to_string(), 15, 25, "method".to_string());
///
/// // Get the containment path
/// assert_eq!(ctx.get_containment_path(), vec!["MyClass", "method"]);
///
/// // After the method ends, update pops it
/// ctx.update(30);
/// assert_eq!(ctx.get_containment_path(), vec!["MyClass"]);
/// ```
#[derive(Debug, Default)]
pub struct ContainmentContext {
    /// Stack of active containers
    stack: Vec<ContainmentEntry>,
}

impl ContainmentContext {
    /// Create a new empty containment context.
    pub fn new() -> Self {
        Self { stack: Vec::new() }
    }

    /// Update the stack by popping containers that have ended.
    ///
    /// Call this before processing each new entity to ensure the stack
    /// reflects the current position in the source file.
    pub fn update(&mut self, current_line: usize) {
        // Pop containers whose end_line we've passed
        while let Some(entry) = self.stack.last() {
            if entry.end_line < current_line {
                self.stack.pop();
            } else {
                break;
            }
        }
    }

    /// Push a new container onto the stack.
    ///
    /// Only Container and Callable types can contain other entities.
    pub fn push_container(
        &mut self,
        node_id: String,
        node_type: String,
        start_line: usize,
        end_line: usize,
        entity_name: String,
    ) {
        // Containers and Callables can contain other entities
        if node_type == "Container" || node_type == "Callable" {
            self.stack.push(ContainmentEntry {
                node_id,
                node_type,
                start_line,
                end_line,
                entity_name,
            });
        }
    }

    /// Get the node ID of the current innermost container.
    ///
    /// Returns `None` if at file level (no active containers).
    pub fn get_current_parent_id(&self) -> Option<&str> {
        self.stack.last().map(|e| e.node_id.as_str())
    }

    /// Get the full containment path as a list of entity names.
    ///
    /// Returns the path from outermost to innermost container.
    pub fn get_containment_path(&self) -> Vec<&str> {
        self.stack.iter().map(|e| e.entity_name.as_str()).collect()
    }

    /// Check if a container is currently active (on the stack).
    pub fn is_container_active(&self, node_id: &str) -> bool {
        self.stack.iter().any(|e| e.node_id == node_id)
    }

    /// Get the current stack depth.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Check if the stack is empty (at file level).
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Clear the containment stack (typically called between files).
    pub fn clear(&mut self) {
        self.stack.clear();
    }

    /// Get a reference to the current stack.
    pub fn stack(&self) -> &[ContainmentEntry] {
        &self.stack
    }
}

impl std::fmt::Display for ContainmentContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.stack.is_empty() {
            write!(f, "ContainmentContext(empty)")
        } else {
            let path: Vec<String> = self
                .stack
                .iter()
                .map(|e| format!("{}({})", e.entity_name, e.node_type))
                .collect();
            write!(f, "ContainmentContext({})", path.join(" → "))
        }
    }
}

// ============================================================================
// Node ID Generation
// ============================================================================

/// Generate a hierarchical node ID for an entity.
///
/// Node IDs follow the format: `file:Container1:Container2:Entity`
///
/// # Arguments
///
/// * `file_path` - Relative file path (e.g., "src/models.py")
/// * `containment_stack` - Stack of parent entity names
/// * `entity_name` - Name of the entity
/// * `line` - Line number for anonymous entities (e.g., lambdas)
///
/// # Examples
///
/// ```
/// use codeprysm_core::parser::generate_node_id;
///
/// assert_eq!(
///     generate_node_id("src/models.py", &["User"], "save", None),
///     "src/models.py:User:save"
/// );
/// assert_eq!(
///     generate_node_id("src/utils.py", &["process"], "<lambda>", Some(42)),
///     "src/utils.py:process:<lambda>:42"
/// );
/// ```
pub fn generate_node_id(
    file_path: &str,
    containment_stack: &[&str],
    entity_name: &str,
    line: Option<usize>,
) -> String {
    let mut components = vec![file_path];
    components.extend(containment_stack);

    // Handle anonymous entities with line numbers
    if entity_name.starts_with('<') && entity_name.ends_with('>') {
        if let Some(line_num) = line {
            components.push(entity_name);
            return format!("{}:{}", components.join(":"), line_num);
        }
    }

    components.push(entity_name);
    components.join(":")
}

/// Parse a node ID into its components.
///
/// Returns the file path, containment stack, and entity name.
///
/// # Returns
///
/// Tuple of (file_path, containment_stack, entity_name)
///
/// # Errors
///
/// Returns `None` if the node ID format is invalid.
pub fn parse_node_id(node_id: &str) -> Option<(&str, Vec<&str>, &str)> {
    let parts: Vec<&str> = node_id.split(':').collect();
    if parts.len() < 2 {
        return None;
    }

    let file_path = parts[0];
    let entity_name = parts[parts.len() - 1];
    let containment = parts[1..parts.len() - 1].to_vec();

    Some((file_path, containment, entity_name))
}

// ============================================================================
// Tag Extractor
// ============================================================================

/// Extracts tags from source code using tree-sitter queries.
pub struct TagExtractor {
    parser: CodeParser,
    query_manager: QueryManager,
}

impl TagExtractor {
    /// Create a new tag extractor for the specified language.
    pub fn new(language: SupportedLanguage, query_source: &str) -> Result<Self, ParserError> {
        let parser = CodeParser::new(language)?;
        let query_manager = QueryManager::new(language, query_source)?;
        Ok(Self {
            parser,
            query_manager,
        })
    }

    /// Create a tag extractor using queries from a directory.
    pub fn from_queries_dir(
        language: SupportedLanguage,
        queries_dir: &Path,
    ) -> Result<Self, ParserError> {
        let parser = CodeParser::new(language)?;
        let query_manager = QueryManager::from_queries_dir(language, queries_dir)?;
        Ok(Self {
            parser,
            query_manager,
        })
    }

    /// Create a tag extractor using embedded queries.
    ///
    /// This is the preferred method for production use as it doesn't require
    /// external query files.
    pub fn from_embedded(language: SupportedLanguage) -> Result<Self, ParserError> {
        let parser = CodeParser::new(language)?;
        let query_manager = QueryManager::from_embedded(language)?;
        Ok(Self {
            parser,
            query_manager,
        })
    }

    /// Get the language this extractor is configured for.
    pub fn language(&self) -> SupportedLanguage {
        self.parser.language()
    }

    /// Extract tags from source code.
    pub fn extract(&mut self, source: &str) -> Result<Vec<ExtractedTag>, ParserError> {
        let tree = self.parser.parse(source)?;
        let source_bytes = source.as_bytes();
        let is_rust = self.parser.language() == SupportedLanguage::Rust;

        let mut tags = Vec::new();
        let mut cursor = QueryCursor::new();
        let query = self.query_manager.query();
        let capture_names = query.capture_names();

        let mut matches = cursor.matches(query, tree.root_node(), source_bytes);
        while let Some(match_) = matches.next() {
            for capture in match_.captures {
                let capture_name = &capture_names[capture.index as usize];
                let node = capture.node;

                // Get the text of the captured node
                let text = node.utf8_text(source_bytes).unwrap_or("").to_string();

                // For name. captures (e.g., @name.definition.X), get the parent node's
                // line range for proper containment tracking. The parent is the actual
                // definition node (class_definition, function_definition, etc.).
                let (parent_start_line, parent_end_line) = if capture_name.starts_with("name.") {
                    if let Some(parent) = node.parent() {
                        (
                            Some(parent.start_position().row),
                            Some(parent.end_position().row),
                        )
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };

                // For Rust methods inside impl blocks, find the impl target type
                let impl_target = if is_rust && capture_name.contains("callable.method") {
                    find_impl_target(&node, source_bytes)
                } else {
                    None
                };

                tags.push(ExtractedTag {
                    tag: (*capture_name).to_string(),
                    name: text,
                    start_line: node.start_position().row,
                    end_line: node.end_position().row,
                    start_col: node.start_position().column,
                    end_col: node.end_position().column,
                    start_byte: node.start_byte(),
                    end_byte: node.end_byte(),
                    parent_start_line,
                    parent_end_line,
                    impl_target,
                });
            }
        }

        Ok(tags)
    }
}

/// Find the impl target type for a node inside a Rust impl block.
/// Traverses up the AST to find an impl_item and extracts its type identifier.
///
/// For `impl Trait for Type`, returns "Type" (not "Trait").
/// For `impl Type`, returns "Type".
fn find_impl_target(node: &tree_sitter::Node, source: &[u8]) -> Option<String> {
    let mut current = node.parent();
    while let Some(parent) = current {
        if parent.kind() == "impl_item" {
            // FIRST: Try to get the "type" field specifically
            // This correctly handles `impl Trait for Type` (returns Type, not Trait)
            if let Some(type_child) = parent.child_by_field_name("type") {
                // Handle generic_type: `impl<T> Foo<T>` or `impl Trait for Foo<T>`
                if type_child.kind() == "generic_type" {
                    for child in type_child.children(&mut type_child.walk()) {
                        if child.kind() == "type_identifier" {
                            if let Ok(type_name) = child.utf8_text(source) {
                                return Some(type_name.to_string());
                            }
                        }
                    }
                } else if let Ok(type_name) = type_child.utf8_text(source) {
                    return Some(type_name.to_string());
                }
            }

            // FALLBACK: For simple `impl Type` without trait, find first type_identifier
            // This is only reached if child_by_field_name("type") didn't find anything
            for child in parent.children(&mut parent.walk()) {
                if child.kind() == "type_identifier" {
                    if let Ok(type_name) = child.utf8_text(source) {
                        return Some(type_name.to_string());
                    }
                }
            }
            break;
        }
        current = parent.parent();
    }
    None
}

// ============================================================================
// Metadata Extraction
// ============================================================================

use crate::graph::NodeMetadata;
use tree_sitter::Node;

/// Extracts metadata from AST nodes for a specific language.
pub struct MetadataExtractor {
    language: SupportedLanguage,
}

impl MetadataExtractor {
    /// Create a new metadata extractor for the specified language.
    pub fn new(language: SupportedLanguage) -> Self {
        Self { language }
    }

    /// Extract metadata from an AST node and its text.
    ///
    /// # Arguments
    ///
    /// * `node` - The tree-sitter AST node
    /// * `source` - The source code bytes
    ///
    /// # Returns
    ///
    /// A `NodeMetadata` struct with extracted metadata.
    pub fn extract(&self, node: &Node, source: &[u8]) -> NodeMetadata {
        let node_text = node.utf8_text(source).unwrap_or("");

        match self.language {
            SupportedLanguage::Python => extract_python_metadata(node, node_text, source),
            SupportedLanguage::JavaScript
            | SupportedLanguage::TypeScript
            | SupportedLanguage::Tsx => extract_typescript_metadata(node, node_text, source),
            SupportedLanguage::Go => extract_go_metadata(node, source),
            SupportedLanguage::CSharp => extract_csharp_metadata(node, node_text, source),
            SupportedLanguage::Rust => extract_rust_metadata(node, node_text, source),
            SupportedLanguage::C | SupportedLanguage::Cpp => {
                extract_c_cpp_metadata(node, node_text, source)
            }
        }
    }

    /// Extract metadata from an entity name (for convention-based visibility).
    ///
    /// Some languages like Python and Go use naming conventions for visibility.
    /// This method extracts metadata from the entity name alone.
    pub fn extract_from_name(&self, name: &str) -> NodeMetadata {
        let mut metadata = NodeMetadata::default();

        match self.language {
            SupportedLanguage::Python => {
                // Python visibility by naming convention
                // Dunder methods (__init__, __str__, etc.) are public
                // Double underscore without trailing __ is private (name mangling)
                // Single underscore is protected (convention)
                if name.starts_with("__") && !name.ends_with("__") {
                    metadata.visibility = Some("private".to_string());
                } else if name.starts_with('_') && !name.starts_with("__") {
                    metadata.visibility = Some("protected".to_string());
                } else {
                    metadata.visibility = Some("public".to_string());
                }
            }
            SupportedLanguage::Go => {
                // Go: uppercase first letter = exported (public)
                if let Some(first_char) = name.chars().next() {
                    if first_char.is_uppercase() {
                        metadata.visibility = Some("public".to_string());
                    } else {
                        metadata.visibility = Some("private".to_string());
                    }
                }
            }
            _ => {}
        }

        metadata
    }
}

/// Extract Python-specific metadata.
fn extract_python_metadata(node: &Node, node_text: &str, source: &[u8]) -> NodeMetadata {
    let mut metadata = NodeMetadata::default();

    // Check for async keyword
    if node_text.contains("async def") || node_text.contains("async with") {
        metadata.is_async = Some(true);
    }

    // Also check for async child node (more reliable)
    if has_child_of_kind(node, "async") {
        metadata.is_async = Some(true);
    }

    // Python visibility by naming convention
    // (handled by extract_from_name, but check node text too)
    if let Some(name) = find_identifier_text(node, source) {
        // Dunder methods (__init__, __str__, etc.) are public
        // Double underscore without trailing __ is private (name mangling)
        // Single underscore is protected (convention)
        if name.starts_with("__") && !name.ends_with("__") {
            metadata.visibility = Some("private".to_string());
        } else if name.starts_with('_') && !name.starts_with("__") {
            metadata.visibility = Some("protected".to_string());
        } else {
            metadata.visibility = Some("public".to_string());
        }
    }

    // Extract decorators
    let decorators = extract_decorators(node, source, "@");
    if !decorators.is_empty() {
        metadata.decorators = Some(decorators);
    }

    metadata
}

/// Extract TypeScript/JavaScript-specific metadata.
fn extract_typescript_metadata(node: &Node, node_text: &str, source: &[u8]) -> NodeMetadata {
    let mut metadata = NodeMetadata::default();

    // Check for async by examining child nodes
    let node_kind = node.kind();

    if node_kind == "variable_declarator" || node_kind == "lexical_declaration" {
        // For arrow functions, look for arrow_function child with async
        if let Some(arrow_fn) = find_child_of_kind(node, "arrow_function") {
            if has_child_of_kind(&arrow_fn, "async") {
                metadata.is_async = Some(true);
            }
        }
    } else {
        // For other nodes (method_definition, function_declaration), check direct children
        if has_child_of_kind(node, "async") {
            metadata.is_async = Some(true);
        }
    }

    // Extract visibility from modifiers
    if node_text.contains("private ") {
        metadata.visibility = Some("private".to_string());
    } else if node_text.contains("protected ") {
        metadata.visibility = Some("protected".to_string());
    } else if node_text.contains("public ") {
        metadata.visibility = Some("public".to_string());
    }

    // Check for static
    if node_text.contains("static ") || has_child_of_kind(node, "static") {
        metadata.is_static = Some(true);
    }

    // Check for abstract
    if node_text.contains("abstract ") || has_child_of_kind(node, "abstract") {
        metadata.is_abstract = Some(true);
    }

    // Extract decorators (TypeScript uses @decorator syntax)
    let decorators = extract_decorators(node, source, "@");
    if !decorators.is_empty() {
        metadata.decorators = Some(decorators);
    }

    metadata
}

/// Extract Go-specific metadata.
fn extract_go_metadata(node: &Node, source: &[u8]) -> NodeMetadata {
    let mut metadata = NodeMetadata::default();

    // Go uses naming convention for visibility
    // Uppercase first letter = exported (public), lowercase = unexported (private)
    let identifier_kinds = ["identifier", "field_identifier", "type_identifier"];

    for identifier_kind in &identifier_kinds {
        if let Some(child) = find_child_of_kind(node, identifier_kind) {
            if let Ok(name) = child.utf8_text(source) {
                if let Some(first_char) = name.chars().next() {
                    if first_char.is_uppercase() {
                        metadata.visibility = Some("public".to_string());
                    } else {
                        metadata.visibility = Some("private".to_string());
                    }
                    break;
                }
            }
        }
    }

    metadata
}

/// Extract C#-specific metadata.
fn extract_csharp_metadata(_node: &Node, node_text: &str, _source: &[u8]) -> NodeMetadata {
    let mut metadata = NodeMetadata::default();

    // Extract visibility
    if node_text.contains("private ") {
        metadata.visibility = Some("private".to_string());
    } else if node_text.contains("protected ") {
        metadata.visibility = Some("protected".to_string());
    } else if node_text.contains("public ") {
        metadata.visibility = Some("public".to_string());
    } else if node_text.contains("internal ") {
        metadata.visibility = Some("internal".to_string());
    }

    // Check for static
    if node_text.contains("static ") {
        metadata.is_static = Some(true);
    }

    // Check for abstract
    if node_text.contains("abstract ") {
        metadata.is_abstract = Some(true);
    }

    // Check for virtual
    if node_text.contains("virtual ") {
        metadata.is_virtual = Some(true);
    }

    // Check for async
    if node_text.contains("async ") {
        metadata.is_async = Some(true);
    }

    // C# modifiers
    let mut modifiers = Vec::new();
    if node_text.contains("sealed ") {
        modifiers.push("sealed".to_string());
    }
    if node_text.contains("override ") {
        modifiers.push("override".to_string());
    }
    if node_text.contains("readonly ") {
        modifiers.push("readonly".to_string());
    }
    if node_text.contains("new ") {
        modifiers.push("new".to_string());
    }
    if !modifiers.is_empty() {
        metadata.modifiers = Some(modifiers);
    }

    // Note: C# attributes are captured by csharp-tags.scm @decorator tag
    // and would be processed by higher-level graph construction logic

    metadata
}

/// Extract Rust-specific metadata.
fn extract_rust_metadata(node: &Node, node_text: &str, source: &[u8]) -> NodeMetadata {
    let mut metadata = NodeMetadata::default();

    // Extract visibility
    if node_text.contains("pub ") || has_child_of_kind(node, "visibility_modifier") {
        metadata.visibility = Some("public".to_string());
    } else {
        metadata.visibility = Some("private".to_string());
    }

    // Check for async
    if node_text.contains("async ") || has_child_of_kind(node, "async") {
        metadata.is_async = Some(true);
    }

    // Rust modifiers
    let mut modifiers = Vec::new();
    if node_text.contains("const ") {
        modifiers.push("const".to_string());
    }
    if node_text.contains("mut ") {
        modifiers.push("mut".to_string());
    }
    if node_text.contains("unsafe ") {
        modifiers.push("unsafe".to_string());
    }
    if node_text.contains("extern ") {
        modifiers.push("extern".to_string());
    }
    if !modifiers.is_empty() {
        metadata.modifiers = Some(modifiers);
    }

    // Extract attributes (Rust uses #[attr] syntax)
    let attributes = extract_rust_attributes(node, source);
    if !attributes.is_empty() {
        metadata.decorators = Some(attributes);
    }

    metadata
}

/// Extract C/C++-specific metadata.
fn extract_c_cpp_metadata(_node: &Node, node_text: &str, _source: &[u8]) -> NodeMetadata {
    let mut metadata = NodeMetadata::default();

    // C++ visibility
    if node_text.contains("private:") || node_text.contains("private ") {
        metadata.visibility = Some("private".to_string());
    } else if node_text.contains("protected:") || node_text.contains("protected ") {
        metadata.visibility = Some("protected".to_string());
    } else if node_text.contains("public:") || node_text.contains("public ") {
        metadata.visibility = Some("public".to_string());
    }

    // Check for static
    if node_text.contains("static ") {
        metadata.is_static = Some(true);
    }

    // Check for virtual
    if node_text.contains("virtual ") {
        metadata.is_virtual = Some(true);
    }

    // C++ modifiers
    let mut modifiers = Vec::new();
    if node_text.contains("const ") {
        modifiers.push("const".to_string());
    }
    if node_text.contains("inline ") {
        modifiers.push("inline".to_string());
    }
    if node_text.contains("extern ") {
        modifiers.push("extern".to_string());
    }
    if node_text.contains("constexpr ") {
        modifiers.push("constexpr".to_string());
    }
    if node_text.contains("override") {
        modifiers.push("override".to_string());
    }
    if node_text.contains("final") {
        modifiers.push("final".to_string());
    }
    if node_text.contains("noexcept") {
        modifiers.push("noexcept".to_string());
    }
    if !modifiers.is_empty() {
        metadata.modifiers = Some(modifiers);
    }

    metadata
}

// ============================================================================
// Helper Functions for Metadata Extraction
// ============================================================================

/// Check if a node has a child of the specified kind.
fn has_child_of_kind(node: &Node, kind: &str) -> bool {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == kind {
            return true;
        }
    }
    false
}

/// Find the first child of a specific kind.
fn find_child_of_kind<'a>(node: &'a Node, kind: &str) -> Option<Node<'a>> {
    let mut cursor = node.walk();
    let result = node
        .children(&mut cursor)
        .find(|child| child.kind() == kind);
    result
}

/// Find identifier text from a node (looks for identifier child nodes).
fn find_identifier_text<'a>(node: &Node, source: &'a [u8]) -> Option<&'a str> {
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        let kind = child.kind();
        if kind == "identifier" || kind == "name" {
            return child.utf8_text(source).ok();
        }
    }
    None
}

/// Extract decorators/attributes from a node.
///
/// Looks for sibling decorator nodes that precede the given node.
fn extract_decorators(node: &Node, source: &[u8], prefix: &str) -> Vec<String> {
    let mut decorators = Vec::new();

    // Look for decorator nodes in the parent's children that precede this node
    if let Some(parent) = node.parent() {
        let mut cursor = parent.walk();
        for sibling in parent.children(&mut cursor) {
            // Stop when we reach our node
            if sibling.id() == node.id() {
                break;
            }

            let kind = sibling.kind();
            if kind == "decorator" || kind == "decorated_definition" {
                if let Ok(text) = sibling.utf8_text(source) {
                    // Extract the decorator name (strip @ prefix and parameters)
                    let decorator_text = text.trim();
                    if let Some(stripped) = decorator_text.strip_prefix(prefix) {
                        let name = stripped.split('(').next().unwrap_or(stripped).trim();
                        if !name.is_empty() {
                            decorators.push(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // Also check for decorators as direct children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "decorator" {
            if let Ok(text) = child.utf8_text(source) {
                let decorator_text = text.trim();
                if let Some(stripped) = decorator_text.strip_prefix(prefix) {
                    let name = stripped.split('(').next().unwrap_or(stripped).trim();
                    if !name.is_empty() {
                        decorators.push(name.to_string());
                    }
                }
            }
        }
    }

    decorators
}

/// Extract Rust attributes (#[...]) from a node.
fn extract_rust_attributes(node: &Node, source: &[u8]) -> Vec<String> {
    let mut attributes = Vec::new();

    // Look for attribute nodes in the parent's children that precede this node
    if let Some(parent) = node.parent() {
        let mut cursor = parent.walk();
        for sibling in parent.children(&mut cursor) {
            // Stop when we reach our node
            if sibling.id() == node.id() {
                break;
            }

            if sibling.kind() == "attribute_item" || sibling.kind() == "inner_attribute_item" {
                if let Ok(text) = sibling.utf8_text(source) {
                    // Extract attribute content (strip #[ and ])
                    let attr_text = text.trim();
                    if attr_text.starts_with("#[") && attr_text.ends_with(']') {
                        let inner = &attr_text[2..attr_text.len() - 1];
                        // Get just the attribute name (before any parameters)
                        let name = inner.split('(').next().unwrap_or(inner).trim();
                        if !name.is_empty() {
                            attributes.push(name.to_string());
                        }
                    }
                }
            }
        }
    }

    // Also check direct children
    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        if child.kind() == "attribute_item" || child.kind() == "inner_attribute_item" {
            if let Ok(text) = child.utf8_text(source) {
                let attr_text = text.trim();
                if attr_text.starts_with("#[") && attr_text.ends_with(']') {
                    let inner = &attr_text[2..attr_text.len() - 1];
                    let name = inner.split('(').next().unwrap_or(inner).trim();
                    if !name.is_empty() {
                        attributes.push(name.to_string());
                    }
                }
            }
        }
    }

    attributes
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_language_from_extension() {
        assert_eq!(
            SupportedLanguage::from_extension("py"),
            Some(SupportedLanguage::Python)
        );
        assert_eq!(
            SupportedLanguage::from_extension("js"),
            Some(SupportedLanguage::JavaScript)
        );
        assert_eq!(
            SupportedLanguage::from_extension("ts"),
            Some(SupportedLanguage::TypeScript)
        );
        assert_eq!(
            SupportedLanguage::from_extension("tsx"),
            Some(SupportedLanguage::Tsx)
        );
        assert_eq!(
            SupportedLanguage::from_extension("rs"),
            Some(SupportedLanguage::Rust)
        );
        assert_eq!(
            SupportedLanguage::from_extension("go"),
            Some(SupportedLanguage::Go)
        );
        assert_eq!(
            SupportedLanguage::from_extension("c"),
            Some(SupportedLanguage::C)
        );
        assert_eq!(
            SupportedLanguage::from_extension("cpp"),
            Some(SupportedLanguage::Cpp)
        );
        assert_eq!(
            SupportedLanguage::from_extension("cs"),
            Some(SupportedLanguage::CSharp)
        );
        assert_eq!(SupportedLanguage::from_extension("unknown"), None);
    }

    #[test]
    fn test_language_from_path() {
        assert_eq!(
            SupportedLanguage::from_path(Path::new("src/main.py")),
            Some(SupportedLanguage::Python)
        );
        assert_eq!(
            SupportedLanguage::from_path(Path::new("app.tsx")),
            Some(SupportedLanguage::Tsx)
        );
        assert_eq!(SupportedLanguage::from_path(Path::new("README.md")), None);
    }

    #[test]
    fn test_language_as_str() {
        assert_eq!(SupportedLanguage::Python.as_str(), "python");
        assert_eq!(SupportedLanguage::TypeScript.as_str(), "typescript");
        assert_eq!(SupportedLanguage::Tsx.as_str(), "typescript");
        assert_eq!(SupportedLanguage::CSharp.as_str(), "csharp");
    }

    #[test]
    fn test_parser_creation() {
        let parser = CodeParser::new(SupportedLanguage::Python);
        assert!(parser.is_ok());
    }

    #[test]
    fn test_parse_python() {
        let mut parser = CodeParser::new(SupportedLanguage::Python).unwrap();
        let source = "def hello():\n    pass";
        let tree = parser.parse(source);
        assert!(tree.is_ok());

        let tree = tree.unwrap();
        assert_eq!(tree.root_node().kind(), "module");
    }

    #[test]
    fn test_parse_rust() {
        let mut parser = CodeParser::new(SupportedLanguage::Rust).unwrap();
        let source = "fn main() {}";
        let tree = parser.parse(source);
        assert!(tree.is_ok());

        let tree = tree.unwrap();
        assert_eq!(tree.root_node().kind(), "source_file");
    }

    #[test]
    fn test_parse_typescript() {
        let mut parser = CodeParser::new(SupportedLanguage::TypeScript).unwrap();
        let source = "function greet(name: string): void {}";
        let tree = parser.parse(source);
        assert!(tree.is_ok());
    }

    #[test]
    fn test_query_manager_simple() {
        // Simple query that captures function names
        let query_source = r#"
            (function_definition
                name: (identifier) @name.definition.callable.function)
        "#;

        let qm = QueryManager::new(SupportedLanguage::Python, query_source);
        assert!(qm.is_ok());

        let qm = qm.unwrap();
        assert!(qm
            .capture_index("name.definition.callable.function")
            .is_some());
    }

    #[test]
    fn test_tag_extractor_python() {
        let query_source = r#"
            (function_definition
                name: (identifier) @name.definition.callable.function) @definition.callable.function
        "#;

        let mut extractor = TagExtractor::new(SupportedLanguage::Python, query_source).unwrap();

        let source = r#"
def hello():
    pass

def world():
    return 42
"#;

        let tags = extractor.extract(source).unwrap();

        // Should have 4 tags: 2 function definitions + 2 name captures
        assert_eq!(tags.len(), 4);

        // Check function names were captured
        let names: Vec<_> = tags
            .iter()
            .filter(|t| t.tag == "name.definition.callable.function")
            .map(|t| t.name.as_str())
            .collect();
        assert!(names.contains(&"hello"));
        assert!(names.contains(&"world"));
    }

    #[test]
    fn test_tag_extractor_rust() {
        let query_source = r#"
            (function_item
                name: (identifier) @name.definition.callable.function) @definition.callable.function
        "#;

        let mut extractor = TagExtractor::new(SupportedLanguage::Rust, query_source).unwrap();

        let source = r#"
fn main() {
    println!("Hello");
}

fn helper() -> i32 {
    42
}
"#;

        let tags = extractor.extract(source).unwrap();

        let names: Vec<_> = tags
            .iter()
            .filter(|t| t.tag == "name.definition.callable.function")
            .map(|t| t.name.as_str())
            .collect();
        assert!(names.contains(&"main"));
        assert!(names.contains(&"helper"));
    }

    #[test]
    fn test_extracted_tag_line_numbers() {
        let query_source = r#"
            (function_definition
                name: (identifier) @name.definition.callable.function)
        "#;

        let mut extractor = TagExtractor::new(SupportedLanguage::Python, query_source).unwrap();

        let source = "def foo():\n    pass";
        let tags = extractor.extract(source).unwrap();

        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].start_line, 0); // 0-indexed
        assert_eq!(tags[0].line_number(), 1); // 1-indexed for display
    }

    #[test]
    fn test_parser_for_path() {
        let parser = CodeParser::for_path(Path::new("test.py"));
        assert!(parser.is_ok());
        assert_eq!(parser.unwrap().language(), SupportedLanguage::Python);

        let parser = CodeParser::for_path(Path::new("test.unknown"));
        assert!(parser.is_err());
    }

    // Containment Context Tests

    #[test]
    fn test_containment_context_new() {
        let ctx = ContainmentContext::new();
        assert!(ctx.is_empty());
        assert_eq!(ctx.depth(), 0);
        assert_eq!(ctx.get_current_parent_id(), None);
    }

    #[test]
    fn test_containment_context_push() {
        let mut ctx = ContainmentContext::new();
        ctx.push_container(
            "file.py:MyClass".to_string(),
            "Container".to_string(),
            10,
            50,
            "MyClass".to_string(),
        );

        assert!(!ctx.is_empty());
        assert_eq!(ctx.depth(), 1);
        assert_eq!(ctx.get_current_parent_id(), Some("file.py:MyClass"));
        assert_eq!(ctx.get_containment_path(), vec!["MyClass"]);
    }

    #[test]
    fn test_containment_context_nested() {
        let mut ctx = ContainmentContext::new();

        // Push class
        ctx.push_container(
            "file.py:MyClass".to_string(),
            "Container".to_string(),
            10,
            50,
            "MyClass".to_string(),
        );

        // Push method inside class
        ctx.push_container(
            "file.py:MyClass:method".to_string(),
            "Callable".to_string(),
            15,
            25,
            "method".to_string(),
        );

        assert_eq!(ctx.depth(), 2);
        assert_eq!(ctx.get_current_parent_id(), Some("file.py:MyClass:method"));
        assert_eq!(ctx.get_containment_path(), vec!["MyClass", "method"]);
    }

    #[test]
    fn test_containment_context_update_pops() {
        let mut ctx = ContainmentContext::new();

        // Push class at lines 10-50
        ctx.push_container(
            "file.py:MyClass".to_string(),
            "Container".to_string(),
            10,
            50,
            "MyClass".to_string(),
        );

        // Push method at lines 15-25
        ctx.push_container(
            "file.py:MyClass:method".to_string(),
            "Callable".to_string(),
            15,
            25,
            "method".to_string(),
        );

        // Update to line 30 - should pop method but keep class
        ctx.update(30);
        assert_eq!(ctx.depth(), 1);
        assert_eq!(ctx.get_containment_path(), vec!["MyClass"]);

        // Update to line 55 - should pop class too
        ctx.update(55);
        assert!(ctx.is_empty());
    }

    #[test]
    fn test_containment_context_is_container_active() {
        let mut ctx = ContainmentContext::new();
        ctx.push_container(
            "file.py:MyClass".to_string(),
            "Container".to_string(),
            10,
            50,
            "MyClass".to_string(),
        );

        assert!(ctx.is_container_active("file.py:MyClass"));
        assert!(!ctx.is_container_active("file.py:OtherClass"));
    }

    #[test]
    fn test_containment_context_clear() {
        let mut ctx = ContainmentContext::new();
        ctx.push_container(
            "file.py:MyClass".to_string(),
            "Container".to_string(),
            10,
            50,
            "MyClass".to_string(),
        );

        ctx.clear();
        assert!(ctx.is_empty());
    }

    #[test]
    fn test_containment_context_only_containers_and_callables() {
        let mut ctx = ContainmentContext::new();

        // Data nodes should not be pushed
        ctx.push_container(
            "file.py:field".to_string(),
            "Data".to_string(),
            10,
            10,
            "field".to_string(),
        );

        assert!(ctx.is_empty());
    }

    #[test]
    fn test_containment_context_display() {
        let mut ctx = ContainmentContext::new();
        assert_eq!(format!("{}", ctx), "ContainmentContext(empty)");

        ctx.push_container(
            "file.py:MyClass".to_string(),
            "Container".to_string(),
            10,
            50,
            "MyClass".to_string(),
        );
        ctx.push_container(
            "file.py:MyClass:method".to_string(),
            "Callable".to_string(),
            15,
            25,
            "method".to_string(),
        );

        let display = format!("{}", ctx);
        assert!(display.contains("MyClass(Container)"));
        assert!(display.contains("method(Callable)"));
    }

    // Node ID Generation Tests

    #[test]
    fn test_generate_node_id_simple() {
        assert_eq!(
            generate_node_id("src/models.py", &[], "User", None),
            "src/models.py:User"
        );
    }

    #[test]
    fn test_generate_node_id_with_containment() {
        assert_eq!(
            generate_node_id("src/models.py", &["User"], "save", None),
            "src/models.py:User:save"
        );
    }

    #[test]
    fn test_generate_node_id_nested_containment() {
        assert_eq!(
            generate_node_id("src/models.py", &["Module", "Class"], "method", None),
            "src/models.py:Module:Class:method"
        );
    }

    #[test]
    fn test_generate_node_id_lambda_with_line() {
        assert_eq!(
            generate_node_id("src/utils.py", &["process"], "<lambda>", Some(42)),
            "src/utils.py:process:<lambda>:42"
        );
    }

    #[test]
    fn test_generate_node_id_lambda_without_line() {
        // Lambda without line number doesn't get special treatment
        assert_eq!(
            generate_node_id("src/utils.py", &["process"], "<lambda>", None),
            "src/utils.py:process:<lambda>"
        );
    }

    #[test]
    fn test_parse_node_id_simple() {
        let result = parse_node_id("src/models.py:User");
        assert!(result.is_some());

        let (file, containment, name) = result.unwrap();
        assert_eq!(file, "src/models.py");
        assert!(containment.is_empty());
        assert_eq!(name, "User");
    }

    #[test]
    fn test_parse_node_id_with_containment() {
        let result = parse_node_id("src/models.py:User:save");
        assert!(result.is_some());

        let (file, containment, name) = result.unwrap();
        assert_eq!(file, "src/models.py");
        assert_eq!(containment, vec!["User"]);
        assert_eq!(name, "save");
    }

    #[test]
    fn test_parse_node_id_nested() {
        let result = parse_node_id("src/models.py:Module:Class:method");
        assert!(result.is_some());

        let (file, containment, name) = result.unwrap();
        assert_eq!(file, "src/models.py");
        assert_eq!(containment, vec!["Module", "Class"]);
        assert_eq!(name, "method");
    }

    #[test]
    fn test_parse_node_id_invalid() {
        assert!(parse_node_id("invalid").is_none());
    }

    // Metadata Extraction Tests

    #[test]
    fn test_metadata_extractor_python_visibility() {
        let extractor = MetadataExtractor::new(SupportedLanguage::Python);

        // Public (default)
        let metadata = extractor.extract_from_name("my_function");
        assert_eq!(metadata.visibility, Some("public".to_string()));

        // Protected (single underscore)
        let metadata = extractor.extract_from_name("_internal_helper");
        assert_eq!(metadata.visibility, Some("protected".to_string()));

        // Private (double underscore)
        let metadata = extractor.extract_from_name("__private_method");
        assert_eq!(metadata.visibility, Some("private".to_string()));

        // Dunder methods are public
        let metadata = extractor.extract_from_name("__init__");
        assert_eq!(metadata.visibility, Some("public".to_string()));
    }

    #[test]
    fn test_metadata_extractor_go_visibility() {
        let extractor = MetadataExtractor::new(SupportedLanguage::Go);

        // Exported (uppercase)
        let metadata = extractor.extract_from_name("ExportedFunction");
        assert_eq!(metadata.visibility, Some("public".to_string()));

        // Unexported (lowercase)
        let metadata = extractor.extract_from_name("internalHelper");
        assert_eq!(metadata.visibility, Some("private".to_string()));
    }

    #[test]
    fn test_metadata_extraction_python_async() {
        let mut parser = CodeParser::new(SupportedLanguage::Python).unwrap();
        let source = "async def fetch_data():\n    pass";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::Python);

        // Find the function definition node
        let func_node = root.child(0).unwrap();
        let metadata = extractor.extract(&func_node, source.as_bytes());

        assert_eq!(metadata.is_async, Some(true));
    }

    #[test]
    fn test_metadata_extraction_rust_pub() {
        let mut parser = CodeParser::new(SupportedLanguage::Rust).unwrap();
        let source = "pub fn public_function() {}";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::Rust);
        let func_node = root.child(0).unwrap();
        let metadata = extractor.extract(&func_node, source.as_bytes());

        assert_eq!(metadata.visibility, Some("public".to_string()));
    }

    #[test]
    fn test_metadata_extraction_rust_private() {
        let mut parser = CodeParser::new(SupportedLanguage::Rust).unwrap();
        let source = "fn private_function() {}";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::Rust);
        let func_node = root.child(0).unwrap();
        let metadata = extractor.extract(&func_node, source.as_bytes());

        assert_eq!(metadata.visibility, Some("private".to_string()));
    }

    #[test]
    fn test_metadata_extraction_rust_async() {
        let mut parser = CodeParser::new(SupportedLanguage::Rust).unwrap();
        let source = "async fn async_function() {}";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::Rust);
        let func_node = root.child(0).unwrap();
        let metadata = extractor.extract(&func_node, source.as_bytes());

        assert_eq!(metadata.is_async, Some(true));
    }

    #[test]
    fn test_metadata_extraction_rust_modifiers() {
        let mut parser = CodeParser::new(SupportedLanguage::Rust).unwrap();
        let source = "pub const MY_CONST: i32 = 42;";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::Rust);
        let const_node = root.child(0).unwrap();
        let metadata = extractor.extract(&const_node, source.as_bytes());

        assert_eq!(metadata.visibility, Some("public".to_string()));
        assert!(metadata
            .modifiers
            .as_ref()
            .is_some_and(|m| m.contains(&"const".to_string())));
    }

    #[test]
    fn test_metadata_extraction_rust_unsafe() {
        let mut parser = CodeParser::new(SupportedLanguage::Rust).unwrap();
        let source = "unsafe fn dangerous() {}";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::Rust);
        let func_node = root.child(0).unwrap();
        let metadata = extractor.extract(&func_node, source.as_bytes());

        assert!(metadata
            .modifiers
            .as_ref()
            .is_some_and(|m| m.contains(&"unsafe".to_string())));
    }

    #[test]
    fn test_metadata_extraction_typescript_static() {
        let mut parser = CodeParser::new(SupportedLanguage::TypeScript).unwrap();
        let source = "class Foo { static bar() {} }";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::TypeScript);

        // Navigate to the method definition: class_declaration -> class_body -> method_definition
        let class_node = root.child(0).unwrap();
        let class_body = class_node.child_by_field_name("body").unwrap();

        // Find method_definition in class_body
        let mut cursor = class_body.walk();
        let method_node = class_body
            .children(&mut cursor)
            .find(|n| n.kind() == "method_definition")
            .unwrap();

        let metadata = extractor.extract(&method_node, source.as_bytes());
        assert_eq!(metadata.is_static, Some(true));
    }

    #[test]
    fn test_metadata_extraction_csharp_abstract() {
        let mut parser = CodeParser::new(SupportedLanguage::CSharp).unwrap();
        let source = "public abstract class MyClass {}";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::CSharp);
        let class_node = root.child(0).unwrap();
        let metadata = extractor.extract(&class_node, source.as_bytes());

        assert_eq!(metadata.visibility, Some("public".to_string()));
        assert_eq!(metadata.is_abstract, Some(true));
    }

    #[test]
    fn test_metadata_extraction_csharp_virtual() {
        let mut parser = CodeParser::new(SupportedLanguage::CSharp).unwrap();
        let source = "public virtual void DoSomething() {}";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::CSharp);
        let method_node = root.child(0).unwrap();
        let metadata = extractor.extract(&method_node, source.as_bytes());

        assert_eq!(metadata.visibility, Some("public".to_string()));
        assert_eq!(metadata.is_virtual, Some(true));
    }

    #[test]
    fn test_metadata_extraction_cpp_static() {
        let mut parser = CodeParser::new(SupportedLanguage::Cpp).unwrap();
        let source = "static int count = 0;";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::Cpp);
        let decl_node = root.child(0).unwrap();
        let metadata = extractor.extract(&decl_node, source.as_bytes());

        assert_eq!(metadata.is_static, Some(true));
    }

    #[test]
    fn test_metadata_extraction_cpp_virtual() {
        let mut parser = CodeParser::new(SupportedLanguage::Cpp).unwrap();
        let source = "virtual void update() {}";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::Cpp);
        let func_node = root.child(0).unwrap();
        let metadata = extractor.extract(&func_node, source.as_bytes());

        assert_eq!(metadata.is_virtual, Some(true));
    }

    #[test]
    fn test_metadata_extraction_cpp_modifiers() {
        let mut parser = CodeParser::new(SupportedLanguage::Cpp).unwrap();
        let source = "inline constexpr int square(int x) { return x * x; }";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::Cpp);
        let func_node = root.child(0).unwrap();
        let metadata = extractor.extract(&func_node, source.as_bytes());

        let modifiers = metadata.modifiers.unwrap();
        assert!(modifiers.contains(&"inline".to_string()));
        assert!(modifiers.contains(&"constexpr".to_string()));
    }

    #[test]
    fn test_metadata_extraction_go() {
        let mut parser = CodeParser::new(SupportedLanguage::Go).unwrap();
        let source = "func ExportedFunc() {}";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::Go);
        let func_node = root.child(0).unwrap();
        let metadata = extractor.extract(&func_node, source.as_bytes());

        assert_eq!(metadata.visibility, Some("public".to_string()));
    }

    #[test]
    fn test_metadata_extraction_go_unexported() {
        let mut parser = CodeParser::new(SupportedLanguage::Go).unwrap();
        let source = "func internalFunc() {}";
        let tree = parser.parse(source).unwrap();
        let root = tree.root_node();

        let extractor = MetadataExtractor::new(SupportedLanguage::Go);
        let func_node = root.child(0).unwrap();
        let metadata = extractor.extract(&func_node, source.as_bytes());

        assert_eq!(metadata.visibility, Some("private".to_string()));
    }

    // ========================================================================
    // ManifestLanguage Tests
    // ========================================================================

    #[test]
    fn test_manifest_language_from_filename_json() {
        assert_eq!(
            ManifestLanguage::from_filename("package.json"),
            Some(ManifestLanguage::Json)
        );
        assert_eq!(
            ManifestLanguage::from_filename("vcpkg.json"),
            Some(ManifestLanguage::Json)
        );
    }

    #[test]
    fn test_manifest_language_from_filename_toml() {
        assert_eq!(
            ManifestLanguage::from_filename("Cargo.toml"),
            Some(ManifestLanguage::Toml)
        );
        assert_eq!(
            ManifestLanguage::from_filename("pyproject.toml"),
            Some(ManifestLanguage::Toml)
        );
    }

    #[test]
    fn test_manifest_language_from_filename_gomod() {
        assert_eq!(
            ManifestLanguage::from_filename("go.mod"),
            Some(ManifestLanguage::GoMod)
        );
    }

    #[test]
    fn test_manifest_language_from_filename_xml() {
        assert_eq!(
            ManifestLanguage::from_filename("MyProject.csproj"),
            Some(ManifestLanguage::Xml)
        );
        assert_eq!(
            ManifestLanguage::from_filename("Legacy.vbproj"),
            Some(ManifestLanguage::Xml)
        );
        assert_eq!(
            ManifestLanguage::from_filename("Functional.fsproj"),
            Some(ManifestLanguage::Xml)
        );
    }

    #[test]
    fn test_manifest_language_from_filename_cmake() {
        assert_eq!(
            ManifestLanguage::from_filename("CMakeLists.txt"),
            Some(ManifestLanguage::CMake)
        );
    }

    #[test]
    fn test_manifest_language_from_filename_not_recognized() {
        assert_eq!(ManifestLanguage::from_filename("README.md"), None);
        assert_eq!(ManifestLanguage::from_filename("main.rs"), None);
        assert_eq!(ManifestLanguage::from_filename("config.json"), None);
        assert_eq!(ManifestLanguage::from_filename("settings.toml"), None);
    }

    #[test]
    fn test_manifest_language_from_path() {
        assert_eq!(
            ManifestLanguage::from_path(Path::new("packages/core/package.json")),
            Some(ManifestLanguage::Json)
        );
        assert_eq!(
            ManifestLanguage::from_path(Path::new("crates/codeprysm-core/Cargo.toml")),
            Some(ManifestLanguage::Toml)
        );
        assert_eq!(
            ManifestLanguage::from_path(Path::new("src/MyProject.csproj")),
            Some(ManifestLanguage::Xml)
        );
        assert_eq!(ManifestLanguage::from_path(Path::new("src/main.rs")), None);
    }

    #[test]
    fn test_manifest_language_is_manifest_file() {
        assert!(ManifestLanguage::is_manifest_file(Path::new(
            "package.json"
        )));
        assert!(ManifestLanguage::is_manifest_file(Path::new("Cargo.toml")));
        assert!(ManifestLanguage::is_manifest_file(Path::new("go.mod")));
        assert!(ManifestLanguage::is_manifest_file(Path::new(
            "CMakeLists.txt"
        )));
        assert!(ManifestLanguage::is_manifest_file(Path::new(
            "MyApp.csproj"
        )));
        assert!(!ManifestLanguage::is_manifest_file(Path::new("main.py")));
        assert!(!ManifestLanguage::is_manifest_file(Path::new("README.md")));
    }

    #[test]
    fn test_manifest_language_as_str() {
        assert_eq!(ManifestLanguage::Json.as_str(), "json");
        assert_eq!(ManifestLanguage::Toml.as_str(), "toml");
        assert_eq!(ManifestLanguage::GoMod.as_str(), "gomod");
        assert_eq!(ManifestLanguage::Xml.as_str(), "xml");
        assert_eq!(ManifestLanguage::CMake.as_str(), "cmake");
    }

    #[test]
    fn test_manifest_language_display() {
        assert_eq!(format!("{}", ManifestLanguage::Json), "json");
        assert_eq!(format!("{}", ManifestLanguage::Toml), "toml");
        assert_eq!(format!("{}", ManifestLanguage::GoMod), "gomod");
        assert_eq!(format!("{}", ManifestLanguage::Xml), "xml");
        assert_eq!(format!("{}", ManifestLanguage::CMake), "cmake");
    }

    #[test]
    fn test_manifest_language_tree_sitter_language() {
        // Verify each manifest language can create a working parser
        for lang in [
            ManifestLanguage::Json,
            ManifestLanguage::Toml,
            ManifestLanguage::GoMod,
            ManifestLanguage::Xml,
            ManifestLanguage::CMake,
        ] {
            let mut parser = Parser::new();
            let result = parser.set_language(&lang.tree_sitter_language());
            assert!(
                result.is_ok(),
                "Failed to set tree-sitter language for {:?}",
                lang
            );
        }
    }

    #[test]
    fn test_manifest_language_parse_json() {
        let mut parser = Parser::new();
        parser
            .set_language(&ManifestLanguage::Json.tree_sitter_language())
            .unwrap();

        let source = r#"{"name": "my-package", "version": "1.0.0"}"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn test_manifest_language_parse_toml() {
        let mut parser = Parser::new();
        parser
            .set_language(&ManifestLanguage::Toml.tree_sitter_language())
            .unwrap();

        let source = r#"[package]
name = "my-crate"
version = "0.1.0"
"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn test_manifest_language_parse_gomod() {
        let mut parser = Parser::new();
        parser
            .set_language(&ManifestLanguage::GoMod.tree_sitter_language())
            .unwrap();

        let source = r#"module github.com/example/mymodule

go 1.21
"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn test_manifest_language_parse_xml() {
        let mut parser = Parser::new();
        parser
            .set_language(&ManifestLanguage::Xml.tree_sitter_language())
            .unwrap();

        let source = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <AssemblyName>MyProject</AssemblyName>
  </PropertyGroup>
</Project>"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn test_manifest_language_parse_cmake() {
        let mut parser = Parser::new();
        parser
            .set_language(&ManifestLanguage::CMake.tree_sitter_language())
            .unwrap();

        let source = r#"cmake_minimum_required(VERSION 3.20)
project(my-project VERSION 1.0.0)
"#;
        let tree = parser.parse(source, None).unwrap();
        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn test_manifest_language_exact_filenames() {
        let filenames = ManifestLanguage::exact_filenames();
        assert!(filenames.contains(&"package.json"));
        assert!(filenames.contains(&"Cargo.toml"));
        assert!(filenames.contains(&"go.mod"));
        assert!(filenames.contains(&"CMakeLists.txt"));
    }

    #[test]
    fn test_manifest_language_extensions() {
        let extensions = ManifestLanguage::manifest_extensions();
        assert!(extensions.contains(&"csproj"));
        assert!(extensions.contains(&"vbproj"));
        assert!(extensions.contains(&"fsproj"));
    }
}
