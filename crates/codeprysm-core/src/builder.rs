//! Graph Builder for Code Graph Generation
//!
//! This module provides the `GraphBuilder` struct for constructing code graphs
//! from source files using tree-sitter parsing and SCM queries.
//!
//! ## Usage
//!
//! ```ignore
//! use codeprysm_core::builder::GraphBuilder;
//! use std::path::Path;
//!
//! let builder = GraphBuilder::new(Path::new("queries"))?;
//! let graph = builder.build_from_directory(Path::new("src"))?;
//! ```

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use thiserror::Error;
use tracing::{debug, info, warn};

use crate::discovery::{DiscoveredRoot, RootDiscovery};
use crate::graph::{
    CallableKind, ContainerKind, DataKind, Edge, EdgeType, Node, NodeMetadata, NodeType,
    PetCodeGraph,
};
use crate::manifest::{DependencyType, LocalDependency, ManifestInfo, ManifestParser};
use crate::merkle::compute_file_hash;
use crate::parser::{
    generate_node_id, ContainmentContext, ManifestLanguage, MetadataExtractor, SupportedLanguage,
    TagExtractor,
};
use crate::tags::{parse_tag_string, TagParseResult};

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur during graph building.
#[derive(Debug, Error)]
pub enum BuilderError {
    /// IO error
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Parser error
    #[error("Parser error: {0}")]
    Parser(#[from] crate::parser::ParserError),

    /// Query directory not found
    #[error("Query directory not found: {0}")]
    QueryDirNotFound(PathBuf),

    /// No supported files found
    #[error("No supported files found in directory: {0}")]
    NoFilesFound(PathBuf),
}

// ============================================================================
// Builder Configuration
// ============================================================================

/// Configuration for the graph builder.
#[derive(Debug, Clone)]
pub struct BuilderConfig {
    /// Skip Data nodes (parameters, locals, fields) for smaller graphs
    pub skip_data_nodes: bool,
    /// Maximum containment depth (None = unlimited)
    pub max_containment_depth: Option<usize>,
    /// Maximum number of files to process (None = unlimited)
    pub max_files: Option<usize>,
    /// File patterns to exclude (glob patterns)
    pub exclude_patterns: Vec<String>,
}

impl Default for BuilderConfig {
    fn default() -> Self {
        Self {
            skip_data_nodes: false,
            max_containment_depth: None,
            max_files: None,
            exclude_patterns: vec![
                "**/.git/**".to_string(),
                "**/node_modules/**".to_string(),
                "**/target/**".to_string(),
                "**/__pycache__/**".to_string(),
                "**/.venv/**".to_string(),
                "**/venv/**".to_string(),
                "**/.tox/**".to_string(),
                "**/dist/**".to_string(),
                "**/build/**".to_string(),
            ],
        }
    }
}

// ============================================================================
// Reference Info
// ============================================================================

/// Information about a reference to be resolved later.
#[derive(Debug, Clone)]
struct ReferenceInfo {
    /// Source node ID (where the reference comes from)
    source_id: String,
    /// Line number of the reference
    line: usize,
}

// ============================================================================
// Graph Builder
// ============================================================================

/// Builds code graphs from source directories.
///
/// The `GraphBuilder` walks a source directory, parses files using tree-sitter,
/// extracts code entities via SCM queries, and constructs a code graph with
/// nodes and edges representing code structure and dependencies.
///
/// ## Example
///
/// ```ignore
/// use codeprysm_core::builder::{GraphBuilder, BuilderConfig};
/// use std::path::Path;
///
/// let config = BuilderConfig::default();
/// let builder = GraphBuilder::with_config(Path::new("queries"), config)?;
/// let graph = builder.build_from_directory(Path::new("src"))?;
///
/// println!("Built graph with {} nodes", graph.node_count());
/// ```
pub struct GraphBuilder {
    /// Path to SCM query files (None = use embedded queries)
    queries_dir: Option<PathBuf>,
    /// Builder configuration
    config: BuilderConfig,
}

impl GraphBuilder {
    /// Create a new graph builder with default configuration using embedded queries.
    ///
    /// This is the preferred constructor for production use as it doesn't require
    /// external query files.
    pub fn new_with_embedded_queries() -> Self {
        Self {
            queries_dir: None,
            config: BuilderConfig::default(),
        }
    }

    /// Create a new graph builder with custom configuration using embedded queries.
    ///
    /// This is the preferred constructor for production use as it doesn't require
    /// external query files.
    pub fn with_embedded_queries(config: BuilderConfig) -> Self {
        Self {
            queries_dir: None,
            config,
        }
    }

    /// Create a new graph builder with default configuration using query files.
    ///
    /// # Arguments
    ///
    /// * `queries_dir` - Path to directory containing SCM query files
    ///
    /// # Errors
    ///
    /// Returns an error if the queries directory doesn't exist.
    pub fn new(queries_dir: &Path) -> Result<Self, BuilderError> {
        Self::with_config(queries_dir, BuilderConfig::default())
    }

    /// Create a new graph builder with custom configuration using query files.
    ///
    /// # Arguments
    ///
    /// * `queries_dir` - Path to directory containing SCM query files
    /// * `config` - Builder configuration
    ///
    /// # Errors
    ///
    /// Returns an error if the queries directory doesn't exist.
    pub fn with_config(queries_dir: &Path, config: BuilderConfig) -> Result<Self, BuilderError> {
        if !queries_dir.exists() {
            return Err(BuilderError::QueryDirNotFound(queries_dir.to_path_buf()));
        }

        Ok(Self {
            queries_dir: Some(queries_dir.to_path_buf()),
            config,
        })
    }

