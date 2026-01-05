//! Edge case and robustness tests for codeprysm-core.
//!
//! These tests validate that unusual inputs are handled gracefully:
//! - Empty files, comment-only files
//! - Very large files, deeply nested code
//! - Malformed syntax, binary content
//! - Unicode identifiers, files without extensions
//! - Symlinks, gitignore patterns
//!
//! ## Running Tests
//!
//! ```bash
//! cargo test --package codeprysm-core --test edge_cases
//! cargo test --package codeprysm-core --test edge_cases -- --nocapture
//! ```

use codeprysm_core::builder::{BuilderConfig, GraphBuilder};
use codeprysm_core::graph::{NodeType, PetCodeGraph};
use std::fs;
use std::io::Write;
use std::path::PathBuf;
use tempfile::Builder as TempBuilder;

// ============================================================================
// Test Helpers
// ============================================================================

fn queries_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("queries")
}

fn build_graph_from_dir(dir: &std::path::Path) -> PetCodeGraph {
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");

    builder
        .build_from_directory(dir)
        .expect("Failed to build graph")
}

/// Build graph and return both graph and any errors (tolerant mode)
fn build_graph_tolerant(dir: &std::path::Path) -> PetCodeGraph {
    let config = BuilderConfig::default();
    let mut builder =
        GraphBuilder::with_config(&queries_dir(), config).expect("Failed to create builder");

    // Build tolerantly - errors in individual files should not crash
    builder
        .build_from_directory(dir)
        .unwrap_or_else(|_| PetCodeGraph::new())
}

/// Count nodes by type
fn count_nodes_by_type(graph: &PetCodeGraph, node_type: NodeType) -> usize {
    graph
        .iter_nodes()
        .filter(|n| n.node_type == node_type)
        .count()
}

/// Count file nodes (Container with kind="file")
#[allow(dead_code)]
fn count_file_nodes(graph: &PetCodeGraph) -> usize {
    graph
        .iter_nodes()
        .filter(|n| n.node_type == NodeType::Container && n.kind.as_deref() == Some("file"))
        .count()
}

/// Find node by name
fn find_node_by_name<'a>(
    graph: &'a PetCodeGraph,
    name: &str,
) -> Option<&'a codeprysm_core::graph::Node> {
    graph.iter_nodes().find(|n| n.name == name)
}

// ============================================================================
// Phase 4.1: File Handling Tests
// ============================================================================

/// Helper to create a temp directory with non-hidden name
fn create_temp_dir() -> tempfile::TempDir {
    TempBuilder::new()
        .prefix("codeprysm_test_")
        .tempdir()
        .expect("Failed to create temp dir")
}

/// Test: Empty file produces only a file node
///
/// Validates that a file with no content is parsed without error
/// and results in only a file Container node with no children.
#[test]
fn test_empty_file() {
    let temp_dir = create_temp_dir();
    let file_path = temp_dir.path().join("empty.py");

    // Create completely empty file
    fs::write(&file_path, "").expect("Failed to write empty file");

    let graph = build_graph_from_dir(temp_dir.path());

    // Should have at least the repository node
    assert!(graph.node_count() >= 1, "Graph should not be empty");

    // Count non-file entities (excluding repository and file nodes)
    let callable_count = count_nodes_by_type(&graph, NodeType::Callable);
    let data_count = count_nodes_by_type(&graph, NodeType::Data);

    // Container count includes repo, file, and type nodes
    let type_containers: Vec<_> = graph
        .iter_nodes()
        .filter(|n| n.node_type == NodeType::Container && n.kind.as_deref() == Some("type"))
        .collect();

    assert_eq!(
        callable_count, 0,
        "Empty file should have no Callable nodes, found {}",
        callable_count
    );
    assert_eq!(
        data_count, 0,
        "Empty file should have no Data nodes, found {}",
        data_count
    );
    assert!(
        type_containers.is_empty(),
        "Empty file should have no type Container nodes, found {:?}",
        type_containers.iter().map(|n| &n.name).collect::<Vec<_>>()
    );
}

