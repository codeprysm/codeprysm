//! Integration tests for the codeprysm CLI
//!
//! These tests exercise full CLI workflows using fixture repositories.
//! Tests are marked as #[ignore] to avoid running in parallel with unit tests,
//! as they require file system operations and external dependencies.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::Path;
use tempfile::TempDir;

/// Get a Command for the codeprysm binary
#[allow(deprecated)]
fn prism() -> Command {
    Command::cargo_bin("codeprysm").expect("Failed to find codeprysm binary")
}

/// Path to codeprysm-core's component repo fixtures
fn fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("codeprysm-core/tests/fixtures/component_repos")
        .join(name)
}

/// Create a temporary workspace with a copy of a fixture
fn setup_workspace(fixture_name: &str) -> TempDir {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let fixture = fixture_path(fixture_name);

    // Copy fixture to temp directory
    copy_dir_recursive(&fixture, temp.path()).expect("Failed to copy fixture");

    temp
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let file_name = entry.file_name();
        let dest_path = dst.join(&file_name);

        // Skip .codeprysm directories (leftover from previous tests)
        if file_name == ".codeprysm" {
            continue;
        }

        if path.is_dir() {
            std::fs::create_dir_all(&dest_path)?;
            copy_dir_recursive(&path, &dest_path)?;
        } else {
            std::fs::copy(&path, &dest_path)?;
        }
    }
    Ok(())
}