    /// Build a code graph from a directory.
    ///
    /// Walks the directory, processes all supported source files, and
    /// constructs a code graph with nodes and edges.
    ///
    /// # Arguments
    ///
    /// * `directory` - Root directory to process
    ///
    /// # Returns
    ///
    /// A `PetCodeGraph` containing all discovered code entities and relationships.
    /// Uses petgraph::StableGraph internally for efficient traversal and algorithms.
    pub fn build_from_directory(&mut self, directory: &Path) -> Result<PetCodeGraph, BuilderError> {
        let mut graph = PetCodeGraph::new();

        // Create Repository node as root of the hierarchy
        let repo_name = get_repo_name(directory);
        let (git_remote, git_branch, git_commit) = extract_git_metadata(directory);
        let repo_metadata = NodeMetadata::default().with_git(git_remote, git_branch, git_commit);
        let repo_node = Node::repository(repo_name.clone(), repo_metadata);
        graph.add_node(repo_node);

        info!("Created repository node: {}", repo_name);

        // Track definitions and references for later resolution
        let mut defines: HashMap<String, String> = HashMap::new();
        let mut references: HashMap<String, Vec<ReferenceInfo>> = HashMap::new();

        // Statistics
        let mut file_count = 0;
        let mut skipped_data_nodes = 0;
        let mut skipped_depth_nodes = 0;

        info!("Processing files in {}", directory.display());

        // Collect files to process
        let files: Vec<PathBuf> = self.collect_files(directory)?;

        if files.is_empty() {
            return Err(BuilderError::NoFilesFound(directory.to_path_buf()));
        }

        info!("Found {} files to process", files.len());

        // Process each file
        for file_path in files {
            // Check max files limit
            if let Some(max) = self.config.max_files {
                if file_count >= max {
                    info!("Reached maximum file limit of {}", max);
                    break;
                }
            }

            // Get relative path
            let rel_path = file_path
                .strip_prefix(directory)
                .unwrap_or(&file_path)
                .to_string_lossy()
                .to_string();

            // Process the file
            match self.process_file(
                &file_path,
                &rel_path,
                &repo_name,
                &mut graph,
                &mut defines,
                &mut references,
                &mut skipped_data_nodes,
                &mut skipped_depth_nodes,
            ) {
                Ok(_) => {
                    file_count += 1;
                    if file_count % 100 == 0 {
                        debug!("Processed {} files", file_count);
                    }
                }
                Err(e) => {
                    warn!("Error processing {}: {}", rel_path, e);
                }
            }
        }

        info!("Processed {} files", file_count);

        // Resolve references and create USES edges
        self.resolve_references(&mut graph, &defines, &references);

        // Log statistics
        let contains_count = graph.edges_by_type(EdgeType::Contains).count();
        let uses_count = graph.edges_by_type(EdgeType::Uses).count();
        let defines_count = graph.edges_by_type(EdgeType::Defines).count();

        info!("Graph summary:");
        info!("  - Nodes: {}", graph.node_count());
        info!("  - CONTAINS edges: {}", contains_count);
        info!("  - USES edges: {}", uses_count);
        info!("  - DEFINES edges: {}", defines_count);
        info!("  - Total edges: {}", graph.edge_count());

        if skipped_data_nodes > 0 || skipped_depth_nodes > 0 {
            info!("Performance filtering:");
            if skipped_data_nodes > 0 {
                info!("  - Skipped Data nodes: {}", skipped_data_nodes);
            }
            if skipped_depth_nodes > 0 {
                info!("  - Skipped nodes (max depth): {}", skipped_depth_nodes);
            }
        }

        Ok(graph)
    }

    /// Build a code graph from a workspace root that may contain multiple repositories.
    ///
    /// This method discovers all code roots (git repositories and code directories)
    /// under the given workspace path and builds a unified graph.
    ///
    /// - If the root is a single repository or contains only one code root,
    ///   returns a graph with that repository as the root (backward compatible).
    /// - Otherwise, creates a workspace container with multiple repository children.
    ///
    /// # Arguments
    ///
    /// * `workspace_path` - Root directory to analyze (may contain multiple repos)
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - The unified `PetCodeGraph` with all discovered code entities
    /// - A list of `DiscoveredRoot` describing each discovered root
    ///
    /// # Example
    ///
    /// ```ignore
    /// use codeprysm_core::builder::GraphBuilder;
    /// use std::path::Path;
    ///
    /// let builder = GraphBuilder::new(Path::new("queries"))?;
    /// let (graph, roots) = builder.build_from_workspace(Path::new("/workspace"))?;
    ///
    /// println!("Found {} code roots", roots.len());
    /// println!("Graph has {} nodes", graph.node_count());
    /// ```
    pub fn build_from_workspace(
        &mut self,
        workspace_path: &Path,
    ) -> Result<(PetCodeGraph, Vec<DiscoveredRoot>), BuilderError> {
        let workspace_path = workspace_path.canonicalize().map_err(BuilderError::Io)?;

        info!("Building workspace graph from {:?}", workspace_path);

        // Discover roots under the workspace
        let discovery = RootDiscovery::with_defaults();
        let roots = discovery
            .discover(&workspace_path)
            .map_err(|e| BuilderError::Io(std::io::Error::other(e.to_string())))?;

        info!("Discovered {} code root(s)", roots.len());

        // Single root case: use existing behavior for backward compatibility
        // This means the workspace itself is the single root (git repo or code dir)
        if roots.len() == 1 && roots[0].relative_path == "." {
            info!("Single root at workspace path, using standard build");
            let graph = self.build_from_directory(&workspace_path)?;
            return Ok((graph, roots));
        }

        // Multi-root case: create workspace container and merge roots
        let workspace_name = workspace_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "workspace".to_string());

        info!(
            "Creating workspace '{}' with {} roots",
            workspace_name,
            roots.len()
        );

        // Use PetCodeGraph directly for efficient construction and traversal
        let mut workspace_graph = PetCodeGraph::new();

        // Create workspace node as the root
        let workspace_node = Node::workspace(workspace_name.clone());
        let workspace_id = workspace_node.id.clone();
        workspace_graph.add_node(workspace_node);

        // Build and merge each discovered root
        for root in &roots {
            info!("Processing root: {} ({:?})", root.name, root.root_type);

            // Build graph for this root (returns PetCodeGraph)
            let root_graph = match self.build_from_directory(&root.path) {
                Ok(g) => g,
                Err(e) => {
                    warn!("Failed to build graph for {}: {}", root.name, e);
                    continue;
                }
            };

            // Find the repository/directory node (root of this sub-graph)
            let root_node_id = self.find_root_node_id(&root_graph, root);

            // Merge the root graph into the workspace graph
            self.merge_root_graph(
                &mut workspace_graph,
                root_graph,
                &workspace_id,
                &root_node_id,
            );

            info!("Merged root '{}' into workspace graph", root.name);
        }

        info!(
            "Workspace graph complete: {} nodes, {} edges across {} roots",
            workspace_graph.node_count(),
            workspace_graph.edge_count(),
            roots.len()
        );

        Ok((workspace_graph, roots))
    }

    /// Find the root node ID in a built graph (repository or first container)
    fn find_root_node_id(&self, graph: &PetCodeGraph, root: &DiscoveredRoot) -> String {
        // Look for repository node first
        graph
            .iter_nodes()
            .find(|n| n.is_repository())
            .map(|n| n.id.clone())
            .unwrap_or_else(|| root.name.clone())
    }

    /// Merge a root graph into the workspace graph
    fn merge_root_graph(
        &self,
        workspace_graph: &mut PetCodeGraph,
        root_graph: PetCodeGraph,
        workspace_id: &str,
        root_node_id: &str,
    ) {
        // Add all nodes from the root graph
        for node in root_graph.iter_nodes() {
            workspace_graph.add_node(node.clone());
        }

        // Add all edges from the root graph
        for edge in root_graph.iter_edges() {
            workspace_graph.add_edge_from_struct(&edge);
        }

        // Add CONTAINS edge from workspace to this root
        workspace_graph.add_edge_from_struct(&Edge::contains(
            workspace_id.to_string(),
            root_node_id.to_string(),
        ));
    }

