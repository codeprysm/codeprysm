//! Merkle Tree Manager for Code Graph Incremental Updates
//!
//! This module provides efficient file change detection using content hashing
//! for the code graph generation system.

use ignore::WalkBuilder;
use rayon::prelude::*;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tracing::{debug, info, warn};

/// Errors that can occur during Merkle tree operations.
#[derive(Error, Debug)]
pub enum MerkleError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Repository path does not exist: {0}")]
    RepoNotFound(PathBuf),

    #[error("Failed to hash file {path}: {reason}")]
    HashError { path: PathBuf, reason: String },
}

/// Result type for Merkle tree operations.
pub type Result<T> = std::result::Result<T, MerkleError>;

/// Represents detected changes between two Merkle trees.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct ChangeSet {
    /// Files that were modified (content changed).
    pub modified: Vec<String>,
    /// Files that were added.
    pub added: Vec<String>,
    /// Files that were deleted.
    pub deleted: Vec<String>,
}

impl ChangeSet {
    /// Create a new empty ChangeSet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any changes were detected.
    pub fn has_changes(&self) -> bool {
        !self.modified.is_empty() || !self.added.is_empty() || !self.deleted.is_empty()
    }

    /// Total number of changed files.
    pub fn total_changes(&self) -> usize {
        self.modified.len() + self.added.len() + self.deleted.len()
    }

    /// Get all files that need to be re-processed (modified + added).
    pub fn files_to_process(&self) -> Vec<&str> {
        self.modified
            .iter()
            .chain(self.added.iter())
            .map(|s| s.as_str())
            .collect()
    }
}

/// Default file extensions to always exclude.
const DEFAULT_EXCLUDE_EXTENSIONS: &[&str] = &[
    "*.pyc",
    "*.pyo",
    "*.jpg",
    "*.jpeg",
    "*.png",
    "*.gif",
    "*.bmp",
    "*.ico",
    "*.svg",
    "*.pdf",
    "*.zip",
    "*.tar",
    "*.gz",
    "*.rar",
    "*.7z",
    "*.exe",
    "*.dll",
    "*.so",
    "*.dylib",
    "*.o",
    "*.a",
    "*.lib",
    "*.class",
    "*.jar",
    "*.war",
    "*.whl",
    "*.egg",
    "*.db",
    "*.sqlite",
    "*.sqlite3",
];

/// Default directories to always exclude.
const DEFAULT_EXCLUDE_DIRS: &[&str] = &[
    ".git",
    ".DS_Store",
    "__pycache__",
    "node_modules",
    ".venv",
    "venv",
    ".env",
    "target",
    "build",
    "dist",
    ".idea",
    ".vscode",
];

/// Filter for determining which files should be excluded from processing.
#[derive(Debug, Clone)]
pub struct ExclusionFilter {
    /// Glob patterns for files to exclude.
    exclude_patterns: Vec<glob::Pattern>,
    /// Directory names to exclude.
    exclude_dirs: HashSet<String>,
    /// Whether to exclude hidden files/directories.
    exclude_hidden: bool,
}

impl Default for ExclusionFilter {
    fn default() -> Self {
        Self::new(None, true)
    }
}

impl ExclusionFilter {
    /// Create a new exclusion filter with optional custom patterns.
    ///
    /// # Arguments
    /// * `custom_patterns` - Additional glob patterns to exclude
    /// * `exclude_hidden` - Whether to exclude hidden files/directories (starting with '.')
    pub fn new(custom_patterns: Option<&[&str]>, exclude_hidden: bool) -> Self {
        let mut patterns = Vec::new();

        // Add default extension patterns
        for ext in DEFAULT_EXCLUDE_EXTENSIONS {
            if let Ok(p) = glob::Pattern::new(ext) {
                patterns.push(p);
            }
        }

        // Add custom patterns
        if let Some(custom) = custom_patterns {
            for pattern in custom {
                if let Ok(p) = glob::Pattern::new(pattern) {
                    patterns.push(p);
                }
            }
        }

        // Build directory exclusion set
        let exclude_dirs: HashSet<String> =
            DEFAULT_EXCLUDE_DIRS.iter().map(|s| s.to_string()).collect();

        Self {
            exclude_patterns: patterns,
            exclude_dirs,
            exclude_hidden,
        }
    }

