//! Root Discovery Module
//!
//! Discovers git repositories and code directories under a workspace root.
//! Used for multi-root workspace support.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use thiserror::Error;
use tracing::{debug, info, warn};
use walkdir::WalkDir;

use crate::parser::SupportedLanguage;

/// Errors during root discovery
#[derive(Debug, Error)]
pub enum DiscoveryError {
    #[error("Root path does not exist: {0}")]
    RootNotFound(PathBuf),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("No code roots found under {0}")]
    NoRootsFound(PathBuf),
}

pub type Result<T> = std::result::Result<T, DiscoveryError>;

/// Type of discovered root
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RootType {
    /// Git repository with optional metadata
    GitRepository {
        remote: Option<String>,
        branch: Option<String>,
        commit: Option<String>,
    },
    /// Directory containing source files but no .git
    CodeDirectory,
}

impl RootType {
    /// Check if this is a git repository
    pub fn is_git(&self) -> bool {
        matches!(self, RootType::GitRepository { .. })
    }
}

/// A discovered code root
#[derive(Debug, Clone)]
pub struct DiscoveredRoot {
    /// Absolute path to the root
    pub path: PathBuf,
    /// Relative path from workspace root
    pub relative_path: String,
    /// Type of root (git repo or code directory)
    pub root_type: RootType,
    /// Name derived from directory name
    pub name: String,
}

impl DiscoveredRoot {
    /// Check if this is a git repository
    pub fn is_git(&self) -> bool {
        self.root_type.is_git()
    }
}

/// Configuration for root discovery
#[derive(Debug, Clone)]
pub struct DiscoveryConfig {
    /// Maximum depth to search for roots
    pub max_depth: usize,
    /// Directories to skip during search
    pub exclude_dirs: HashSet<String>,
    /// Whether to include non-git code directories
    pub include_code_dirs: bool,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        let exclude_dirs: HashSet<String> = [
            "node_modules",
            "target",
            "build",
            "dist",
            "__pycache__",
            ".venv",
            "venv",
            ".idea",
            ".vscode",
            "vendor",
            "bin",
            "obj",
            ".tox",
            ".mypy_cache",
            ".pytest_cache",
            ".coverage",
            "coverage",
            ".next",
            ".nuxt",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        Self {
            max_depth: 3,
            exclude_dirs,
            include_code_dirs: true,
        }
    }
}

/// Root discovery service
pub struct RootDiscovery {
    config: DiscoveryConfig,
}

impl Default for RootDiscovery {
    fn default() -> Self {
        Self::with_defaults()
    }
}

impl RootDiscovery {
    /// Create a new RootDiscovery with custom configuration
    pub fn new(config: DiscoveryConfig) -> Self {
        Self { config }
    }

    /// Create a new RootDiscovery with default configuration
    pub fn with_defaults() -> Self {
        Self::new(DiscoveryConfig::default())
    }