    /// Collect all supported source files from a directory.
    ///
    /// Uses the `ignore` crate to respect:
    /// - `.gitignore` files
    /// - `.codeprysmignore` files (custom exclusions for CodePrysm indexing)
    /// - Global gitignore patterns
    fn collect_files(&self, directory: &Path) -> Result<Vec<PathBuf>, BuilderError> {
        let mut files = Vec::new();
        let glob_set = self.build_exclude_glob_set();

        // Use ignore::WalkBuilder which respects .gitignore and custom ignore files
        let walker = WalkBuilder::new(directory)
            .follow_links(false)
            .hidden(true) // Skip hidden files/directories
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .add_custom_ignore_filename(".codeprysmignore") // Respect .codeprysmignore
            .build();

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    debug!("Error walking directory: {}", e);
                    continue;
                }
            };

            // Skip directories - we only want files
            let file_type = match entry.file_type() {
                Some(ft) => ft,
                None => continue,
            };
            if !file_type.is_file() {
                continue;
            }

            let path = entry.path();

            // Check if file is supported
            if SupportedLanguage::from_path(path).is_none() {
                continue;
            }

            // Check additional exclude patterns from config (beyond .gitignore/.codeprysmignore)
            let rel_path = path
                .strip_prefix(directory)
                .unwrap_or(path)
                .to_string_lossy();
            if glob_set.is_match(rel_path.as_ref()) {
                continue;
            }

            files.push(path.to_path_buf());
        }

        // Sort for deterministic ordering
        files.sort();

        Ok(files)
    }

    /// Build a glob set from exclude patterns.
    fn build_exclude_glob_set(&self) -> globset::GlobSet {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in &self.config.exclude_patterns {
            if let Ok(glob) = globset::Glob::new(pattern) {
                builder.add(glob);
            }
        }
        builder
            .build()
            .unwrap_or_else(|_| globset::GlobSet::empty())
    }

    /// Process a single file and add its entities to the graph.
    #[allow(clippy::too_many_arguments)]
    fn process_file(
        &mut self,
        file_path: &Path,
        rel_path: &str,
        repo_name: &str,
        graph: &mut PetCodeGraph,
        defines: &mut HashMap<String, String>,
        references: &mut HashMap<String, Vec<ReferenceInfo>>,
        skipped_data_nodes: &mut usize,
        skipped_depth_nodes: &mut usize,
    ) -> Result<(), BuilderError> {
        // Detect language
        let language = match SupportedLanguage::from_path(file_path) {
            Some(lang) => lang,
            None => return Ok(()), // Skip unsupported files
        };

        // Read file content
        let source = std::fs::read_to_string(file_path)?;

        // Compute file hash
        let file_hash = compute_file_hash(file_path)?;

        // Count lines
        let line_count = source.lines().count();

        // Add file container node
        let file_node = Node::source_file(
            rel_path.to_string(),
            rel_path.to_string(),
            file_hash,
            line_count,
        );
        graph.add_node(file_node);

        // Add CONTAINS edge from Repository to File (if we have a repo context)
        if !repo_name.is_empty() {
            graph
                .add_edge_from_struct(&Edge::contains(repo_name.to_string(), rel_path.to_string()));
        }

        // Get or create tag extractor for this language
        let mut extractor = match &self.queries_dir {
            Some(dir) => TagExtractor::from_queries_dir(language, dir)?,
            None => TagExtractor::from_embedded(language)?,
        };
        let metadata_extractor = MetadataExtractor::new(language);

        // Extract tags
        let tags = extractor.extract(&source)?;

        // Separate definition and reference tags
        // IMPORTANT: Only use `name.` prefixed tags (e.g., @name.definition.X) which capture
        // the identifier. Tags without `name.` prefix (e.g., @definition.X) capture the whole
        // node body and should be skipped for node creation.
        let mut definition_tags: Vec<_> = tags
            .iter()
            .filter(|t| t.tag.starts_with("name.") && t.tag.contains(".definition."))
            .collect();

        let reference_tags: Vec<_> = tags
            .iter()
            .filter(|t| t.tag.starts_with("name.") && t.tag.contains(".reference."))
            .collect();

        // Sort definition tags by line for proper containment tracking
        definition_tags.sort_by_key(|t| (t.start_line, t.end_line));

        // Initialize containment context
        let mut containment_ctx = ContainmentContext::new();

        // Process definitions
        for tag in &definition_tags {
            // Parse tag type
            let tag_string = normalize_tag_string(&tag.tag);
            let tag_info = match parse_tag_string(&tag_string) {
                Ok(info) => info,
                Err(e) => {
                    warn!(
                        "Could not parse tag type '{}' in {}:{}: {}",
                        tag.tag,
                        rel_path,
                        tag.line_number(),
                        e
                    );
                    continue;
                }
            };

            // Skip Data nodes if configured
            if self.config.skip_data_nodes && tag_info.node_type == NodeType::Data {
                *skipped_data_nodes += 1;
                continue;
            }

            // Update containment context (use parent line range for proper nesting)
            containment_ctx.update(tag.containment_start_line());

            // Check max containment depth
            if let Some(max_depth) = self.config.max_containment_depth {
                let current_depth = containment_ctx.depth();
                if current_depth >= max_depth {
                    *skipped_depth_nodes += 1;
                    continue;
                }
            }

            // Get containment path - special handling for Rust impl methods
            let (containment_path, parent_id) = if let Some(impl_type) = &tag.impl_target {
                // For Rust methods inside impl blocks, use the impl type as parent
                let impl_type_id = format!("{}:{}", rel_path, impl_type);
                (vec![impl_type.as_str()], impl_type_id)
            } else {
                // Normal containment tracking
                let path = containment_ctx.get_containment_path();
                let parent = containment_ctx
                    .get_current_parent_id()
                    .map(String::from)
                    .unwrap_or_else(|| rel_path.to_string());
                (path, parent)
            };

            // Skip self-referential containment
            if containment_path.last() == Some(&tag.name.as_str()) {
                continue;
            }

            // Generate node ID
            let node_id = generate_node_id(
                rel_path,
                &containment_path,
                &tag.name,
                Some(tag.line_number()),
            );

            // Add to definitions dictionary
            defines.insert(tag.name.clone(), node_id.clone());

            // Create node
            let node = self.create_node_from_tag(
                &node_id,
                &tag.name,
                &tag_info,
                rel_path,
                tag.line_number(),
                tag.end_line_number(),
                &metadata_extractor,
            );

            // Skip if node already exists
            if graph.contains_node(&node_id) {
                continue;
            }

            // Add node to graph
            graph.add_node(node);

            // Add CONTAINS edge from parent
            graph.add_edge_from_struct(&Edge::contains(parent_id.clone(), node_id.clone()));

            // Add DEFINES edge for Data nodes (if parent is not the file)
            if tag_info.node_type == NodeType::Data && parent_id != rel_path {
                graph.add_edge_from_struct(&Edge::defines(parent_id.clone(), node_id.clone()));
            }

            // Push containers onto containment stack (use parent line range for proper nesting)
            let node_type_str = tag_info.node_type.as_str();
            if node_type_str == "Container" || node_type_str == "Callable" {
                containment_ctx.push_container(
                    node_id,
                    node_type_str.to_string(),
                    tag.containment_start_line(),
                    tag.containment_end_line(),
                    tag.name.clone(),
                );
            }
        }

        // Process references
        for tag in &reference_tags {
            let tag_string = normalize_tag_string(&tag.tag);
            let _tag_info = match parse_tag_string(&tag_string) {
                Ok(info) => info,
                Err(_) => continue,
            };

            // Find source entity context
            // Rebuild containment by finding enclosing definitions
            let source_id = self.find_enclosing_context(&definition_tags, tag.start_line, rel_path);

            // Store reference
            references
                .entry(tag.name.clone())
                .or_default()
                .push(ReferenceInfo {
                    source_id,
                    line: tag.line_number(),
                });
        }

        Ok(())
    }

    /// Create a Node from a parsed tag.
    #[allow(clippy::too_many_arguments)]
    fn create_node_from_tag(
        &self,
        node_id: &str,
        name: &str,
        tag_info: &TagParseResult,
        file: &str,
        line: usize,
        end_line: usize,
        metadata_extractor: &MetadataExtractor,
    ) -> Node {
        // Extract metadata from name (for visibility conventions)
        let metadata = metadata_extractor.extract_from_name(name);

        match tag_info.node_type {
            NodeType::Container => {
                let kind = match &tag_info.kind {
                    Some(crate::graph::NodeKind::Container(k)) => *k,
                    _ => ContainerKind::Type,
                };
                // File containers use source_file() constructor
                if kind == ContainerKind::File {
                    Node::source_file(
                        node_id.to_string(),
                        file.to_string(),
                        String::new(),
                        end_line,
                    )
                } else {
                    Node::container(
                        node_id.to_string(),
                        name.to_string(),
                        kind,
                        tag_info.subtype.clone(),
                        file.to_string(),
                        line,
                        end_line,
                    )
                    .with_metadata(metadata)
                }
            }
            NodeType::Callable => {
                let kind = match &tag_info.kind {
                    Some(crate::graph::NodeKind::Callable(k)) => *k,
                    _ => CallableKind::Function,
                };
                let node = Node::callable(
                    node_id.to_string(),
                    name.to_string(),
                    kind,
                    file.to_string(),
                    line,
                    end_line,
                );

                // Add scope to metadata if present
                let mut meta = metadata;
                if let Some(scope) = &tag_info.scope {
                    meta.scope = Some(scope.clone());
                }
                node.with_metadata(meta)
            }
            NodeType::Data => {
                let kind = match &tag_info.kind {
                    Some(crate::graph::NodeKind::Data(k)) => *k,
                    _ => DataKind::Value,
                };
                Node::data(
                    node_id.to_string(),
                    name.to_string(),
                    kind,
                    tag_info.subtype.clone(),
                    file.to_string(),
                    line,
                    end_line,
                )
                .with_metadata(metadata)
            }
        }
    }

    /// Find the enclosing context for a reference at a given line.
    fn find_enclosing_context(
        &self,
        definition_tags: &[&crate::parser::ExtractedTag],
        line: usize,
        file: &str,
    ) -> String {
        // Find the innermost enclosing definition (use containment lines for proper nesting)
        let mut enclosing: Option<&crate::parser::ExtractedTag> = None;

        for tag in definition_tags {
            // Check if this definition contains the reference line
            let tag_start = tag.containment_start_line();
            let tag_end = tag.containment_end_line();
            if tag_start <= line && tag_end >= line {
                // Parse tag to check if it's a Container or Callable
                let tag_string = normalize_tag_string(&tag.tag);
                if let Ok(info) = parse_tag_string(&tag_string) {
                    if info.node_type == NodeType::Container || info.node_type == NodeType::Callable
                    {
                        // Check if this is more specific (narrower) than current enclosing
                        if let Some(current) = enclosing {
                            let current_start = current.containment_start_line();
                            let current_end = current.containment_end_line();
                            if tag_start >= current_start && tag_end <= current_end {
                                enclosing = Some(tag);
                            }
                        } else {
                            enclosing = Some(tag);
                        }
                    }
                }
            }
        }

        // Build containment path for the enclosing entity
        if let Some(enc) = enclosing {
            let mut path = Vec::new();
            let enc_start = enc.containment_start_line();
            let enc_end = enc.containment_end_line();

            // Find all parents of the enclosing definition
            for tag in definition_tags {
                let tag_start = tag.containment_start_line();
                let tag_end = tag.containment_end_line();
                if tag_start < enc_start && tag_end >= enc_end {
                    let tag_string = normalize_tag_string(&tag.tag);
                    if let Ok(info) = parse_tag_string(&tag_string) {
                        if info.node_type == NodeType::Container
                            || info.node_type == NodeType::Callable
                        {
                            path.push(tag.name.as_str());
                        }
                    }
                }
            }

            path.push(enc.name.as_str());
            generate_node_id(file, &path[..path.len() - 1], path.last().unwrap(), None)
        } else {
            // Reference is at file level
            file.to_string()
        }
    }

    /// Resolve references and create USES edges.
    fn resolve_references(
        &self,
        graph: &mut PetCodeGraph,
        defines: &HashMap<String, String>,
        references: &HashMap<String, Vec<ReferenceInfo>>,
    ) {
        info!("Creating USES relationships...");
        let mut uses_count = 0;
        let mut forward_refs = 0;
        let mut skipped_missing_source = 0;

        for (name, refs) in references {
            if let Some(target_id) = defines.get(name) {
                // Target definition found - create USES edges
                for ref_info in refs {
                    // Only create edge if source and target are different
                    if ref_info.source_id != *target_id {
                        // Only create edge if source node exists in the graph
                        if graph.contains_node(&ref_info.source_id) {
                            graph.add_edge_from_struct(&Edge::uses(
                                ref_info.source_id.clone(),
                                target_id.clone(),
                                Some(ref_info.line),
                                Some(name.clone()),
                            ));
                            uses_count += 1;
                        } else {
                            // Source node doesn't exist (e.g., reference inside impl block
                            // for a type not defined in this codebase)
                            skipped_missing_source += 1;
                            debug!(
                                "Skipped USES edge: source '{}' not found (ref to '{}')",
                                ref_info.source_id, name
                            );
                        }
                    }
                }
            } else {
                // Forward reference or external dependency
                forward_refs += refs.len();
                debug!(
                    "Forward/external reference to '{}' ({} occurrences)",
                    name,
                    refs.len()
                );
            }
        }

        info!("Created {} USES relationships", uses_count);
        if forward_refs > 0 {
            info!("Skipped {} forward/external references", forward_refs);
        }
        if skipped_missing_source > 0 {
            info!(
                "Skipped {} references with missing source nodes",
                skipped_missing_source
            );
        }
    }

    /// Parse a single file and return a graph with its entities.
    ///
    /// This method is useful for incremental updates where only specific files
    /// need to be reparsed. Returns a graph containing:
    /// - FILE node with hash
    /// - All definition nodes (Container, Callable, Data)
    /// - CONTAINS edges (parent → child)
    /// - DEFINES edges (Container/Callable → Data)
    ///
    /// Note: USES edges are NOT included because they require cross-file
    /// reference resolution. Call `resolve_references_for_file()` after
    /// merging into the main graph if needed.
    ///
    /// # Arguments
    ///
    /// * `file_path` - Absolute path to the file
    /// * `rel_path` - Relative path (used for node IDs)
    ///
    /// # Returns
    ///
    /// A `PetCodeGraph` containing entities from this file only.
    pub fn parse_file(
        &mut self,
        file_path: &Path,
        rel_path: &str,
    ) -> Result<PetCodeGraph, BuilderError> {
        let mut graph = PetCodeGraph::new();
        let mut defines = HashMap::new();
        let mut references = HashMap::new();
        let mut skipped_data = 0;
        let mut skipped_depth = 0;

        self.process_file(
            file_path,
            rel_path,
            "", // No repository context for single file parsing
            &mut graph,
            &mut defines,
            &mut references,
            &mut skipped_data,
            &mut skipped_depth,
        )?;

        debug!(
            "Parsed {}: {} nodes, {} edges",
            rel_path,
            graph.node_count(),
            graph.edge_count()
        );

        Ok(graph)
    }
}