    /// Check if a path should be excluded.
    pub fn should_exclude(&self, path: &Path) -> bool {
        let path_str = path.to_string_lossy();

        // Check for hidden files/directories
        if self.exclude_hidden {
            for component in path.components() {
                if let std::path::Component::Normal(name) = component {
                    if let Some(name_str) = name.to_str() {
                        if name_str.starts_with('.') && name_str != "." && name_str != ".." {
                            return true;
                        }
                    }
                }
            }
        }

        // Check directory exclusions
        for component in path.components() {
            if let std::path::Component::Normal(name) = component {
                if let Some(name_str) = name.to_str() {
                    if self.exclude_dirs.contains(name_str) {
                        return true;
                    }
                }
            }
        }

        // Check glob patterns against filename
        if let Some(filename) = path.file_name() {
            let filename_str = filename.to_string_lossy();
            for pattern in &self.exclude_patterns {
                if pattern.matches(&filename_str) {
                    return true;
                }
            }
        }

        // Check glob patterns against full path
        for pattern in &self.exclude_patterns {
            if pattern.matches(&path_str) {
                return true;
            }
        }

        false
    }

    /// Check if a directory should be skipped entirely.
    pub fn should_skip_dir(&self, dir_name: &str) -> bool {
        if self.exclude_hidden && dir_name.starts_with('.') && dir_name != "." && dir_name != ".." {
            return true;
        }
        self.exclude_dirs.contains(dir_name)
    }

    /// Check if hidden files/directories are excluded.
    pub fn excludes_hidden(&self) -> bool {
        self.exclude_hidden
    }
}

/// Merkle tree represented as a map of file paths to their content hashes.
pub type MerkleTree = HashMap<String, String>;

/// Statistics about a Merkle tree.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TreeStats {
    /// Total number of files in the tree.
    pub total_files: usize,
    /// Number of unique directories.
    pub total_dirs: usize,
    /// Average path depth.
    pub avg_path_depth: f64,
}

/// Manages Merkle trees for efficient change detection in code repositories.
///
/// Uses file content hashes to build a tree representation of the repository
/// state, enabling fast detection of file changes without parsing.
#[derive(Debug, Clone)]
pub struct MerkleTreeManager {
    exclusion_filter: ExclusionFilter,
}

impl Default for MerkleTreeManager {
    fn default() -> Self {
        Self::new(ExclusionFilter::default())
    }
}

impl MerkleTreeManager {
    /// Create a new MerkleTreeManager with the given exclusion filter.
    pub fn new(exclusion_filter: ExclusionFilter) -> Self {
        Self { exclusion_filter }
    }