    /// Create with a custom max depth
    pub fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.config.max_depth = max_depth;
        self
    }

    /// Discover all code roots under the given path
    ///
    /// Returns a list of discovered roots. If the root path itself is a git repo
    /// or contains source files, it will be the only root returned.
    pub fn discover(&self, root_path: &Path) -> Result<Vec<DiscoveredRoot>> {
        let root_path = root_path
            .canonicalize()
            .map_err(|_| DiscoveryError::RootNotFound(root_path.to_path_buf()))?;

        info!("Discovering code roots under {:?}", root_path);

        // Check if root itself is a git repo
        if self.is_git_repo(&root_path) {
            info!("Root is a git repository");
            return Ok(vec![self.create_discovered_root(&root_path, &root_path)?]);
        }

        // Check if root has source files but no subdirectories to search
        if self.has_source_files(&root_path) && !self.has_discoverable_subdirs(&root_path) {
            info!("Root is a code directory");
            return Ok(vec![self.create_discovered_root(&root_path, &root_path)?]);
        }

        let mut roots = Vec::new();
        let mut discovered_paths: HashSet<PathBuf> = HashSet::new();

        // Walk the directory tree looking for git repos and code directories
        for entry in WalkDir::new(&root_path)
            .max_depth(self.config.max_depth)
            .into_iter()
            .filter_entry(|e| {
                if !e.file_type().is_dir() {
                    return true;
                }
                // Always allow the root entry (depth 0) to be traversed
                // This handles temp directories with names like ".tmpXXXXXX"
                if e.depth() == 0 {
                    return true;
                }
                let name = e.file_name().to_string_lossy();
                // Skip hidden directories and excluded directories
                !name.starts_with('.') && !self.config.exclude_dirs.contains(name.as_ref())
            })
        {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!("Error walking directory: {}", e);
                    continue;
                }
            };

            if !entry.file_type().is_dir() {
                continue;
            }

            let path = entry.path();

            // Skip the root itself (we already checked it)
            if path == root_path {
                continue;
            }

            // Skip if this path is under an already-discovered root
            if discovered_paths.iter().any(|p| path.starts_with(p)) {
                continue;
            }

            // Check if this is a git repo
            if self.is_git_repo(path) {
                debug!("Found git repository: {:?}", path);
                if let Ok(root) = self.create_discovered_root(path, &root_path) {
                    discovered_paths.insert(path.to_path_buf());
                    roots.push(root);
                }
                continue;
            }

            // Check for code directories only if enabled and at appropriate depth
            if self.config.include_code_dirs {
                // Only consider as a code dir if it has source files
                // and is not nested under another potential code dir
                let is_nested = roots.iter().any(|r| path.starts_with(&r.path));
                if !is_nested && self.has_source_files(path) {
                    // Don't add intermediate directories as code dirs if they have
                    // subdirectories that might be git repos
                    if !self.has_git_subdirs(path) {
                        debug!("Found code directory: {:?}", path);
                        if let Ok(root) = self.create_discovered_root(path, &root_path) {
                            roots.push(root);
                        }
                    }
                }
            }
        }

        // If no roots found and root has source files, treat root as code directory
        if roots.is_empty() && self.has_source_files(&root_path) {
            info!("No sub-roots found, treating root as code directory");
            roots.push(self.create_discovered_root(&root_path, &root_path)?);
        }

        if roots.is_empty() {
            return Err(DiscoveryError::NoRootsFound(root_path));
        }

        // Sort roots by path for deterministic ordering
        roots.sort_by(|a, b| a.path.cmp(&b.path));

        info!("Discovered {} code root(s)", roots.len());
        for root in &roots {
            info!(
                "  - {} ({:?}) at {}",
                root.name,
                if root.is_git() { "git" } else { "code" },
                root.relative_path
            );
        }

        Ok(roots)
    }

    /// Check if a directory is a git repository
    fn is_git_repo(&self, path: &Path) -> bool {
        path.join(".git").exists()
    }

    /// Check if a directory has any discoverable subdirectories
    fn has_discoverable_subdirs(&self, path: &Path) -> bool {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let name = entry.file_name().to_string_lossy().to_string();
                    if !name.starts_with('.') && !self.config.exclude_dirs.contains(&name) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if a directory has any git repos in its subdirectories
    fn has_git_subdirs(&self, path: &Path) -> bool {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                    let subpath = entry.path();
                    if self.is_git_repo(&subpath) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if a directory directly contains supported source files
    fn has_source_files(&self, path: &Path) -> bool {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let entry_path = entry.path();
                if entry_path.is_file() && SupportedLanguage::from_path(&entry_path).is_some() {
                    return true;
                }
            }
        }
        false
    }

    /// Create a DiscoveredRoot from a path
    fn create_discovered_root(&self, path: &Path, root_path: &Path) -> Result<DiscoveredRoot> {
        let relative_path = path
            .strip_prefix(root_path)
            .map(|p| {
                let s = p.to_string_lossy().to_string();
                if s.is_empty() {
                    ".".to_string()
                } else {
                    s
                }
            })
            .unwrap_or_else(|_| ".".to_string());

        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| {
                // For root path, try to get the directory name
                root_path
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "root".to_string())
            });

        let root_type = if self.is_git_repo(path) {
            let git_info = extract_git_metadata(path);
            RootType::GitRepository {
                remote: git_info.0,
                branch: git_info.1,
                commit: git_info.2,
            }
        } else {
            RootType::CodeDirectory
        };

        Ok(DiscoveredRoot {
            path: path.to_path_buf(),
            relative_path,
            root_type,
            name,
        })
    }
}