// ============================================================================
// Component Builder
// ============================================================================

/// Information about a discovered component from a manifest file.
#[derive(Debug, Clone)]
pub struct DiscoveredComponent {
    /// Node ID for the component (e.g., "my-repo:packages/core")
    pub node_id: String,
    /// Component name from manifest (e.g., "@myorg/core")
    pub name: String,
    /// Path to the manifest file relative to repo root
    pub manifest_path: String,
    /// Directory containing the manifest (relative to repo root)
    pub directory: String,
    /// Parsed manifest info
    pub info: ManifestInfo,
}

/// Builds Component nodes and DependsOn edges from manifest files.
///
/// The `ComponentBuilder` discovers manifest files in a repository,
/// parses them to extract component metadata, and creates the component
/// graph with proper containment and dependency edges.
///
/// ## Usage
///
/// ```ignore
/// use codeprysm_core::builder::{ComponentBuilder, BuilderConfig};
/// use codeprysm_core::graph::PetCodeGraph;
/// use std::path::Path;
///
/// let mut graph = PetCodeGraph::new();
/// let mut builder = ComponentBuilder::new()?;
///
/// let components = builder.discover_components(Path::new("my-repo"), &[])?;
/// builder.add_to_graph(&mut graph, "my-repo", &components)?;
/// ```
pub struct ComponentBuilder {
    /// Reusable manifest parser
    parser: ManifestParser,
    /// Index from manifest directory path to component node ID
    path_index: HashMap<PathBuf, String>,
}

