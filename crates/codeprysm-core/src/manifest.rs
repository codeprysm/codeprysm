//! Manifest Parser for Component Extraction
//!
//! This module parses manifest files (package.json, Cargo.toml, go.mod, etc.)
//! to extract component metadata and local dependencies for building the
//! component graph with DependsOn edges.
//!
//! ## Supported Manifest Files
//!
//! | Filename | Language | Ecosystem |
//! |----------|----------|-----------|
//! | package.json | Json | npm/Node.js |
//! | vcpkg.json | Json | vcpkg (C/C++) |
//! | Cargo.toml | Toml | Rust |
//! | pyproject.toml | Toml | Python |
//! | go.mod | GoMod | Go |
//! | *.csproj/*.vbproj/*.fsproj | Xml | .NET |
//! | CMakeLists.txt | CMake | CMake |
//!
//! ## Usage
//!
//! ```rust,ignore
//! use codeprysm_core::manifest::{ManifestParser, ManifestInfo};
//! use std::path::Path;
//!
//! let content = r#"{"name": "my-package", "version": "1.0.0"}"#;
//! let path = Path::new("package.json");
//!
//! let mut parser = ManifestParser::new()?;
//! let info = parser.parse(path, content)?;
//! println!("Component: {:?}", info.component_name);
//! ```

use std::path::Path;

use thiserror::Error;
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

use crate::embedded_queries::get_manifest_query;
use crate::parser::ManifestLanguage;

// ============================================================================
// Errors
// ============================================================================

/// Errors that can occur during manifest parsing.
#[derive(Debug, Error)]
pub enum ManifestError {
    /// Manifest file type not recognized
    #[error("Unrecognized manifest file: {0}")]
    UnrecognizedManifest(String),

    /// Failed to parse manifest content
    #[error("Failed to parse manifest: {0}")]
    ParseFailed(String),

    /// Failed to compile query
    #[error("Failed to compile manifest query: {0}")]
    QueryCompileFailed(String),

    /// Failed to set parser language
    #[error("Failed to set parser language: {0}")]
    LanguageSetFailed(String),
}

// ============================================================================
// Dependency Types
// ============================================================================

/// Type of local dependency reference.
///
/// These are the types of dependencies that create DependsOn edges
/// between components in the graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum DependencyType {
    /// Path-based dependency (Cargo: `{ path = "../sibling" }`)
    Path,
    /// Workspace dependency (npm: `workspace:*`, Cargo workspace member)
    Workspace,
    /// Project reference (.NET: `<ProjectReference Include="..." />`)
    ProjectReference,
    /// Replace directive (Go: `replace module => ../local`)
    Replace,
    /// Subdirectory dependency (CMake: `add_subdirectory(../shared)`)
    Subdirectory,
}

impl DependencyType {
    /// Get string representation
    pub fn as_str(&self) -> &'static str {
        match self {
            DependencyType::Path => "path",
            DependencyType::Workspace => "workspace",
            DependencyType::ProjectReference => "project_reference",
            DependencyType::Replace => "replace",
            DependencyType::Subdirectory => "subdirectory",
        }
    }
}

impl std::fmt::Display for DependencyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

// ============================================================================
// Local Dependency
// ============================================================================

/// A local dependency extracted from a manifest file.
///
/// Local dependencies are references to other components in the same
/// repository or workspace, which create DependsOn edges in the graph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalDependency {
    /// Name of the dependency (package/crate/module name)
    pub name: String,
    /// Relative path to the dependency (if specified)
    pub path: Option<String>,
    /// Type of dependency reference
    pub dep_type: DependencyType,
    /// Whether this is a dev dependency
    pub is_dev: bool,
    /// Whether this is a build dependency
    pub is_build: bool,
    /// Original version/spec string (for context)
    pub version_spec: Option<String>,
}

impl LocalDependency {
    /// Create a new local dependency
    pub fn new(name: String, dep_type: DependencyType) -> Self {
        Self {
            name,
            path: None,
            dep_type,
            is_dev: false,
            is_build: false,
            version_spec: None,
        }
    }