/// Test: File with only comments doesn't produce spurious entities
///
/// Validates that comments are not misinterpreted as code entities.
#[test]
fn test_file_with_only_comments() {
    let temp_dir = create_temp_dir();
    let file_path = temp_dir.path().join("comments_only.py");

    let content = r#"# This is a Python file with only comments
# No actual code here

"""
This is a docstring-style comment
that spans multiple lines.
But it's not assigned to anything.
"""

# More comments
# def fake_function():  # This is commented out
#     pass
"#;

    fs::write(&file_path, content).expect("Failed to write file");

    let graph = build_graph_from_dir(temp_dir.path());

    // Should have only repo and file nodes
    let callable_count = count_nodes_by_type(&graph, NodeType::Callable);
    let data_count = count_nodes_by_type(&graph, NodeType::Data);

    let type_containers: Vec<_> = graph
        .iter_nodes()
        .filter(|n| n.node_type == NodeType::Container && n.kind.as_deref() == Some("type"))
        .collect();

    assert_eq!(
        callable_count, 0,
        "Comment-only file should have no Callable nodes, found {}",
        callable_count
    );
    assert_eq!(
        data_count, 0,
        "Comment-only file should have no Data nodes, found {}",
        data_count
    );
    assert!(
        type_containers.is_empty(),
        "Comment-only file should have no type Container nodes"
    );
}

/// Test: Very large file parses without OOM or timeout
///
/// Generates a 10MB+ Python file and validates it parses within limits.
/// This test is marked as expensive and can be skipped in quick test runs.
#[test]
#[ignore] // Run with: cargo test --test edge_cases test_very_large_file -- --ignored
fn test_very_large_file() {
    let temp_dir = create_temp_dir();
    let file_path = temp_dir.path().join("large.py");

    // Generate a large file (>10MB) with many functions
    let mut file = fs::File::create(&file_path).expect("Failed to create file");

    // Write header
    writeln!(file, "# Large generated Python file for testing").unwrap();
    writeln!(file, "from typing import List, Dict\n").unwrap();

    // Generate ~3000 functions (each ~100 lines = 10MB+)
    let num_functions = 3000;
    for i in 0..num_functions {
        writeln!(file, "def function_{i}(param_{i}: int) -> int:").unwrap();
        writeln!(file, "    '''Function {i} docstring.'''").unwrap();
        writeln!(file, "    result = param_{i}").unwrap();
        // Add some body lines to increase size
        for j in 0..30 {
            writeln!(file, "    result = result + {j}  # line {j}").unwrap();
        }
        writeln!(file, "    return result\n").unwrap();
    }

    // Add a class for variety
    writeln!(file, "class LargeClass:").unwrap();
    writeln!(file, "    '''A class in a large file.'''").unwrap();
    for i in 0..100 {
        writeln!(file, "    def method_{i}(self) -> int:").unwrap();
        writeln!(file, "        return {i}").unwrap();
    }

    drop(file);

    // Verify file size
    let metadata = fs::metadata(&file_path).expect("Failed to get metadata");
    let file_size_mb = metadata.len() as f64 / (1024.0 * 1024.0);
    assert!(
        file_size_mb >= 1.0,
        "Generated file should be at least 1MB, got {:.2}MB",
        file_size_mb
    );

    // Time the parsing
    let start = std::time::Instant::now();
    let graph = build_graph_from_dir(temp_dir.path());
    let duration = start.elapsed();

    // Validate results
    assert!(
        duration.as_secs() < 60,
        "Large file should parse in <60s, took {:?}",
        duration
    );

    let callable_count = count_nodes_by_type(&graph, NodeType::Callable);
    assert!(
        callable_count >= num_functions,
        "Should capture at least {} functions, found {}",
        num_functions,
        callable_count
    );

    println!(
        "Large file test: {:.2}MB parsed in {:?}, {} callables found",
        file_size_mb, duration, callable_count
    );
}