/// Extract git metadata from a repository
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_discovery_config_defaults() {
        let config = DiscoveryConfig::default();
        assert_eq!(config.max_depth, 3);
        assert!(config.exclude_dirs.contains("node_modules"));
        assert!(config.exclude_dirs.contains("target"));
        assert!(config.include_code_dirs);
    }

    #[test]
    fn test_is_git_repo() {
        let temp = TempDir::new().unwrap();
        let discovery = RootDiscovery::with_defaults();

        assert!(!discovery.is_git_repo(temp.path()));

        std::fs::create_dir(temp.path().join(".git")).unwrap();
        assert!(discovery.is_git_repo(temp.path()));
    }

    #[test]
    fn test_has_source_files() {
        let temp = TempDir::new().unwrap();
        let discovery = RootDiscovery::with_defaults();

        // Empty directory
        assert!(!discovery.has_source_files(temp.path()));

        // Add a non-source file
        std::fs::write(temp.path().join("readme.txt"), "hello").unwrap();
        assert!(!discovery.has_source_files(temp.path()));

        // Add a source file
        std::fs::write(temp.path().join("main.py"), "print('hello')").unwrap();
        assert!(discovery.has_source_files(temp.path()));
    }

    #[test]
    fn test_discover_single_git_repo() {
        let temp = TempDir::new().unwrap();

        // Create a git repo
        std::fs::create_dir(temp.path().join(".git")).unwrap();
        std::fs::write(temp.path().join("main.py"), "print('hello')").unwrap();

        let discovery = RootDiscovery::with_defaults();
        let roots = discovery.discover(temp.path()).unwrap();

        assert_eq!(roots.len(), 1);
        assert!(roots[0].is_git());
        assert_eq!(roots[0].relative_path, ".");
    }

    #[test]
    fn test_discover_multiple_git_repos() {
        let temp = TempDir::new().unwrap();

        // Create two git repos
        let repo_a = temp.path().join("repo-a");
        let repo_b = temp.path().join("repo-b");

        std::fs::create_dir_all(repo_a.join(".git")).unwrap();
        std::fs::write(repo_a.join("main.py"), "# repo a").unwrap();

        std::fs::create_dir_all(repo_b.join(".git")).unwrap();
        std::fs::write(repo_b.join("main.rs"), "fn main() {}").unwrap();

        let discovery = RootDiscovery::with_defaults();
        let roots = discovery.discover(temp.path()).unwrap();

        assert_eq!(roots.len(), 2);
        assert!(roots.iter().any(|r| r.name == "repo-a"));
        assert!(roots.iter().any(|r| r.name == "repo-b"));
    }

    #[test]
    fn test_discover_code_directory() {
        let temp = TempDir::new().unwrap();

        // Create a code directory (no .git)
        std::fs::write(temp.path().join("main.py"), "print('hello')").unwrap();

        let discovery = RootDiscovery::with_defaults();
        let roots = discovery.discover(temp.path()).unwrap();

        assert_eq!(roots.len(), 1);
        assert!(!roots[0].is_git());
        assert_eq!(roots[0].root_type, RootType::CodeDirectory);
    }

    #[test]
    fn test_discover_mixed_roots() {
        let temp = TempDir::new().unwrap();

        // Git repo
        let git_repo = temp.path().join("git-project");
        std::fs::create_dir_all(git_repo.join(".git")).unwrap();
        std::fs::write(git_repo.join("main.py"), "# git project").unwrap();

        // Code directory
        let code_dir = temp.path().join("scripts");
        std::fs::create_dir_all(&code_dir).unwrap();
        std::fs::write(code_dir.join("util.py"), "# utilities").unwrap();

        let discovery = RootDiscovery::with_defaults();
        let roots = discovery.discover(temp.path()).unwrap();

        assert_eq!(roots.len(), 2);

        let git_root = roots.iter().find(|r| r.name == "git-project").unwrap();
        assert!(git_root.is_git());

        let code_root = roots.iter().find(|r| r.name == "scripts").unwrap();
        assert!(!code_root.is_git());
    }

    #[test]
    fn test_discover_skips_nested_repos() {
        let temp = TempDir::new().unwrap();

        // Parent git repo
        std::fs::create_dir(temp.path().join(".git")).unwrap();
        std::fs::write(temp.path().join("main.py"), "# parent").unwrap();

        // Nested git repo (should be skipped)
        let nested = temp.path().join("nested");
        std::fs::create_dir_all(nested.join(".git")).unwrap();
        std::fs::write(nested.join("lib.py"), "# nested").unwrap();

        let discovery = RootDiscovery::with_defaults();
        let roots = discovery.discover(temp.path()).unwrap();

        // Only parent should be discovered since root is a git repo
        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].relative_path, ".");
    }

    #[test]
    fn test_discover_skips_excluded_dirs() {
        let temp = TempDir::new().unwrap();

        // Create node_modules with source files (should be skipped)
        let node_modules = temp.path().join("node_modules").join("some-package");
        std::fs::create_dir_all(&node_modules).unwrap();
        std::fs::write(node_modules.join("index.js"), "// package").unwrap();

        // Create actual code
        std::fs::write(temp.path().join("app.js"), "// app").unwrap();

        let discovery = RootDiscovery::with_defaults();
        let roots = discovery.discover(temp.path()).unwrap();

        assert_eq!(roots.len(), 1);
        assert_eq!(roots[0].relative_path, ".");
    }

    #[test]
    fn test_root_type_is_git() {
        let git_type = RootType::GitRepository {
            remote: Some("origin".to_string()),
            branch: Some("main".to_string()),
            commit: None,
        };
        assert!(git_type.is_git());

        let code_type = RootType::CodeDirectory;
        assert!(!code_type.is_git());
    }

    #[test]
    fn test_with_max_depth() {
        let discovery = RootDiscovery::with_defaults().with_max_depth(5);
        assert_eq!(discovery.config.max_depth, 5);
    }

    #[test]
    fn test_no_roots_found_error() {
        let temp = TempDir::new().unwrap();

        // Empty directory with no source files
        let discovery = RootDiscovery::with_defaults();
        let result = discovery.discover(temp.path());

        assert!(matches!(result, Err(DiscoveryError::NoRootsFound(_))));
    }
}