    /// Create with a path
    pub fn with_path(name: String, path: String, dep_type: DependencyType) -> Self {
        Self {
            name,
            path: Some(path),
            dep_type,
            is_dev: false,
            is_build: false,
            version_spec: None,
        }
    }

    /// Set as dev dependency
    pub fn as_dev(mut self) -> Self {
        self.is_dev = true;
        self
    }

    /// Set as build dependency
    pub fn as_build(mut self) -> Self {
        self.is_build = true;
        self
    }

    /// Set version spec
    pub fn with_version(mut self, version: String) -> Self {
        self.version_spec = Some(version);
        self
    }
}

// ============================================================================
// Manifest Info
// ============================================================================

/// Information extracted from a manifest file.
///
/// Contains component metadata and local dependencies for graph construction.
#[derive(Debug, Clone, Default)]
pub struct ManifestInfo {
    /// Component name (package/crate/module name)
    pub component_name: Option<String>,
    /// Component version
    pub version: Option<String>,
    /// Whether this is a workspace root
    pub is_workspace_root: bool,
    /// Workspace member paths (for workspace roots)
    pub workspace_members: Vec<String>,
    /// Local dependencies that create DependsOn edges
    pub local_dependencies: Vec<LocalDependency>,
    /// Ecosystem identifier (npm, cargo, python, go, dotnet, cmake)
    pub ecosystem: Option<String>,
}

impl ManifestInfo {
    /// Create an empty ManifestInfo
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any useful information was extracted
    pub fn is_empty(&self) -> bool {
        self.component_name.is_none()
            && self.version.is_none()
            && !self.is_workspace_root
            && self.workspace_members.is_empty()
            && self.local_dependencies.is_empty()
    }

    /// Check if this manifest defines a publishable component
    pub fn is_publishable(&self) -> bool {
        self.component_name.is_some()
    }
}

// ============================================================================
// Manifest Parser
// ============================================================================

/// Parser for extracting component information from manifest files.
///
/// Uses tree-sitter with embedded SCM queries to parse various manifest
/// file formats and extract component names, versions, and local dependencies.
pub struct ManifestParser {
    /// Reusable tree-sitter parser
    parser: Parser,
}

impl ManifestParser {
    /// Create a new manifest parser.
    pub fn new() -> Result<Self, ManifestError> {
        Ok(Self {
            parser: Parser::new(),
        })
    }

    /// Parse a manifest file and extract component information.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the manifest file (used to detect language)
    /// * `content` - Content of the manifest file
    ///
    /// # Returns
    ///
    /// `ManifestInfo` containing extracted component metadata and dependencies.
    pub fn parse(&mut self, path: &Path, content: &str) -> Result<ManifestInfo, ManifestError> {
        // Detect manifest language from filename
        let language = ManifestLanguage::from_path(path)
            .ok_or_else(|| ManifestError::UnrecognizedManifest(path.display().to_string()))?;

        self.parse_with_language(content, language)
    }