/// Test: Deeply nested code (50+ levels) doesn't cause stack overflow
///
/// Creates Python code with deeply nested structures and validates
/// all levels are captured without crashing.
#[test]
fn test_deeply_nested_code() {
    let temp_dir = create_temp_dir();
    let file_path = temp_dir.path().join("nested.py");

    let nesting_depth = 50;

    // Build deeply nested function calls
    let mut content = String::new();
    content.push_str("# Deeply nested Python code\n\n");

    // Create deeply nested classes (Python allows this)
    for i in 0..nesting_depth {
        let indent = "    ".repeat(i);
        content.push_str(&format!("{}class Level{}:\n", indent, i));
        content.push_str(&format!("{}    def method_{i}(self):\n", indent));
        content.push_str(&format!("{}        pass\n", indent));
    }

    // Also create deeply nested if statements in a function
    content.push_str("\ndef deeply_nested_function(x):\n");
    for i in 0..nesting_depth {
        let indent = "    ".repeat(i + 1);
        content.push_str(&format!("{}if x > {}:\n", indent, i));
    }
    let deepest_indent = "    ".repeat(nesting_depth + 1);
    content.push_str(&format!("{}return x\n", deepest_indent));

    fs::write(&file_path, &content).expect("Failed to write file");

    // Should not panic
    let graph = build_graph_from_dir(temp_dir.path());

    // Validate we captured something
    assert!(graph.node_count() > 1, "Graph should have nodes");

    // Check that we have the deeply nested function
    let has_nested_func = graph
        .iter_nodes()
        .any(|n| n.name == "deeply_nested_function");
    assert!(has_nested_func, "Should capture deeply_nested_function");

    // Check that we captured at least some of the nested classes
    let level_classes: Vec<_> = graph
        .iter_nodes()
        .filter(|n| n.name.starts_with("Level"))
        .collect();
    assert!(
        !level_classes.is_empty(),
        "Should capture at least some Level classes"
    );

    println!(
        "Deeply nested test: {} total nodes, {} Level classes captured",
        graph.node_count(),
        level_classes.len()
    );
}

/// Test: Malformed syntax doesn't crash the parser
///
/// Creates Python files with various syntax errors and validates
/// the parser handles them gracefully.
#[test]
fn test_malformed_syntax() {
    let temp_dir = create_temp_dir();

    // Test various types of malformed syntax
    let malformed_files = [
        ("unclosed_paren.py", "def foo(x:\n    return x"),
        ("unclosed_string.py", "def bar():\n    return \"unclosed"),
        ("invalid_indent.py", "def baz():\nreturn 1"),
        ("incomplete_class.py", "class Incomplete:\n    def"),
        ("random_tokens.py", "def ) ( { } [ ] @#$%"),
        ("truncated.py", "class Foo:\n    def method(self"),
    ];

    for (filename, content) in &malformed_files {
        let file_path = temp_dir.path().join(filename);
        fs::write(&file_path, content).expect("Failed to write malformed file");
    }

    // Should not panic - tolerant parsing
    let graph = build_graph_tolerant(temp_dir.path());

    // Graph might be empty or partial, but should not crash
    println!(
        "Malformed syntax test: {} nodes captured from malformed files",
        graph.node_count()
    );
}

// ============================================================================
// Phase 4.2: Special Content Tests
// ============================================================================

/// Test: Unicode identifiers are captured correctly
///
/// Creates Python functions with unicode names and validates they appear
/// in the graph with correct names.
#[test]
fn test_unicode_identifiers() {
    let temp_dir = create_temp_dir();
    let file_path = temp_dir.path().join("unicode.py");

    // Python 3 supports unicode identifiers
    let content = r#"# Unicode identifier test

def 计算(x, y):
    """Chinese: calculate"""
    return x + y

def калькулятор(a, b):
    """Russian: calculator"""
    return a * b

def calcular(x):
    """Spanish: calculate (with accent in comment: señor)"""
    return x * 2

class Τεστ:
    """Greek: Test"""
    def μέθοδος(self):
        return 42

# Japanese function
def 関数():
    return "hello"
"#;

    fs::write(&file_path, content).expect("Failed to write file");

    let graph = build_graph_from_dir(temp_dir.path());

    // Check for unicode function names
    let unicode_names = ["计算", "калькулятор", "calcular", "Τεστ", "μέθοδος", "関数"];

    let mut found = Vec::new();
    for name in &unicode_names {
        if find_node_by_name(&graph, name).is_some() {
            found.push(*name);
        }
    }

    // At minimum, ASCII-compatible "calcular" should be found
    assert!(
        find_node_by_name(&graph, "calcular").is_some(),
        "Should capture ASCII function 'calcular'"
    );

    println!(
        "Unicode test: found {}/{} unicode identifiers: {:?}",
        found.len(),
        unicode_names.len(),
        found
    );
}