// ============================================================================
// Init Command Integration Tests
// ============================================================================

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_init_creates_prism_directory() {
    let workspace = setup_workspace("rust-workspace");

    // Make sure no .codeprysm exists initially
    let prism_dir = workspace.path().join(".codeprysm");
    if prism_dir.exists() {
        std::fs::remove_dir_all(&prism_dir).unwrap();
    }

    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Verify .codeprysm directory was created
    assert!(workspace.path().join(".codeprysm").exists());
    assert!(workspace.path().join(".codeprysm/manifest.json").exists());
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_init_force_reinitializes() {
    let workspace = setup_workspace("rust-workspace");

    // First init
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Second init without force should fail
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .failure();

    // With force should succeed
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index", "--force"])
        .assert()
        .success();
}

#[test]
fn test_init_non_existent_path() {
    prism()
        .args(["init", "/nonexistent/path/123456789"])
        .assert()
        .failure();
}

// ============================================================================
// Status Command Integration Tests
// ============================================================================

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_status_uninitialized() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    // Create a simple file so it's not completely empty
    std::fs::write(temp.path().join("test.txt"), "test").unwrap();

    prism()
        .current_dir(temp.path())
        .args(["status"])
        .assert()
        .success();
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_status_initialized() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Check status
    prism()
        .current_dir(workspace.path())
        .args(["status"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialized"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_status_json_output() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Check status as JSON
    prism()
        .current_dir(workspace.path())
        .args(["status", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"initialized\""));
}

// ============================================================================
// Graph Command Integration Tests
// ============================================================================

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_graph_stats() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Get graph stats
    prism()
        .current_dir(workspace.path())
        .args(["graph", "stats"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Nodes"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_graph_find_pattern() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Find nodes matching pattern
    prism()
        .current_dir(workspace.path())
        .args(["graph", "find", "*"])
        .assert()
        .success();
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_graph_requires_initialization() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    // Create a simple file so it's not completely empty
    std::fs::write(temp.path().join("test.txt"), "test").unwrap();

    prism()
        .current_dir(temp.path())
        .args(["graph", "stats"])
        .assert()
        .failure();
}

// ============================================================================
// Components Command Integration Tests
// ============================================================================

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_components_list() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // List components
    prism()
        .current_dir(workspace.path())
        .args(["components", "list"])
        .assert()
        .success();
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_components_graph_dot() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Generate DOT graph
    prism()
        .current_dir(workspace.path())
        .args(["components", "graph", "--format", "dot"])
        .assert()
        .success()
        .stdout(predicate::str::contains("digraph"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_components_graph_mermaid() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Generate Mermaid graph
    prism()
        .current_dir(workspace.path())
        .args(["components", "graph", "--format", "mermaid"])
        .assert()
        .success();
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_components_requires_initialization() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    // Create a simple file so it's not completely empty
    std::fs::write(temp.path().join("test.txt"), "test").unwrap();

    prism()
        .current_dir(temp.path())
        .args(["components", "list"])
        .assert()
        .failure();
}

// ============================================================================
// Update Command Integration Tests
// ============================================================================

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_update_requires_initialization() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    // Create a simple file so it's not completely empty
    std::fs::write(temp.path().join("test.txt"), "test").unwrap();

    prism()
        .current_dir(temp.path())
        .args(["update"])
        .assert()
        .failure();
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_update_after_init() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Update should work
    prism()
        .current_dir(workspace.path())
        .args(["update", "--force"])
        .assert()
        .success();
}

// ============================================================================
// Workspace Command Integration Tests
// ============================================================================

#[test]
fn test_workspace_list_empty() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    prism()
        .current_dir(temp.path())
        .args(["workspace", "list"])
        .assert()
        .success();
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_workspace_add_and_list() {
    let temp = TempDir::new().expect("Failed to create temp dir");
    let workspace = setup_workspace("rust-workspace");

    // Add a workspace
    prism()
        .current_dir(temp.path())
        .args([
            "workspace",
            "add",
            "test-ws",
            workspace.path().to_str().unwrap(),
        ])
        .assert()
        .success();

    // List should show it
    prism()
        .current_dir(temp.path())
        .args(["workspace", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("test-ws"));
}

#[test]
fn test_workspace_discover() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    // Discover should work without errors
    prism()
        .current_dir(temp.path())
        .args(["workspace", "discover", "--depth", "1"])
        .assert()
        .success();
}

// ============================================================================
// Multi-Language Workspace Tests
// ============================================================================

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_npm_workspace_init() {
    let workspace = setup_workspace("npm-workspace");

    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    assert!(workspace.path().join(".codeprysm/manifest.json").exists());
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_python_workspace_init() {
    let workspace = setup_workspace("python-monorepo");

    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    assert!(workspace.path().join(".codeprysm/manifest.json").exists());
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_go_workspace_init() {
    let workspace = setup_workspace("go-workspace");

    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    assert!(workspace.path().join(".codeprysm/manifest.json").exists());
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_dotnet_workspace_init() {
    let workspace = setup_workspace("dotnet-solution");

    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    assert!(workspace.path().join(".codeprysm/manifest.json").exists());
}

// ============================================================================
// Quiet Mode Tests
// ============================================================================

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_quiet_mode_suppresses_output() {
    let workspace = setup_workspace("rust-workspace");

    let output = prism()
        .current_dir(workspace.path())
        .args(["--quiet", "init", "--no-index"])
        .output()
        .expect("Failed to execute command");

    // In quiet mode, stderr should be minimal (no info messages)
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("Initializing"),
        "Quiet mode should suppress output"
    );
}

// ============================================================================
// Doctor Command Integration Tests
// ============================================================================

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_doctor_uninitialized_workspace() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    // Create a simple file so it's not completely empty
    std::fs::write(temp.path().join("test.txt"), "test").unwrap();

    // Doctor should run but report failures
    prism()
        .current_dir(temp.path())
        .args(["doctor"])
        .assert()
        .failure() // Exit code 1 because workspace check fails
        .stdout(predicate::str::contains("Workspace"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_doctor_initialized_workspace() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Doctor should report on workspace status
    prism()
        .current_dir(workspace.path())
        .args(["doctor"])
        .assert()
        .stdout(predicate::str::contains("Workspace"))
        .stdout(predicate::str::contains("Graph"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_doctor_json_output() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Doctor with JSON output
    prism()
        .current_dir(workspace.path())
        .args(["doctor", "--json"])
        .assert()
        .stdout(predicate::str::contains("\"overall\""))
        .stdout(predicate::str::contains("\"checks\""))
        .stdout(predicate::str::contains("\"summary\""));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_doctor_check_filter() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Doctor with specific check filter
    prism()
        .current_dir(workspace.path())
        .args(["doctor", "--check", "workspace"])
        .assert()
        .stdout(predicate::str::contains("Workspace"))
        // Should not contain other checks when filtered
        .stdout(predicate::str::contains("Workspace").count(1));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_doctor_check_graph() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Doctor check graph specifically
    prism()
        .current_dir(workspace.path())
        .args(["doctor", "--check", "graph", "--json"])
        .assert()
        .stdout(predicate::str::contains("\"name\":\"Graph\""));
}

// ============================================================================
// Clean Command Integration Tests
// ============================================================================

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_clean_dry_run() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Verify .codeprysm exists
    assert!(workspace.path().join(".codeprysm").exists());

    // Clean with dry-run should not delete
    prism()
        .current_dir(workspace.path())
        .args(["clean", "--dry-run", "--local-only"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Would delete"));

    // .codeprysm should still exist
    assert!(workspace.path().join(".codeprysm").exists());
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_clean_local_only() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Verify .codeprysm exists
    assert!(workspace.path().join(".codeprysm").exists());

    // Clean with force and local-only
    prism()
        .current_dir(workspace.path())
        .args(["clean", "--force", "--local-only"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Deleted"));

    // .codeprysm should be gone
    assert!(!workspace.path().join(".codeprysm").exists());
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_clean_json_output() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize first
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Clean with dry-run and JSON output
    prism()
        .current_dir(workspace.path())
        .args(["clean", "--dry-run", "--local-only", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"dry_run\":true"))
        .stdout(predicate::str::contains("\"local\""));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_clean_already_clean() {
    let temp = TempDir::new().expect("Failed to create temp dir");

    // Create a simple file so it's not completely empty
    std::fs::write(temp.path().join("test.txt"), "test").unwrap();

    // Clean on workspace with no .codeprysm should succeed
    prism()
        .current_dir(temp.path())
        .args(["clean", "--force", "--local-only"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Already clean"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_clean_reinitialize_after_clean() {
    let workspace = setup_workspace("rust-workspace");

    // Initialize
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // Clean
    prism()
        .current_dir(workspace.path())
        .args(["clean", "--force", "--local-only"])
        .assert()
        .success();

    // Re-initialize should work
    prism()
        .current_dir(workspace.path())
        .args(["init", "--no-index"])
        .assert()
        .success();

    // .codeprysm should exist again
    assert!(workspace.path().join(".codeprysm").exists());
}

// ============================================================================
// Config Command Integration Tests
// ============================================================================

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_config_list_defaults() {
    let workspace = setup_workspace("rust-workspace");

    // Config list should work without initialization
    prism()
        .current_dir(workspace.path())
        .args(["config", "list"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Prism Configuration"))
        .stdout(predicate::str::contains("[storage]"))
        .stdout(predicate::str::contains("[backend.qdrant]"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_config_list_json() {
    let workspace = setup_workspace("rust-workspace");

    // Config list with JSON output (array of {key, value, source})
    prism()
        .current_dir(workspace.path())
        .args(["config", "list", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"key\""))
        .stdout(predicate::str::contains("\"value\""))
        .stdout(predicate::str::contains("\"source\""));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_config_list_effective() {
    let workspace = setup_workspace("rust-workspace");

    // Config list with --effective flag
    prism()
        .current_dir(workspace.path())
        .args(["config", "list", "--json", "--effective"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"prism_dir\""));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_config_get_existing_key() {
    let workspace = setup_workspace("rust-workspace");

    // Get a known config value
    prism()
        .current_dir(workspace.path())
        .args(["config", "get", "backend.qdrant.url"])
        .assert()
        .success()
        .stdout(predicate::str::contains("localhost"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_config_get_json() {
    let workspace = setup_workspace("rust-workspace");

    // Get config value with JSON output
    prism()
        .current_dir(workspace.path())
        .args(["config", "get", "backend.qdrant.url", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"http://localhost:6334\""));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_config_get_unknown_key() {
    let workspace = setup_workspace("rust-workspace");

    // Getting unknown key should fail
    prism()
        .current_dir(workspace.path())
        .args(["config", "get", "nonexistent.key"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown configuration key"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_config_set_local() {
    let workspace = setup_workspace("rust-workspace");

    // Set a config value locally
    prism()
        .current_dir(workspace.path())
        .args(["config", "set", "logging.level", "debug"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Set logging.level = debug in local config",
        ));

    // Verify .codeprysm directory was created
    assert!(workspace.path().join(".codeprysm").exists());

    // Verify value was set
    prism()
        .current_dir(workspace.path())
        .args(["config", "get", "logging.level"])
        .assert()
        .success()
        .stdout(predicate::str::contains("debug"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_config_set_invalid_key() {
    let workspace = setup_workspace("rust-workspace");

    // Setting unknown key should fail
    prism()
        .current_dir(workspace.path())
        .args(["config", "set", "unknown.key", "value"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Unknown or read-only"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_config_set_invalid_value() {
    let workspace = setup_workspace("rust-workspace");

    // Setting invalid boolean value should fail
    prism()
        .current_dir(workspace.path())
        .args(["config", "set", "storage.compression", "notabool"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("Failed to set"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_config_path() {
    let workspace = setup_workspace("rust-workspace");

    // Config path should show paths
    prism()
        .current_dir(workspace.path())
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Configuration Paths"))
        .stdout(predicate::str::contains("Global:"))
        .stdout(predicate::str::contains("Local:"));
}

#[test]
#[ignore = "Integration test - run with --ignored"]
fn test_config_path_json() {
    let workspace = setup_workspace("rust-workspace");

    // Config path with JSON output
    prism()
        .current_dir(workspace.path())
        .args(["config", "path", "--json"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"local\""))
        .stdout(predicate::str::contains("\"global_exists\""))
        .stdout(predicate::str::contains("\"local_exists\""));
}