    /// Parse manifest content with a known language.
    ///
    /// # Arguments
    ///
    /// * `content` - Content of the manifest file
    /// * `language` - The manifest language to use for parsing
    ///
    /// # Returns
    ///
    /// `ManifestInfo` containing extracted component metadata and dependencies.
    pub fn parse_with_language(
        &mut self,
        content: &str,
        language: ManifestLanguage,
    ) -> Result<ManifestInfo, ManifestError> {
        // Set up parser with manifest grammar
        self.parser
            .set_language(&language.tree_sitter_language())
            .map_err(|e| ManifestError::LanguageSetFailed(e.to_string()))?;

        // Parse content into AST
        let tree = self
            .parser
            .parse(content, None)
            .ok_or_else(|| ManifestError::ParseFailed("tree-sitter parse returned None".into()))?;

        // Compile manifest query
        let query_source = get_manifest_query(language);
        let query = Query::new(&language.tree_sitter_language(), query_source)
            .map_err(|e| ManifestError::QueryCompileFailed(format!("{:?}", e)))?;

        // Run query and collect captures
        let mut cursor = QueryCursor::new();
        let source_bytes = content.as_bytes();
        let capture_names = query.capture_names();

        let mut info = ManifestInfo::new();
        info.ecosystem = Some(language.as_str().to_string());

        // Track paired captures for path dependencies
        let mut pending_path_deps: std::collections::HashMap<String, PendingPathDep> =
            std::collections::HashMap::new();

        let mut matches = cursor.matches(&query, tree.root_node(), source_bytes);
        while let Some(match_) = matches.next() {
            for capture in match_.captures {
                let capture_name = &capture_names[capture.index as usize];
                let text = capture
                    .node
                    .utf8_text(source_bytes)
                    .unwrap_or("")
                    .trim_matches('"')
                    .to_string();

                self.process_capture(capture_name, &text, &mut info, &mut pending_path_deps);
            }
        }

        // Finalize any pending path dependencies
        for (_, pending) in pending_path_deps {
            if let (Some(name), Some(path)) = (pending.name, pending.path) {
                let mut dep = LocalDependency::with_path(name, path, DependencyType::Path);
                dep.is_dev = pending.is_dev;
                dep.is_build = pending.is_build;
                info.local_dependencies.push(dep);
            }
        }

        Ok(info)
    }

    /// Process a single capture from the query results.
    fn process_capture(
        &self,
        capture_name: &str,
        text: &str,
        info: &mut ManifestInfo,
        pending_path_deps: &mut std::collections::HashMap<String, PendingPathDep>,
    ) {
        // Skip internal captures (starting with _)
        if capture_name.starts_with('_') {
            return;
        }

        // Parse capture name to determine what was captured
        // Format: manifest.{element}.{ecosystem}[.{modifier}]
        let parts: Vec<&str> = capture_name.split('.').collect();
        if parts.is_empty() || parts[0] != "manifest" {
            return;
        }

        // Handle different capture patterns
        match parts.get(1).copied() {
            Some("component") => self.process_component_capture(&parts[2..], text, info),
            Some("workspace") => self.process_workspace_capture(&parts[2..], text, info),
            Some("dependency") => {
                self.process_dependency_capture(&parts[2..], text, info, pending_path_deps)
            }
            _ => {}
        }
    }

    /// Process component-related captures (name, version, namespace).
    fn process_component_capture(&self, parts: &[&str], text: &str, info: &mut ManifestInfo) {
        match parts.first().copied() {
            Some("name") => {
                // manifest.component.name.{ecosystem}
                if info.component_name.is_none() {
                    info.component_name = Some(text.to_string());
                }
            }
            Some("version") => {
                // manifest.component.version.{ecosystem}
                if info.version.is_none() {
                    info.version = Some(text.to_string());
                }
            }
            Some("namespace") => {
                // manifest.component.namespace.{ecosystem} (fallback for .NET)
                if info.component_name.is_none() {
                    info.component_name = Some(text.to_string());
                }
            }
            Some("packageversion") => {
                // manifest.component.packageversion.{ecosystem}
                if info.version.is_none() {
                    info.version = Some(text.to_string());
                }
            }
            _ => {}
        }
    }

    /// Process workspace-related captures (root, member).
    fn process_workspace_capture(&self, parts: &[&str], text: &str, info: &mut ManifestInfo) {
        match parts.first().copied() {
            Some("root") => {
                // manifest.workspace.root.{ecosystem}
                info.is_workspace_root = true;
            }
            Some("member") => {
                // manifest.workspace.member.{ecosystem}
                info.workspace_members.push(text.to_string());
            }
            _ => {}
        }
    }