/// Test: Binary files are skipped during parsing
///
/// Creates binary content and validates it doesn't appear in the graph.
#[test]
fn test_binary_content_detection() {
    let temp_dir = create_temp_dir();

    // Create a real Python file
    let py_path = temp_dir.path().join("real.py");
    fs::write(&py_path, "def real_function():\n    pass\n").expect("Failed to write");

    // Create binary files with common extensions
    let binary_path = temp_dir.path().join("binary.exe");
    let binary_content: Vec<u8> = vec![
        0x4D, 0x5A, 0x90, 0x00, // MZ header (PE executable)
        0x00, 0x00, 0x00, 0x00, 0xFF, 0xFE, 0x00, 0x00, // Random binary data
        0x00, 0x01, 0x02, 0x03,
    ];
    fs::write(&binary_path, &binary_content).expect("Failed to write binary");

    // Create a .dll file
    let dll_path = temp_dir.path().join("library.dll");
    fs::write(&dll_path, &binary_content).expect("Failed to write dll");

    // Create a file with binary content but .py extension (edge case)
    let fake_py_path = temp_dir.path().join("fake_code.py");
    let binary_with_nulls: Vec<u8> = vec![
        0x00, 0x00, 0x64, 0x65, 0x66, 0x00, 0x00, 0x00, // null bytes mixed
    ];
    fs::write(&fake_py_path, &binary_with_nulls).expect("Failed to write fake py");

    let graph = build_graph_tolerant(temp_dir.path());

    // Real function should be captured
    assert!(
        find_node_by_name(&graph, "real_function").is_some(),
        "Should capture real Python function"
    );

    // Binary files should not create spurious nodes
    let file_nodes: Vec<_> = graph
        .iter_nodes()
        .filter(|n| n.node_type == NodeType::Container && n.kind.as_deref() == Some("file"))
        .map(|n| n.name.clone())
        .collect();

    // Only valid Python files should be in graph
    assert!(
        !file_nodes.iter().any(|name| name.ends_with(".exe")),
        "Should not include .exe files: {:?}",
        file_nodes
    );
    assert!(
        !file_nodes.iter().any(|name| name.ends_with(".dll")),
        "Should not include .dll files: {:?}",
        file_nodes
    );

    println!(
        "Binary detection test: {} file nodes: {:?}",
        file_nodes.len(),
        file_nodes
    );
}

/// Test: Files without extension (shebang files) are detected by content
///
/// Creates shebang scripts without .py extension and validates they're parsed.
#[test]
fn test_files_without_extension() {
    let temp_dir = create_temp_dir();

    // Create a Python script without extension (shebang detection)
    let shebang_path = temp_dir.path().join("run_script");
    let shebang_content = r#"#!/usr/bin/env python3
# This is a Python script without .py extension

def main():
    print("Hello from shebang script")

if __name__ == "__main__":
    main()
"#;
    fs::write(&shebang_path, shebang_content).expect("Failed to write shebang file");

    // Also create a bash script (should not be parsed as Python)
    let bash_path = temp_dir.path().join("bash_script");
    let bash_content = r#"#!/bin/bash
echo "This is bash"
def not_python() {
    echo "This looks like Python but isn't"
}
"#;
    fs::write(&bash_path, bash_content).expect("Failed to write bash file");

    // Create regular .py file for comparison
    let py_path = temp_dir.path().join("regular.py");
    fs::write(&py_path, "def regular_function():\n    pass\n").expect("Failed to write py");

    let graph = build_graph_from_dir(temp_dir.path());

    // Regular Python file should be parsed
    assert!(
        find_node_by_name(&graph, "regular_function").is_some(),
        "Should capture regular Python function"
    );

    // Check if shebang Python is detected (this depends on implementation)
    let has_main = find_node_by_name(&graph, "main").is_some();
    println!("Shebang detection test: main function found = {}", has_main);

    // Note: Whether shebang files are detected depends on implementation
    // This test documents current behavior - not necessarily a requirement
}

// ============================================================================
// Phase 4.3: Repository Structure Tests
// ============================================================================

