//! Integration tests for .codeprysmignore support
//!
//! Tests that .codeprysmignore files properly exclude files from:
//! - Graph building (builder.rs)
//! - Merkle tree generation (merkle.rs)

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::merkle::{ExclusionFilter, MerkleTreeManager};
use std::path::PathBuf;
use tempfile::TempDir;

/// Get the path to the queries directory.
fn queries_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("queries")
}

// ============================================================================
// Graph Builder .codeprysmignore Tests
// ============================================================================

#[test]
fn test_prismignore_excludes_files_from_graph() {
    let temp = TempDir::new().unwrap();

    // Create source files
    std::fs::write(
        temp.path().join("main.py"),
        "def main():\n    print('hello')\n",
    )
    .unwrap();
    std::fs::write(
        temp.path().join("excluded.py"),
        "def excluded():\n    pass\n",
    )
    .unwrap();
    std::fs::write(temp.path().join("utils.py"), "def helper():\n    pass\n").unwrap();

    // Create .codeprysmignore to exclude 'excluded.py'
    std::fs::write(temp.path().join(".codeprysmignore"), "excluded.py\n").unwrap();

    // Build graph
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");

    let graph = builder
        .build_from_directory(temp.path())
        .expect("Failed to build graph");

    // Get all file nodes
    let file_names: Vec<String> = graph
        .iter_nodes()
        .filter(|node| node.is_file())
        .map(|node| node.name.clone())
        .collect();

    // Verify: main.py and utils.py should be included, excluded.py should not
    assert!(
        file_names.contains(&"main.py".to_string()),
        "main.py should be in graph"
    );
    assert!(
        file_names.contains(&"utils.py".to_string()),
        "utils.py should be in graph"
    );
    assert!(
        !file_names.contains(&"excluded.py".to_string()),
        "excluded.py should be excluded by .codeprysmignore"
    );
}

#[test]
fn test_prismignore_glob_patterns() {
    let temp = TempDir::new().unwrap();

    // Create source files
    std::fs::write(temp.path().join("app.py"), "# app").unwrap();
    std::fs::write(temp.path().join("test_app.py"), "# test").unwrap();
    std::fs::write(temp.path().join("test_utils.py"), "# test").unwrap();

    // Create .codeprysmignore to exclude all test_*.py files
    std::fs::write(temp.path().join(".codeprysmignore"), "test_*.py\n").unwrap();

    // Build graph
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");

    let graph = builder
        .build_from_directory(temp.path())
        .expect("Failed to build graph");

    let file_names: Vec<String> = graph
        .iter_nodes()
        .filter(|node| node.is_file())
        .map(|node| node.name.clone())
        .collect();

    assert!(
        file_names.contains(&"app.py".to_string()),
        "app.py should be in graph"
    );
    assert!(
        !file_names.contains(&"test_app.py".to_string()),
        "test_app.py should be excluded"
    );
    assert!(
        !file_names.contains(&"test_utils.py".to_string()),
        "test_utils.py should be excluded"
    );
}

#[test]
fn test_prismignore_directory_patterns() {
    let temp = TempDir::new().unwrap();

    // Create directory structure with files at root and in subdirs
    let gen_dir = temp.path().join("generated");
    std::fs::create_dir_all(&gen_dir).unwrap();

    // Root level file that should be included
    std::fs::write(temp.path().join("main.py"), "# source").unwrap();
    // File in generated/ that should be excluded
    std::fs::write(gen_dir.join("output.py"), "# generated").unwrap();

    // Create .codeprysmignore to exclude generated/ directory
    std::fs::write(temp.path().join(".codeprysmignore"), "generated/\n").unwrap();

    // Build graph
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");

    let graph = builder
        .build_from_directory(temp.path())
        .expect("Failed to build graph");

    let file_names: Vec<String> = graph
        .iter_nodes()
        .filter(|node| node.is_file())
        .map(|node| node.name.clone())
        .collect();

    assert!(
        file_names.contains(&"main.py".to_string()),
        "main.py should be in graph, found: {:?}",
        file_names
    );
    assert!(
        !file_names.contains(&"output.py".to_string()),
        "generated/output.py should be excluded"
    );
}

// ============================================================================
// Merkle Tree .codeprysmignore Tests
// ============================================================================