    /// Process dependency-related captures.
    fn process_dependency_capture(
        &self,
        parts: &[&str],
        text: &str,
        info: &mut ManifestInfo,
        pending_path_deps: &mut std::collections::HashMap<String, PendingPathDep>,
    ) {
        if parts.is_empty() {
            return;
        }

        // Determine ecosystem from first part after "dependency"
        let ecosystem = parts[0];
        let remaining = &parts[1..];

        match ecosystem {
            "cargo" => self.process_cargo_dependency(remaining, text, info, pending_path_deps),
            "gomod" => self.process_gomod_dependency(remaining, text, info),
            "poetry" => self.process_poetry_dependency(remaining, text, info, pending_path_deps),
            "local" => self.process_local_dependency(remaining, text, info, pending_path_deps),
            "projectref" => self.process_projectref_dependency(remaining, text, info),
            "cmake" => self.process_cmake_dependency(remaining, text, info),
            _ => {}
        }
    }

    /// Process Cargo.toml dependencies.
    fn process_cargo_dependency(
        &self,
        parts: &[&str],
        text: &str,
        info: &mut ManifestInfo,
        pending_path_deps: &mut std::collections::HashMap<String, PendingPathDep>,
    ) {
        // Patterns:
        // - path.name, path.value (path dependencies)
        // - path.dev.name, path.dev.value (dev path dependencies)
        // - name, version (regular dependencies - not local)
        // - dev.name, dev.version (dev dependencies - not local)

        match parts {
            ["path", "name"] => {
                let key = format!("cargo:path:{}", text);
                pending_path_deps.entry(key).or_default().name = Some(text.to_string());
            }
            ["path", "value"] => {
                // Need to find the matching name - use a different approach
                // For now, create a dependency with just the path
                // The name will be inferred from the path
                let name = infer_name_from_path(text);
                info.local_dependencies.push(LocalDependency::with_path(
                    name,
                    text.to_string(),
                    DependencyType::Path,
                ));
            }
            ["path", "dev", "name"] => {
                let key = format!("cargo:path:dev:{}", text);
                let entry = pending_path_deps.entry(key).or_default();
                entry.name = Some(text.to_string());
                entry.is_dev = true;
            }
            ["path", "dev", "value"] => {
                let name = infer_name_from_path(text);
                info.local_dependencies.push(
                    LocalDependency::with_path(name, text.to_string(), DependencyType::Path)
                        .as_dev(),
                );
            }
            _ => {} // Ignore non-local dependencies
        }
    }

    /// Process go.mod dependencies.
    fn process_gomod_dependency(&self, parts: &[&str], text: &str, info: &mut ManifestInfo) {
        // Patterns:
        // - replace.local (local path from replace directive)
        // - replace.from (module being replaced)
        // - replace.multi.local, replace.multi.from (multi-line replace)

        match parts {
            ["replace", "local"] | ["replace", "multi", "local"] => {
                // Local file path replacement
                let name = infer_name_from_path(text);
                info.local_dependencies.push(LocalDependency::with_path(
                    name,
                    text.to_string(),
                    DependencyType::Replace,
                ));
            }
            _ => {} // Ignore non-local dependencies
        }
    }

    /// Process Poetry path dependencies.
    fn process_poetry_dependency(
        &self,
        parts: &[&str],
        text: &str,
        info: &mut ManifestInfo,
        pending_path_deps: &mut std::collections::HashMap<String, PendingPathDep>,
    ) {
        match parts {
            ["path", "name"] => {
                let key = format!("poetry:path:{}", text);
                pending_path_deps.entry(key).or_default().name = Some(text.to_string());
            }
            ["path", "value"] => {
                let name = infer_name_from_path(text);
                info.local_dependencies.push(LocalDependency::with_path(
                    name,
                    text.to_string(),
                    DependencyType::Path,
                ));
            }
            _ => {}
        }
    }