    /// Build a Merkle tree for the given repository.
    ///
    /// Uses rayon for parallel file hashing to maximize performance.
    ///
    /// # Arguments
    /// * `repo_path` - Path to the repository root
    ///
    /// # Returns
    /// HashMap mapping relative file paths to their SHA-256 hashes
    pub fn build_merkle_tree(&self, repo_path: &Path) -> Result<MerkleTree> {
        let repo_path = repo_path
            .canonicalize()
            .map_err(|_| MerkleError::RepoNotFound(repo_path.to_path_buf()))?;

        info!("Building Merkle tree for {:?}", repo_path);
        let start = std::time::Instant::now();

        // Find all files to process
        let files = self.find_files(&repo_path)?;
        info!("Found {} files to process", files.len());

        // Hash files in parallel using rayon
        let file_hashes: Vec<(String, Option<String>)> = files
            .par_iter()
            .map(|(abs_path, rel_path)| {
                let hash = hash_file(abs_path);
                (rel_path.clone(), hash)
            })
            .collect();

        // Collect successful hashes
        let mut tree = HashMap::new();
        let mut failed_count = 0;

        for (rel_path, hash_result) in file_hashes {
            match hash_result {
                Some(hash) => {
                    tree.insert(rel_path, hash);
                }
                None => {
                    failed_count += 1;
                    debug!("Failed to hash: {}", rel_path);
                }
            }
        }

        let elapsed = start.elapsed();
        let rate = tree.len() as f64 / elapsed.as_secs_f64();
        info!(
            "Built Merkle tree: {} files in {:.2}s ({:.0} files/sec)",
            tree.len(),
            elapsed.as_secs_f64(),
            rate
        );

        if failed_count > 0 {
            warn!("Failed to hash {} files", failed_count);
        }

        Ok(tree)
    }

    /// Find all files in the repository that should be included.
    ///
    /// Uses the `ignore` crate to respect `.gitignore` patterns automatically.
    fn find_files(&self, repo_path: &Path) -> Result<Vec<(PathBuf, String)>> {
        let mut files = Vec::new();

        // Use ignore::WalkBuilder which respects .gitignore by default
        let walker = WalkBuilder::new(repo_path)
            .follow_links(false)
            .hidden(self.exclusion_filter.excludes_hidden()) // Respect hidden file setting
            .git_ignore(true) // Respect .gitignore
            .git_global(true) // Respect global gitignore
            .git_exclude(true) // Respect .git/info/exclude
            .add_custom_ignore_filename(".codeprysmignore") // Respect .codeprysmignore
            .build();

        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    warn!("Error walking directory: {}", e);
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

            // Skip excluded directories (additional exclusions beyond gitignore)
            let abs_path = entry.path();
            let rel_path = abs_path
                .strip_prefix(repo_path)
                .unwrap_or(abs_path)
                .to_string_lossy()
                .replace('\\', "/"); // Normalize path separators

            // Check our additional exclusion filter (for patterns not in .gitignore)
            if self.exclusion_filter.should_exclude(Path::new(&rel_path)) {
                continue;
            }

            files.push((abs_path.to_path_buf(), rel_path));
        }