impl ComponentBuilder {
    /// Create a new component builder.
    pub fn new() -> Result<Self, BuilderError> {
        let parser = ManifestParser::new()
            .map_err(|e| BuilderError::Io(std::io::Error::other(e.to_string())))?;

        Ok(Self {
            parser,
            path_index: HashMap::new(),
        })
    }

    /// Discover all components (manifest files) in a directory.
    ///
    /// Walks the directory tree, finds manifest files, parses them,
    /// and returns information about each discovered component.
    ///
    /// # Arguments
    ///
    /// * `root` - Root directory to search
    /// * `exclude_patterns` - Glob patterns to exclude (e.g., "node_modules", "target")
    ///
    /// # Returns
    ///
    /// A list of discovered components with their manifest information.
    pub fn discover_components(
        &mut self,
        root: &Path,
        exclude_patterns: &[String],
    ) -> Result<Vec<DiscoveredComponent>, BuilderError> {
        let root = root.canonicalize().map_err(BuilderError::Io)?;
        let repo_name = get_repo_name(&root);

        let mut components = Vec::new();
        let glob_set = build_exclude_glob_set(exclude_patterns);

        info!("Discovering components in {}", root.display());

        // Use ignore::WalkBuilder which respects .gitignore and .codeprysmignore
        let walker = WalkBuilder::new(&root)
            .follow_links(false)
            .hidden(true) // Skip hidden files/directories
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .add_custom_ignore_filename(".codeprysmignore") // Respect .codeprysmignore
            .build();

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    debug!("Error walking directory: {}", e);
                    continue;
                }
            };

            // Skip directories - we only want files
            let file_type = match entry.file_type() {
                Some(ft) => ft,
                None => continue,
            };
            if !file_type.is_file() {
                continue;
            }

            let path = entry.path();

            // Check if this is a manifest file
            if ManifestLanguage::from_path(path).is_none() {
                continue;
            }

            // Check additional exclude patterns from config (beyond .gitignore/.codeprysmignore)
            let rel_path = path.strip_prefix(&root).unwrap_or(path);
            let rel_path_str = rel_path.to_string_lossy();
            if glob_set.is_match(rel_path_str.as_ref()) {
                debug!("Skipping excluded manifest: {}", rel_path_str);
                continue;
            }

            // Parse the manifest
            match self.parse_manifest_file(path, &root, &repo_name) {
                Ok(Some(component)) => {
                    debug!(
                        "Discovered component: {} at {}",
                        component.name, component.manifest_path
                    );
                    components.push(component);
                }
                Ok(None) => {
                    // Manifest parsed but no component info extracted
                    debug!("No component info in {}", rel_path_str);
                }
                Err(e) => {
                    warn!("Failed to parse manifest {}: {}", rel_path_str, e);
                }
            }
        }

        info!("Discovered {} components", components.len());
        Ok(components)
    }

    /// Parse a manifest file and create a DiscoveredComponent.
    fn parse_manifest_file(
        &mut self,
        path: &Path,
        root: &Path,
        repo_name: &str,
    ) -> Result<Option<DiscoveredComponent>, BuilderError> {
        let content = std::fs::read_to_string(path)?;
        let rel_path = path
            .strip_prefix(root)
            .unwrap_or(path)
            .to_string_lossy()
            .to_string();

        let info = self
            .parser
            .parse(path, &content)
            .map_err(|e| BuilderError::Io(std::io::Error::other(e.to_string())))?;

        // Get the manifest directory (relative to root)
        let manifest_dir = path
            .parent()
            .and_then(|p| p.strip_prefix(root).ok())
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        // Determine component name
        let name = info.component_name.clone().unwrap_or_else(|| {
            // Infer name from directory
            if manifest_dir.is_empty() {
                repo_name.to_string()
            } else {
                manifest_dir
                    .rsplit('/')
                    .find(|s| !s.is_empty())
                    .unwrap_or(&manifest_dir)
                    .to_string()
            }
        });

        // Skip empty manifests (no name, no workspace, no deps)
        if info.is_empty() && info.component_name.is_none() {
            return Ok(None);
        }

        // Generate node ID: repo_name:relative_dir or repo_name for root
        let node_id = if manifest_dir.is_empty() {
            format!("component:{}", repo_name)
        } else {
            format!(
                "component:{}:{}",
                repo_name,
                manifest_dir.replace('\\', "/")
            )
        };

        Ok(Some(DiscoveredComponent {
            node_id,
            name,
            manifest_path: rel_path,
            directory: manifest_dir,
            info,
        }))
    }

    /// Add discovered components to a graph.
    ///
    /// Creates Component nodes with proper metadata and builds the
    /// containment hierarchy and dependency edges.
    ///
    /// # Arguments
    ///
    /// * `graph` - The graph to add components to
    /// * `repo_name` - Name of the repository (for parent containment)
    /// * `components` - List of discovered components
    ///
    /// # Returns
    ///
    /// The number of Component nodes added to the graph.
    pub fn add_to_graph(
        &mut self,
        graph: &mut PetCodeGraph,
        repo_name: &str,
        components: &[DiscoveredComponent],
    ) -> Result<usize, BuilderError> {
        // Build path index for dependency resolution
        self.build_path_index(components);

        // First pass: create all Component nodes
        let mut added = 0;
        for component in components {
            self.add_component_node(graph, component);
            added += 1;
        }

        // Second pass: create CONTAINS edges for hierarchy
        self.build_containment_hierarchy(graph, repo_name, components);

        // Third pass: create DependsOn edges
        self.create_dependency_edges(graph, components);

        info!(
            "Added {} components with {} dependency edges",
            added,
            graph.edges_by_type(EdgeType::DependsOn).count()
        );

        Ok(added)
    }

    /// Build the path index for dependency resolution.
    fn build_path_index(&mut self, components: &[DiscoveredComponent]) {
        self.path_index.clear();

        for component in components {
            // Index by directory path
            let dir_path = PathBuf::from(&component.directory);
            self.path_index
                .insert(dir_path.clone(), component.node_id.clone());

            // Also index by normalized path (forward slashes)
            let normalized = component.directory.replace('\\', "/");
            if normalized != component.directory {
                self.path_index
                    .insert(PathBuf::from(normalized), component.node_id.clone());
            }
        }

        debug!("Built path index with {} entries", self.path_index.len());
    }

    /// Add a single Component node to the graph.
    fn add_component_node(&self, graph: &mut PetCodeGraph, component: &DiscoveredComponent) {
        let metadata = NodeMetadata::default().with_component(
            Some(component.info.is_workspace_root),
            Some(component.info.is_publishable()),
            Some(component.manifest_path.clone()),
        );

        let node = Node::component(
            component.node_id.clone(),
            component.name.clone(),
            component.manifest_path.clone(),
            metadata,
        );

        graph.add_node(node);
    }

    /// Build the containment hierarchy for components.
    ///
    /// - Repository CONTAINS top-level components
    /// - Workspace root CONTAINS its member components
    fn build_containment_hierarchy(
        &self,
        graph: &mut PetCodeGraph,
        repo_name: &str,
        components: &[DiscoveredComponent],
    ) {
        // Find workspace roots and their members
        let workspace_roots: Vec<_> = components
            .iter()
            .filter(|c| c.info.is_workspace_root)
            .collect();

        for component in components {
            // Determine the parent for this component
            let parent_id = self.find_parent_component(component, &workspace_roots, repo_name);

            // Only add CONTAINS edge if parent exists in graph
            if graph.contains_node(&parent_id) && graph.contains_node(&component.node_id) {
                graph.add_edge_from_struct(&Edge::contains(parent_id, component.node_id.clone()));
            }
        }
    }

    /// Find the parent for a component.
    ///
    /// Returns the workspace root if this component is a member,
    /// otherwise returns the repository node.
    fn find_parent_component(
        &self,
        component: &DiscoveredComponent,
        workspace_roots: &[&DiscoveredComponent],
        repo_name: &str,
    ) -> String {
        // Check if this component is a workspace member
        for root in workspace_roots {
            // Skip if this IS the workspace root
            if root.node_id == component.node_id {
                continue;
            }

            // Check if component directory matches any workspace member pattern
            for pattern in &root.info.workspace_members {
                if self.matches_workspace_pattern(&component.directory, pattern, &root.directory) {
                    return root.node_id.clone();
                }
            }
        }

        // Default to repository as parent
        repo_name.to_string()
    }

    /// Check if a component directory matches a workspace member pattern.
    fn matches_workspace_pattern(
        &self,
        component_dir: &str,
        pattern: &str,
        workspace_dir: &str,
    ) -> bool {
        // Normalize paths
        let component_dir = component_dir.replace('\\', "/");
        let pattern = pattern.replace('\\', "/");
        let workspace_dir = workspace_dir.replace('\\', "/");

        // Calculate the full pattern path relative to repo root
        let full_pattern = if workspace_dir.is_empty() {
            pattern.clone()
        } else {
            format!("{}/{}", workspace_dir, pattern)
        };

        // Handle glob patterns (e.g., "packages/*", "crates/*")
        if full_pattern.ends_with("/*") {
            let prefix = full_pattern.trim_end_matches("/*");
            component_dir.starts_with(prefix) && component_dir != prefix
        } else if full_pattern.contains('*') {
            // More complex glob - use simple prefix matching for now
            let prefix = full_pattern.split('*').next().unwrap_or("");
            !prefix.is_empty() && component_dir.starts_with(prefix)
        } else {
            // Exact match
            component_dir == full_pattern
        }
    }

    /// Create DependsOn edges for local dependencies.
    fn create_dependency_edges(
        &self,
        graph: &mut PetCodeGraph,
        components: &[DiscoveredComponent],
    ) {
        for component in components {
            for dep in &component.info.local_dependencies {
                if let Some(target_id) = self.resolve_dependency(component, dep) {
                    // Only create edge if both nodes exist
                    if graph.contains_node(&component.node_id) && graph.contains_node(&target_id) {
                        let version_spec = self.format_version_spec(dep);
                        let edge = Edge::depends_on(
                            component.node_id.clone(),
                            target_id,
                            Some(dep.name.clone()),
                            version_spec,
                            Some(dep.is_dev),
                        );
                        graph.add_edge_from_struct(&edge);
                    }
                } else {
                    debug!(
                        "Could not resolve dependency '{}' from {} (path: {:?})",
                        dep.name, component.node_id, dep.path
                    );
                }
            }
        }
    }

    /// Resolve a local dependency to a component node ID.
    fn resolve_dependency(
        &self,
        from: &DiscoveredComponent,
        dep: &LocalDependency,
    ) -> Option<String> {
        // First, try to resolve by path
        if let Some(ref dep_path) = dep.path {
            let mut resolved = self.resolve_dependency_path(&from.directory, dep_path);

            // For .NET ProjectReference, strip the .csproj/.vbproj/.fsproj filename
            // to get the directory containing the manifest
            if dep.dep_type == DependencyType::ProjectReference {
                if let Some(parent) = resolved.parent() {
                    resolved = parent.to_path_buf();
                }
            }

            if let Some(id) = self.path_index.get(&resolved) {
                return Some(id.clone());
            }

            // Try the path as-is
            if let Some(id) = self.path_index.get(&PathBuf::from(dep_path)) {
                return Some(id.clone());
            }
        }

        // For workspace dependencies, search by name
        if dep.dep_type == DependencyType::Workspace {
            // Search for a component with matching name
            // This is a simple linear search - could be optimized with a name index
            for (path, id) in &self.path_index {
                let dir_name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                if dir_name == dep.name || dep.name.ends_with(&format!("/{}", dir_name)) {
                    return Some(id.clone());
                }
            }
        }

        None
    }

    /// Resolve a dependency path relative to a component directory.
    fn resolve_dependency_path(&self, from_dir: &str, dep_path: &str) -> PathBuf {
        let from_dir = from_dir.replace('\\', "/");
        let dep_path = dep_path.replace('\\', "/");

        // Handle absolute paths
        if let Some(stripped) = dep_path.strip_prefix('/') {
            return PathBuf::from(stripped);
        }

        // Handle relative paths
        let from_parts: Vec<&str> = from_dir.split('/').filter(|s| !s.is_empty()).collect();
        let dep_parts: Vec<&str> = dep_path.split('/').filter(|s| !s.is_empty()).collect();

        let mut result: Vec<&str> = from_parts.clone();

        for part in dep_parts {
            match part {
                ".." => {
                    result.pop();
                }
                "." => {}
                _ => {
                    result.push(part);
                }
            }
        }

        PathBuf::from(result.join("/"))
    }

    /// Format a version spec string for a dependency.
    fn format_version_spec(&self, dep: &LocalDependency) -> Option<String> {
        match dep.dep_type {
            DependencyType::Path => dep.path.as_ref().map(|p| format!("path:{}", p)),
            DependencyType::Workspace => Some("workspace:*".to_string()),
            DependencyType::ProjectReference => dep.path.as_ref().map(|p| format!("project:{}", p)),
            DependencyType::Replace => dep.path.as_ref().map(|p| format!("replace:{}", p)),
            DependencyType::Subdirectory => dep.path.as_ref().map(|p| format!("subdir:{}", p)),
        }
    }

    /// Get the path index (for testing or inspection).
    pub fn path_index(&self) -> &HashMap<PathBuf, String> {
        &self.path_index
    }
}