    /// Process npm local dependencies (workspace:*, file:, link:).
    fn process_local_dependency(
        &self,
        parts: &[&str],
        text: &str,
        info: &mut ManifestInfo,
        pending_path_deps: &mut std::collections::HashMap<String, PendingPathDep>,
    ) {
        match parts {
            ["name"] => {
                // Track the name for pairing with version
                let key = "npm:local:pending".to_string();
                pending_path_deps.entry(key).or_default().name = Some(text.to_string());
            }
            ["version"] => {
                // Get the pending name
                let pending_name = pending_path_deps
                    .remove("npm:local:pending")
                    .and_then(|p| p.name);

                // Check for local dependency markers
                if text.starts_with("workspace:") {
                    // Workspace dependency - use captured name
                    let name = pending_name.unwrap_or_else(|| {
                        // Fallback to the version string without prefix
                        text.trim_start_matches("workspace:").to_string()
                    });
                    info.local_dependencies
                        .push(LocalDependency::new(name, DependencyType::Workspace));
                } else if text.starts_with("file:") || text.starts_with("link:") {
                    let path = text.trim_start_matches("file:").trim_start_matches("link:");
                    let name = pending_name.unwrap_or_else(|| infer_name_from_path(path));
                    info.local_dependencies.push(LocalDependency::with_path(
                        name,
                        path.to_string(),
                        DependencyType::Path,
                    ));
                }
            }
            _ => {}
        }
    }

    /// Process .NET ProjectReference dependencies.
    fn process_projectref_dependency(&self, parts: &[&str], text: &str, info: &mut ManifestInfo) {
        if let ["dotnet"] = parts {
            // <ProjectReference Include="..\..\Shared\Shared.csproj" />
            // The text is the Include attribute value (relative path)
            let name = infer_name_from_csproj_path(text);
            info.local_dependencies.push(LocalDependency::with_path(
                name,
                text.to_string(),
                DependencyType::ProjectReference,
            ));
        }
    }

    /// Process CMake subdirectory dependencies.
    fn process_cmake_dependency(&self, parts: &[&str], text: &str, info: &mut ManifestInfo) {
        if let ["subdirectory"] = parts {
            // add_subdirectory(../shared) or add_subdirectory(libs/utils)
            let name = infer_name_from_path(text);
            info.local_dependencies.push(LocalDependency::with_path(
                name,
                text.to_string(),
                DependencyType::Subdirectory,
            ));
        }
    }
}

// ============================================================================
// Helper Types
// ============================================================================

/// Pending path dependency waiting for paired captures.
#[derive(Debug, Default)]
struct PendingPathDep {
    name: Option<String>,
    path: Option<String>,
    is_dev: bool,
    is_build: bool,
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Infer component name from a path.
///
/// Takes the last path component as the name.
fn infer_name_from_path(path: &str) -> String {
    // Normalize path separators
    let path = path.replace('\\', "/");

    // Get last path component
    path.rsplit('/')
        .find(|s| !s.is_empty())
        .unwrap_or(&path)
        .to_string()
}

/// Infer component name from a .csproj path.
///
/// Removes the .csproj extension and takes the last path component.
fn infer_name_from_csproj_path(path: &str) -> String {
    let path = path.replace('\\', "/");
    let filename = path.rsplit('/').find(|s| !s.is_empty()).unwrap_or(&path);

    // Remove .csproj/.vbproj/.fsproj extension
    filename
        .strip_suffix(".csproj")
        .or_else(|| filename.strip_suffix(".vbproj"))
        .or_else(|| filename.strip_suffix(".fsproj"))
        .unwrap_or(filename)
        .to_string()
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // ManifestParser Tests
    // ========================================================================

    #[test]
    fn test_parse_package_json_simple() {
        let content = r#"{"name": "my-package", "version": "1.0.0"}"#;
        let path = Path::new("package.json");

        let mut parser = ManifestParser::new().unwrap();
        let info = parser.parse(path, content).unwrap();

        assert_eq!(info.component_name, Some("my-package".to_string()));
        assert_eq!(info.version, Some("1.0.0".to_string()));
        assert!(!info.is_workspace_root);
    }

    #[test]
    fn test_parse_package_json_with_workspaces() {
        let content = r#"{
            "name": "monorepo",
            "version": "1.0.0",
            "workspaces": ["packages/*", "apps/*"]
        }"#;
        let path = Path::new("package.json");

        let mut parser = ManifestParser::new().unwrap();
        let info = parser.parse(path, content).unwrap();

        assert_eq!(info.component_name, Some("monorepo".to_string()));
        assert!(info.workspace_members.contains(&"packages/*".to_string()));
        assert!(info.workspace_members.contains(&"apps/*".to_string()));
    }