        Ok(files)
    }

    /// Detect changes between two Merkle trees.
    ///
    /// # Arguments
    /// * `old_tree` - Previous tree state (path -> hash)
    /// * `new_tree` - Current tree state (path -> hash)
    ///
    /// # Returns
    /// ChangeSet with detected changes
    pub fn detect_changes(&self, old_tree: &MerkleTree, new_tree: &MerkleTree) -> ChangeSet {
        info!("Detecting changes between Merkle trees");
        let start = std::time::Instant::now();

        let old_files: HashSet<&String> = old_tree.keys().collect();
        let new_files: HashSet<&String> = new_tree.keys().collect();

        // Find modified files (in both trees but with different hashes)
        let modified: Vec<String> = old_files
            .intersection(&new_files)
            .filter(|path| old_tree.get(**path) != new_tree.get(**path))
            .map(|s| (*s).clone())
            .collect();

        // Find added files (in new but not old)
        let added: Vec<String> = new_files
            .difference(&old_files)
            .map(|s| (*s).clone())
            .collect();

        // Find deleted files (in old but not new)
        let deleted: Vec<String> = old_files
            .difference(&new_files)
            .map(|s| (*s).clone())
            .collect();

        let changeset = ChangeSet {
            modified,
            added,
            deleted,
        };

        let elapsed = start.elapsed();
        info!(
            "Change detection completed in {:.3}s: {} modified, {} added, {} deleted",
            elapsed.as_secs_f64(),
            changeset.modified.len(),
            changeset.added.len(),
            changeset.deleted.len()
        );

        changeset
    }

    /// Get statistics about a Merkle tree.
    pub fn get_tree_stats(&self, tree: &MerkleTree) -> TreeStats {
        if tree.is_empty() {
            return TreeStats {
                total_files: 0,
                total_dirs: 0,
                avg_path_depth: 0.0,
            };
        }

        let total_files = tree.len();
        let mut dirs: HashSet<String> = HashSet::new();
        let mut total_depth = 0usize;

        for file_path in tree.keys() {
            let path = Path::new(file_path);
            let depth = path.components().count();
            total_depth += depth;

            // Collect all parent directories
            let mut current = PathBuf::new();
            for component in path.components() {
                if let std::path::Component::Normal(_) = component {
                    current.push(component);
                    if current != path {
                        dirs.insert(current.to_string_lossy().to_string());
                    }
                }
            }
        }

        TreeStats {
            total_files,
            total_dirs: dirs.len(),
            avg_path_depth: total_depth as f64 / total_files as f64,
        }
    }

    /// Save a Merkle tree to a JSON file.
    pub fn save_tree_to_file(&self, tree: &MerkleTree, file_path: &Path) -> Result<()> {
        let file = File::create(file_path)?;
        serde_json::to_writer_pretty(file, tree)?;
        info!(
            "Saved Merkle tree with {} files to {:?}",
            tree.len(),
            file_path
        );
        Ok(())
    }

    /// Load a Merkle tree from a JSON file.
    pub fn load_tree_from_file(&self, file_path: &Path) -> Result<MerkleTree> {
        match File::open(file_path) {
            Ok(file) => {
                let reader = BufReader::new(file);
                let tree: MerkleTree = serde_json::from_reader(reader)?;
                info!(
                    "Loaded Merkle tree with {} files from {:?}",
                    tree.len(),
                    file_path
                );
                Ok(tree)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                warn!("Merkle tree file not found: {:?}", file_path);
                Ok(HashMap::new())
            }
            Err(e) => Err(MerkleError::Io(e)),
        }
    }
}

/// Compute SHA-256 hash of a single file.
///
/// Reads the file in chunks to handle large files efficiently.
/// Compute the SHA-256 hash of a file's contents.
///
/// This is the public API for file hashing used by the graph builder.
///
/// # Arguments
///
/// * `file_path` - Path to the file to hash
///
/// # Returns
///
/// The hex-encoded SHA-256 hash string, or an IO error.
pub fn compute_file_hash(file_path: &Path) -> std::io::Result<String> {
    hash_file(file_path).ok_or_else(|| {
        std::io::Error::other(format!("Failed to hash file: {}", file_path.display()))
    })
}