#[test]
fn test_prismignore_merkle_tree_excludes_files() {
    let temp = TempDir::new().unwrap();

    // Create source files
    std::fs::write(temp.path().join("included.py"), "# included").unwrap();
    std::fs::write(temp.path().join("excluded.py"), "# excluded").unwrap();

    // Create .codeprysmignore
    std::fs::write(temp.path().join(".codeprysmignore"), "excluded.py\n").unwrap();

    // Build merkle tree
    let filter = ExclusionFilter::default();
    let manager = MerkleTreeManager::new(filter);
    let tree = manager
        .build_merkle_tree(temp.path())
        .expect("Failed to build merkle tree");

    // Check which files are in the tree
    let file_paths: Vec<&String> = tree.keys().collect();

    assert!(
        file_paths.iter().any(|p| p.contains("included.py")),
        "included.py should be in merkle tree"
    );
    assert!(
        !file_paths.iter().any(|p| p.contains("excluded.py")),
        "excluded.py should be excluded from merkle tree"
    );
}

#[test]
fn test_prismignore_combined_with_gitignore() {
    let temp = TempDir::new().unwrap();

    // Create a .git directory so .gitignore is respected
    std::fs::create_dir(temp.path().join(".git")).unwrap();

    // Create source files
    std::fs::write(temp.path().join("app.py"), "# app").unwrap();
    std::fs::write(temp.path().join("ignored_by_git.py"), "# git ignored").unwrap();
    std::fs::write(temp.path().join("ignored_by_prism.py"), "# prism ignored").unwrap();

    // Create .gitignore
    std::fs::write(temp.path().join(".gitignore"), "ignored_by_git.py\n").unwrap();

    // Create .codeprysmignore (additional exclusions)
    std::fs::write(
        temp.path().join(".codeprysmignore"),
        "ignored_by_prism.py\n",
    )
    .unwrap();

    // Build graph
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");

    let graph = builder
        .build_from_directory(temp.path())
        .expect("Failed to build graph");

    let file_names: Vec<String> = graph
        .iter_nodes()
        .filter(|node| node.is_file())
        .map(|node| node.name.clone())
        .collect();

    assert!(
        file_names.contains(&"app.py".to_string()),
        "app.py should be in graph"
    );
    assert!(
        !file_names.contains(&"ignored_by_git.py".to_string()),
        "ignored_by_git.py should be excluded by .gitignore"
    );
    assert!(
        !file_names.contains(&"ignored_by_prism.py".to_string()),
        "ignored_by_prism.py should be excluded by .codeprysmignore"
    );
}

#[test]
fn test_prismignore_empty_file() {
    let temp = TempDir::new().unwrap();

    // Create source files
    std::fs::write(temp.path().join("main.py"), "# main").unwrap();

    // Create empty .codeprysmignore (should not affect anything)
    std::fs::write(temp.path().join(".codeprysmignore"), "").unwrap();

    // Build graph
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");

    let graph = builder
        .build_from_directory(temp.path())
        .expect("Failed to build graph");

    let file_names: Vec<String> = graph
        .iter_nodes()
        .filter(|node| node.is_file())
        .map(|node| node.name.clone())
        .collect();

    assert!(
        file_names.contains(&"main.py".to_string()),
        "main.py should be in graph with empty .codeprysmignore"
    );
}

#[test]
fn test_prismignore_comments_and_blank_lines() {
    let temp = TempDir::new().unwrap();

    // Create source files
    std::fs::write(temp.path().join("app.py"), "# app").unwrap();
    std::fs::write(temp.path().join("test.py"), "# test").unwrap();

    // Create .codeprysmignore with comments and blank lines
    std::fs::write(
        temp.path().join(".codeprysmignore"),
        "# This is a comment\n\ntest.py\n\n# Another comment\n",
    )
    .unwrap();

    // Build graph
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");

    let graph = builder
        .build_from_directory(temp.path())
        .expect("Failed to build graph");

    let file_names: Vec<String> = graph
        .iter_nodes()
        .filter(|node| node.is_file())
        .map(|node| node.name.clone())
        .collect();

    assert!(
        file_names.contains(&"app.py".to_string()),
        "app.py should be in graph"
    );
    assert!(
        !file_names.contains(&"test.py".to_string()),
        "test.py should be excluded (ignoring comments)"
    );
}