    #[test]
    fn test_parse_cargo_toml_simple() {
        let content = r#"
[package]
name = "my-crate"
version = "0.1.0"
"#;
        let path = Path::new("Cargo.toml");

        let mut parser = ManifestParser::new().unwrap();
        let info = parser.parse(path, content).unwrap();

        assert_eq!(info.component_name, Some("my-crate".to_string()));
        assert_eq!(info.version, Some("0.1.0".to_string()));
    }

    #[test]
    fn test_parse_cargo_toml_workspace() {
        let content = r#"
[workspace]
members = ["crates/*", "examples/*"]
"#;
        let path = Path::new("Cargo.toml");

        let mut parser = ManifestParser::new().unwrap();
        let info = parser.parse(path, content).unwrap();

        assert!(info.is_workspace_root);
        assert!(info.workspace_members.contains(&"crates/*".to_string()));
        assert!(info.workspace_members.contains(&"examples/*".to_string()));
    }

    #[test]
    fn test_parse_go_mod() {
        let content = r#"module github.com/myorg/myproject

go 1.21
"#;
        let path = Path::new("go.mod");

        let mut parser = ManifestParser::new().unwrap();
        let info = parser.parse(path, content).unwrap();

        assert_eq!(
            info.component_name,
            Some("github.com/myorg/myproject".to_string())
        );
    }

    #[test]
    fn test_parse_go_mod_with_replace() {
        let content = r#"module github.com/myorg/myproject

go 1.21

replace github.com/myorg/shared => ../shared
"#;
        let path = Path::new("go.mod");

        let mut parser = ManifestParser::new().unwrap();
        let info = parser.parse(path, content).unwrap();

        assert_eq!(info.local_dependencies.len(), 1);
        let dep = &info.local_dependencies[0];
        assert_eq!(dep.path, Some("../shared".to_string()));
        assert_eq!(dep.dep_type, DependencyType::Replace);
    }

    #[test]
    fn test_parse_csproj() {
        let content = r#"<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <AssemblyName>MyProject</AssemblyName>
    <Version>1.0.0</Version>
  </PropertyGroup>
  <ItemGroup>
    <ProjectReference Include="..\Shared\Shared.csproj" />
  </ItemGroup>
</Project>"#;
        let path = Path::new("MyProject.csproj");

        let mut parser = ManifestParser::new().unwrap();
        let info = parser.parse(path, content).unwrap();

        assert_eq!(info.component_name, Some("MyProject".to_string()));
        assert_eq!(info.version, Some("1.0.0".to_string()));
        assert_eq!(info.local_dependencies.len(), 1);

        let dep = &info.local_dependencies[0];
        assert_eq!(dep.name, "Shared");
        assert_eq!(dep.dep_type, DependencyType::ProjectReference);
    }

    #[test]
    fn test_parse_cmake() {
        let content = r#"cmake_minimum_required(VERSION 3.20)
project(my-project VERSION 1.0.0)
add_subdirectory(../shared shared_build)
"#;
        let path = Path::new("CMakeLists.txt");

        let mut parser = ManifestParser::new().unwrap();
        let info = parser.parse(path, content).unwrap();

        assert_eq!(info.component_name, Some("my-project".to_string()));
        // CMake query captures all arguments from add_subdirectory
        // (both source_dir and optional binary_dir)
        assert!(!info.local_dependencies.is_empty());

        // First captured should be the source directory
        let dep = info
            .local_dependencies
            .iter()
            .find(|d| d.path.as_deref() == Some("../shared"))
            .expect("Should find ../shared dependency");
        assert_eq!(dep.dep_type, DependencyType::Subdirectory);
    }

    #[test]
    fn test_parse_pyproject_toml() {
        let content = r#"
[project]
name = "my-package"
version = "1.0.0"
"#;
        let path = Path::new("pyproject.toml");

        let mut parser = ManifestParser::new().unwrap();
        let info = parser.parse(path, content).unwrap();

        assert_eq!(info.component_name, Some("my-package".to_string()));
        assert_eq!(info.version, Some("1.0.0".to_string()));
    }