fn hash_file(file_path: &Path) -> Option<String> {
    let file = match File::open(file_path) {
        Ok(f) => f,
        Err(e) => {
            debug!("Cannot open {:?}: {}", file_path, e);
            return None;
        }
    };

    let mut reader = BufReader::with_capacity(8192, file);
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];

    loop {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(n) => hasher.update(&buffer[..n]),
            Err(e) => {
                debug!("Error reading {:?}: {}", file_path, e);
                return None;
            }
        }
    }

    Some(format!("{:x}", hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_changeset_empty() {
        let cs = ChangeSet::new();
        assert!(!cs.has_changes());
        assert_eq!(cs.total_changes(), 0);
    }

    #[test]
    fn test_changeset_with_changes() {
        let cs = ChangeSet {
            modified: vec!["a.rs".to_string()],
            added: vec!["b.rs".to_string(), "c.rs".to_string()],
            deleted: vec![],
        };
        assert!(cs.has_changes());
        assert_eq!(cs.total_changes(), 3);
        assert_eq!(cs.files_to_process().len(), 3);
    }

    #[test]
    fn test_exclusion_filter_default() {
        let filter = ExclusionFilter::default();

        // Should exclude hidden files
        assert!(filter.should_exclude(Path::new(".hidden")));
        assert!(filter.should_exclude(Path::new("dir/.hidden")));

        // Should exclude default directories
        assert!(filter.should_exclude(Path::new(".git/config")));
        assert!(filter.should_exclude(Path::new("node_modules/package.json")));

        // Should exclude binary extensions
        assert!(filter.should_exclude(Path::new("image.png")));
        assert!(filter.should_exclude(Path::new("lib.so")));

        // Should not exclude source files
        assert!(!filter.should_exclude(Path::new("main.rs")));
        assert!(!filter.should_exclude(Path::new("src/lib.rs")));
    }

    #[test]
    fn test_hash_file() {
        let temp_dir = TempDir::new().unwrap();
        let file_path = temp_dir.path().join("test.txt");
        fs::write(&file_path, "hello world").unwrap();

        let hash = hash_file(&file_path);
        assert!(hash.is_some());
        // SHA-256 hash of "hello world"
        assert_eq!(
            hash.unwrap(),
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn test_merkle_tree_build() {
        let temp_dir = TempDir::new().unwrap();

        // Create some test files
        fs::write(temp_dir.path().join("main.rs"), "fn main() {}").unwrap();
        fs::create_dir(temp_dir.path().join("src")).unwrap();
        fs::write(temp_dir.path().join("src/lib.rs"), "pub mod foo;").unwrap();

        // Use filter that doesn't exclude hidden paths (tempdir paths may contain hidden components)
        let filter = ExclusionFilter::new(None, false);
        let manager = MerkleTreeManager::new(filter);
        let tree = manager.build_merkle_tree(temp_dir.path()).unwrap();

        assert_eq!(tree.len(), 2);
        assert!(tree.contains_key("main.rs"));
        assert!(tree.contains_key("src/lib.rs"));
    }

    #[test]
    fn test_detect_changes() {
        let manager = MerkleTreeManager::default();

        let mut old_tree = HashMap::new();
        old_tree.insert("a.rs".to_string(), "hash1".to_string());
        old_tree.insert("b.rs".to_string(), "hash2".to_string());
        old_tree.insert("c.rs".to_string(), "hash3".to_string());

        let mut new_tree = HashMap::new();
        new_tree.insert("a.rs".to_string(), "hash1".to_string()); // unchanged
        new_tree.insert("b.rs".to_string(), "hash2_modified".to_string()); // modified
        new_tree.insert("d.rs".to_string(), "hash4".to_string()); // added

        let changes = manager.detect_changes(&old_tree, &new_tree);

        assert_eq!(changes.modified, vec!["b.rs"]);
        assert_eq!(changes.added, vec!["d.rs"]);
        assert_eq!(changes.deleted, vec!["c.rs"]);
    }

    #[test]
    fn test_tree_stats() {
        let manager = MerkleTreeManager::default();

        let mut tree = HashMap::new();
        tree.insert("main.rs".to_string(), "hash1".to_string());
        tree.insert("src/lib.rs".to_string(), "hash2".to_string());
        tree.insert("src/core/mod.rs".to_string(), "hash3".to_string());

        let stats = manager.get_tree_stats(&tree);

        assert_eq!(stats.total_files, 3);
        assert!(stats.total_dirs >= 2); // src and src/core
    }

    #[test]
    fn test_save_and_load_tree() {
        let temp_dir = TempDir::new().unwrap();
        let tree_path = temp_dir.path().join("tree.json");

        let manager = MerkleTreeManager::default();

        let mut tree = HashMap::new();
        tree.insert("main.rs".to_string(), "hash1".to_string());
        tree.insert("lib.rs".to_string(), "hash2".to_string());

        manager.save_tree_to_file(&tree, &tree_path).unwrap();
        let loaded_tree = manager.load_tree_from_file(&tree_path).unwrap();

        assert_eq!(tree, loaded_tree);
    }
}