/// Build a glob set from exclude patterns.
fn build_exclude_glob_set(patterns: &[String]) -> globset::GlobSet {
    let mut builder = globset::GlobSetBuilder::new();
    for pattern in patterns {
        if let Ok(glob) = globset::Glob::new(pattern) {
            builder.add(glob);
        }
    }
    // Add default excludes
    for pattern in &[
        "**/.git/**",
        "**/node_modules/**",
        "**/target/**",
        "**/__pycache__/**",
        "**/.venv/**",
        "**/venv/**",
        "**/.tox/**",
        "**/dist/**",
        "**/build/**",
    ] {
        if let Ok(glob) = globset::Glob::new(pattern) {
            builder.add(glob);
        }
    }
    builder
        .build()
        .unwrap_or_else(|_| globset::GlobSet::empty())
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Normalize a tag string to the expected format.
///
/// Handles various tag formats:
/// - `@definition.callable.function` -> `definition.callable.function`
/// - `name.definition.callable.function` -> `definition.callable.function`
fn normalize_tag_string(tag: &str) -> String {
    // Remove @ prefix if present
    let tag = tag.strip_prefix('@').unwrap_or(tag);

    // Handle `name.` prefix (tree-sitter capture name convention)
    if let Some(stripped) = tag.strip_prefix("name.") {
        return stripped.to_string();
    }

    tag.to_string()
}

/// Extract git metadata from a repository directory.
///
/// Returns (remote_url, branch, commit_sha) - all optional.
fn extract_git_metadata(repo_path: &Path) -> (Option<String>, Option<String>, Option<String>) {
    let git_dir = repo_path.join(".git");
    if !git_dir.exists() {
        return (None, None, None);
    }

    // Try to get remote URL
    let remote = std::process::Command::new("git")
        .args(["remote", "get-url", "origin"])
        .current_dir(repo_path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    // Try to get current branch
    let branch = std::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(repo_path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    // Try to get current commit SHA
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(repo_path)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty());

    (remote, branch, commit)
}

/// Extract repository name from a directory path.
fn get_repo_name(directory: &Path) -> String {
    directory
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "repository".to_string())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_tag_string() {
        assert_eq!(
            normalize_tag_string("@definition.callable.function"),
            "definition.callable.function"
        );
        assert_eq!(
            normalize_tag_string("definition.callable.function"),
            "definition.callable.function"
        );
        assert_eq!(
            normalize_tag_string("name.definition.callable.function"),
            "definition.callable.function"
        );
    }

    #[test]
    fn test_builder_config_default() {
        let config = BuilderConfig::default();
        assert!(!config.skip_data_nodes);
        assert!(config.max_containment_depth.is_none());
        assert!(config.max_files.is_none());
        assert!(!config.exclude_patterns.is_empty());
    }

    #[test]
    fn test_builder_new_missing_queries_dir() {
        let result = GraphBuilder::new(Path::new("/nonexistent/queries"));
        assert!(matches!(result, Err(BuilderError::QueryDirNotFound(_))));
    }

    // ========================================================================
    // ComponentBuilder Tests
    // ========================================================================

    #[test]
    fn test_component_builder_new() {
        let builder = ComponentBuilder::new();
        assert!(builder.is_ok());
    }

    #[test]
    fn test_resolve_dependency_path_relative() {
        let builder = ComponentBuilder::new().unwrap();

        // From packages/core, ../utils -> packages/utils
        let result = builder.resolve_dependency_path("packages/core", "../utils");
        assert_eq!(result, PathBuf::from("packages/utils"));

        // From packages/core, ./lib -> packages/core/lib
        let result = builder.resolve_dependency_path("packages/core", "./lib");
        assert_eq!(result, PathBuf::from("packages/core/lib"));

        // From empty (root), packages/shared -> packages/shared
        let result = builder.resolve_dependency_path("", "packages/shared");
        assert_eq!(result, PathBuf::from("packages/shared"));

        // Multiple .. traversals
        let result = builder.resolve_dependency_path("deep/nested/path", "../../sibling");
        assert_eq!(result, PathBuf::from("deep/sibling"));
    }

    #[test]
    fn test_resolve_dependency_path_windows() {
        let builder = ComponentBuilder::new().unwrap();

        // Windows path separators
        let result = builder.resolve_dependency_path("packages\\core", "..\\utils");
        assert_eq!(result, PathBuf::from("packages/utils"));
    }

    #[test]
    fn test_matches_workspace_pattern() {
        let builder = ComponentBuilder::new().unwrap();

        // Simple glob pattern: packages/*
        assert!(builder.matches_workspace_pattern("packages/core", "packages/*", ""));
        assert!(builder.matches_workspace_pattern("packages/utils", "packages/*", ""));
        assert!(!builder.matches_workspace_pattern("packages", "packages/*", "")); // exact match should fail
        assert!(!builder.matches_workspace_pattern("other/core", "packages/*", ""));

        // Pattern with workspace directory
        assert!(builder.matches_workspace_pattern("apps/web/core", "core", "apps/web"));

        // Nested glob: crates/*
        assert!(builder.matches_workspace_pattern("crates/codeprysm-core", "crates/*", ""));
        assert!(builder.matches_workspace_pattern("crates/codeprysm-search", "crates/*", ""));
    }

    #[test]
    fn test_format_version_spec() {
        let builder = ComponentBuilder::new().unwrap();

        let path_dep = LocalDependency::with_path(
            "my-dep".to_string(),
            "../shared".to_string(),
            DependencyType::Path,
        );
        assert_eq!(
            builder.format_version_spec(&path_dep),
            Some("path:../shared".to_string())
        );

        let workspace_dep = LocalDependency::new("my-dep".to_string(), DependencyType::Workspace);
        assert_eq!(
            builder.format_version_spec(&workspace_dep),
            Some("workspace:*".to_string())
        );

        let project_ref_dep = LocalDependency::with_path(
            "Shared".to_string(),
            "../Shared/Shared.csproj".to_string(),
            DependencyType::ProjectReference,
        );
        assert_eq!(
            builder.format_version_spec(&project_ref_dep),
            Some("project:../Shared/Shared.csproj".to_string())
        );
    }

    #[test]
    fn test_build_exclude_glob_set() {
        let default_set = build_exclude_glob_set(&[]);

        // Default excludes should match common directories
        assert!(default_set.is_match("node_modules/foo"));
        assert!(default_set.is_match("target/debug"));
        assert!(default_set.is_match(".git/objects"));
        assert!(default_set.is_match("__pycache__/module"));

        // Custom patterns
        let custom_set = build_exclude_glob_set(&["vendor/**".to_string()]);
        assert!(custom_set.is_match("vendor/github.com"));
    }

    #[test]
    fn test_discovered_component_creation() {
        let info = ManifestInfo {
            component_name: Some("my-package".to_string()),
            version: Some("1.0.0".to_string()),
            is_workspace_root: false,
            workspace_members: vec![],
            local_dependencies: vec![],
            ecosystem: Some("npm".to_string()),
        };

        let component = DiscoveredComponent {
            node_id: "component:my-repo:packages/core".to_string(),
            name: "my-package".to_string(),
            manifest_path: "packages/core/package.json".to_string(),
            directory: "packages/core".to_string(),
            info,
        };

        assert_eq!(component.node_id, "component:my-repo:packages/core");
        assert_eq!(component.name, "my-package");
        assert!(!component.info.is_workspace_root);
        assert!(component.info.is_publishable());
    }

    #[test]
    fn test_add_component_node() {
        let builder = ComponentBuilder::new().unwrap();
        let mut graph = PetCodeGraph::new();

        let info = ManifestInfo {
            component_name: Some("test-component".to_string()),
            version: Some("0.1.0".to_string()),
            is_workspace_root: true,
            workspace_members: vec!["packages/*".to_string()],
            local_dependencies: vec![],
            ecosystem: Some("cargo".to_string()),
        };

        let component = DiscoveredComponent {
            node_id: "component:test-repo".to_string(),
            name: "test-component".to_string(),
            manifest_path: "Cargo.toml".to_string(),
            directory: "".to_string(),
            info,
        };

        builder.add_component_node(&mut graph, &component);

        // Verify node was added
        assert!(graph.contains_node("component:test-repo"));
        let node = graph.get_node("component:test-repo").unwrap();
        assert_eq!(node.name, "test-component");
        assert_eq!(node.node_type, NodeType::Container);
        assert_eq!(node.kind, Some("component".to_string()));

        // Check metadata
        assert_eq!(node.metadata.is_workspace_root, Some(true));
        assert_eq!(node.metadata.is_publishable, Some(true));
        assert_eq!(node.metadata.manifest_path, Some("Cargo.toml".to_string()));
    }

    #[test]
    fn test_build_path_index() {
        let mut builder = ComponentBuilder::new().unwrap();

        let components = vec![
            DiscoveredComponent {
                node_id: "component:repo:packages/core".to_string(),
                name: "core".to_string(),
                manifest_path: "packages/core/package.json".to_string(),
                directory: "packages/core".to_string(),
                info: ManifestInfo::new(),
            },
            DiscoveredComponent {
                node_id: "component:repo:packages/utils".to_string(),
                name: "utils".to_string(),
                manifest_path: "packages/utils/package.json".to_string(),
                directory: "packages/utils".to_string(),
                info: ManifestInfo::new(),
            },
        ];

        builder.build_path_index(&components);

        let index = builder.path_index();
        assert_eq!(index.len(), 2);
        assert_eq!(
            index.get(&PathBuf::from("packages/core")),
            Some(&"component:repo:packages/core".to_string())
        );
        assert_eq!(
            index.get(&PathBuf::from("packages/utils")),
            Some(&"component:repo:packages/utils".to_string())
        );
    }

    #[test]
    fn test_resolve_dependency() {
        let mut builder = ComponentBuilder::new().unwrap();

        let components = vec![
            DiscoveredComponent {
                node_id: "component:repo:packages/core".to_string(),
                name: "core".to_string(),
                manifest_path: "packages/core/package.json".to_string(),
                directory: "packages/core".to_string(),
                info: ManifestInfo::new(),
            },
            DiscoveredComponent {
                node_id: "component:repo:packages/utils".to_string(),
                name: "utils".to_string(),
                manifest_path: "packages/utils/package.json".to_string(),
                directory: "packages/utils".to_string(),
                info: ManifestInfo::new(),
            },
        ];

        builder.build_path_index(&components);

        // Test path-based resolution
        let from = &components[0]; // packages/core
        let dep = LocalDependency::with_path(
            "utils".to_string(),
            "../utils".to_string(),
            DependencyType::Path,
        );

        let resolved = builder.resolve_dependency(from, &dep);
        assert_eq!(resolved, Some("component:repo:packages/utils".to_string()));
    }

    #[test]
    fn test_dependency_edges_created() {
        let mut builder = ComponentBuilder::new().unwrap();
        let mut graph = PetCodeGraph::new();

        let dep = LocalDependency::with_path(
            "utils".to_string(),
            "../utils".to_string(),
            DependencyType::Path,
        );

        let mut core_info = ManifestInfo::new();
        core_info.component_name = Some("core".to_string());
        core_info.local_dependencies.push(dep);

        let mut utils_info = ManifestInfo::new();
        utils_info.component_name = Some("utils".to_string());

        let components = vec![
            DiscoveredComponent {
                node_id: "component:repo:packages/core".to_string(),
                name: "core".to_string(),
                manifest_path: "packages/core/package.json".to_string(),
                directory: "packages/core".to_string(),
                info: core_info,
            },
            DiscoveredComponent {
                node_id: "component:repo:packages/utils".to_string(),
                name: "utils".to_string(),
                manifest_path: "packages/utils/package.json".to_string(),
                directory: "packages/utils".to_string(),
                info: utils_info,
            },
        ];

        builder
            .add_to_graph(&mut graph, "repo", &components)
            .unwrap();

        // Verify DependsOn edge was created
        let deps_edges: Vec<_> = graph.edges_by_type(EdgeType::DependsOn).collect();
        assert_eq!(deps_edges.len(), 1);

        let (source, target, data) = &deps_edges[0];
        assert_eq!(source.id, "component:repo:packages/core");
        assert_eq!(target.id, "component:repo:packages/utils");
        assert_eq!(data.ident, Some("utils".to_string()));
        assert_eq!(data.version_spec, Some("path:../utils".to_string()));
    }
}