    // ========================================================================
    // Helper Function Tests
    // ========================================================================

    #[test]
    fn test_infer_name_from_path() {
        assert_eq!(infer_name_from_path("../shared"), "shared");
        assert_eq!(infer_name_from_path("packages/core"), "core");
        assert_eq!(infer_name_from_path("..\\shared"), "shared");
        assert_eq!(infer_name_from_path("./libs/utils"), "utils");
        assert_eq!(infer_name_from_path("sibling"), "sibling");
    }

    #[test]
    fn test_infer_name_from_csproj_path() {
        assert_eq!(
            infer_name_from_csproj_path("..\\Shared\\Shared.csproj"),
            "Shared"
        );
        assert_eq!(infer_name_from_csproj_path("../Core/Core.vbproj"), "Core");
        assert_eq!(infer_name_from_csproj_path("Utils.fsproj"), "Utils");
    }

    // ========================================================================
    // DependencyType Tests
    // ========================================================================

    #[test]
    fn test_dependency_type_as_str() {
        assert_eq!(DependencyType::Path.as_str(), "path");
        assert_eq!(DependencyType::Workspace.as_str(), "workspace");
        assert_eq!(
            DependencyType::ProjectReference.as_str(),
            "project_reference"
        );
        assert_eq!(DependencyType::Replace.as_str(), "replace");
        assert_eq!(DependencyType::Subdirectory.as_str(), "subdirectory");
    }

    #[test]
    fn test_dependency_type_display() {
        assert_eq!(format!("{}", DependencyType::Path), "path");
        assert_eq!(format!("{}", DependencyType::Workspace), "workspace");
    }

    // ========================================================================
    // LocalDependency Tests
    // ========================================================================

    #[test]
    fn test_local_dependency_new() {
        let dep = LocalDependency::new("my-dep".to_string(), DependencyType::Path);
        assert_eq!(dep.name, "my-dep");
        assert_eq!(dep.dep_type, DependencyType::Path);
        assert!(!dep.is_dev);
        assert!(!dep.is_build);
        assert!(dep.path.is_none());
    }

    #[test]
    fn test_local_dependency_with_path() {
        let dep = LocalDependency::with_path(
            "my-dep".to_string(),
            "../sibling".to_string(),
            DependencyType::Path,
        );
        assert_eq!(dep.path, Some("../sibling".to_string()));
    }

    #[test]
    fn test_local_dependency_as_dev() {
        let dep = LocalDependency::new("my-dep".to_string(), DependencyType::Path).as_dev();
        assert!(dep.is_dev);
        assert!(!dep.is_build);
    }

    #[test]
    fn test_local_dependency_as_build() {
        let dep = LocalDependency::new("my-dep".to_string(), DependencyType::Path).as_build();
        assert!(!dep.is_dev);
        assert!(dep.is_build);
    }

    // ========================================================================
    // ManifestInfo Tests
    // ========================================================================

    #[test]
    fn test_manifest_info_is_empty() {
        let info = ManifestInfo::new();
        assert!(info.is_empty());

        let mut info_with_name = ManifestInfo::new();
        info_with_name.component_name = Some("test".to_string());
        assert!(!info_with_name.is_empty());
    }

    #[test]
    fn test_manifest_info_is_publishable() {
        let mut info = ManifestInfo::new();
        assert!(!info.is_publishable());

        info.component_name = Some("my-package".to_string());
        assert!(info.is_publishable());
    }

    // ========================================================================
    // Unrecognized Manifest Tests
    // ========================================================================

    #[test]
    fn test_unrecognized_manifest() {
        let mut parser = ManifestParser::new().unwrap();
        let result = parser.parse(Path::new("README.md"), "# Hello");
        assert!(matches!(
            result,
            Err(ManifestError::UnrecognizedManifest(_))
        ));
    }
}