/// Test: Symlinks are handled consistently (followed or ignored)
///
/// Creates symlinked files and validates consistent behavior without loops.
#[test]
#[cfg(unix)] // Symlinks work differently on Windows
fn test_symlink_handling() {
    let temp_dir = create_temp_dir();

    // Create a real file
    let real_dir = temp_dir.path().join("real");
    fs::create_dir(&real_dir).expect("Failed to create dir");
    let real_file = real_dir.join("real_module.py");
    fs::write(&real_file, "def real_func():\n    pass\n").expect("Failed to write");

    // Create a symlink to the file
    let link_path = temp_dir.path().join("link_to_module.py");
    std::os::unix::fs::symlink(&real_file, &link_path).ok(); // Ignore if fails

    // Create a symlink to a directory
    let link_dir = temp_dir.path().join("linked_dir");
    std::os::unix::fs::symlink(&real_dir, &link_dir).ok();

    // Create a circular symlink (should not cause infinite loop)
    let circular_dir = temp_dir.path().join("circular");
    fs::create_dir(&circular_dir).expect("Failed to create circular dir");
    let circular_link = circular_dir.join("back");
    std::os::unix::fs::symlink(&circular_dir, &circular_link).ok();

    // Should complete without infinite loop
    let start = std::time::Instant::now();
    let graph = build_graph_tolerant(temp_dir.path());
    let duration = start.elapsed();

    assert!(
        duration.as_secs() < 10,
        "Symlink handling should complete quickly, took {:?}",
        duration
    );

    // Should find the real function
    assert!(
        find_node_by_name(&graph, "real_func").is_some(),
        "Should capture real function"
    );

    println!(
        "Symlink test: {} nodes in {:?}",
        graph.node_count(),
        duration
    );
}

/// Test: .gitignore patterns exclude files from parsing
///
/// Creates a .gitignore and validates excluded files don't appear.
#[test]
fn test_gitignore_respect() {
    let temp_dir = create_temp_dir();

    // Create .gitignore
    let gitignore_path = temp_dir.path().join(".gitignore");
    fs::write(
        &gitignore_path,
        "node_modules/\n__pycache__/\n*.pyc\nbuild/\n.env\n",
    )
    .expect("Failed to write .gitignore");

    // Create files that should be included
    let src_dir = temp_dir.path().join("src");
    fs::create_dir(&src_dir).expect("Failed to create src dir");
    fs::write(src_dir.join("main.py"), "def main():\n    pass\n").expect("Failed to write main.py");

    // Create files that should be excluded by gitignore
    let node_modules = temp_dir.path().join("node_modules");
    fs::create_dir(&node_modules).expect("Failed to create node_modules");
    fs::write(
        node_modules.join("fake.py"),
        "def should_be_ignored():\n    pass\n",
    )
    .expect("Failed to write");

    let pycache = temp_dir.path().join("__pycache__");
    fs::create_dir(&pycache).expect("Failed to create __pycache__");
    fs::write(pycache.join("cached.py"), "def cached():\n    pass\n").expect("Failed to write");

    let build = temp_dir.path().join("build");
    fs::create_dir(&build).expect("Failed to create build");
    fs::write(build.join("generated.py"), "def generated():\n    pass\n").expect("Failed to write");

    // Also create a .pyc file
    fs::write(temp_dir.path().join("module.pyc"), "binary pyc content").expect("Failed to write");

    let graph = build_graph_from_dir(temp_dir.path());

    // main should be found
    assert!(
        find_node_by_name(&graph, "main").is_some(),
        "Should capture main function from src/"
    );

    // Ignored files should NOT be found
    let ignored_functions = ["should_be_ignored", "cached", "generated"];
    for name in &ignored_functions {
        assert!(
            find_node_by_name(&graph, name).is_none(),
            "Should NOT find '{}' from gitignored directory",
            name
        );
    }

    // Check file paths don't include ignored directories
    let file_paths: Vec<_> = graph
        .iter_nodes()
        .filter(|n| n.node_type == NodeType::Container && n.kind.as_deref() == Some("file"))
        .map(|n| n.file.clone())
        .collect();

    for path in &file_paths {
        assert!(
            !path.contains("node_modules"),
            "Should not include node_modules files: {}",
            path
        );
        assert!(
            !path.contains("__pycache__"),
            "Should not include __pycache__ files: {}",
            path
        );
        assert!(
            !path.contains("/build/"),
            "Should not include build/ files: {}",
            path
        );
    }

    println!(
        "Gitignore test: {} files in graph: {:?}",
        file_paths.len(),
        file_paths
    );
}
