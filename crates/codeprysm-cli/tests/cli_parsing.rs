//! CLI parsing tests for the codeprysm command
//!
//! Tests that verify CLI argument parsing works correctly.

use assert_cmd::Command;
use predicates::prelude::*;

/// Get a Command for the codeprysm binary
#[allow(deprecated)]
fn prism() -> Command {
    Command::cargo_bin("codeprysm").expect("Failed to find codeprysm binary")
}

// ============================================================================
// Help and Version Tests
// ============================================================================

#[test]
fn test_help_shows_all_commands() {
    prism()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("init"))
        .stdout(predicate::str::contains("update"))
        .stdout(predicate::str::contains("search"))
        .stdout(predicate::str::contains("graph"))
        .stdout(predicate::str::contains("components"))
        .stdout(predicate::str::contains("workspace"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("doctor"))
        .stdout(predicate::str::contains("clean"))
        .stdout(predicate::str::contains("backend"))
        .stdout(predicate::str::contains("config"))
        .stdout(predicate::str::contains("mcp"));
}

#[test]
fn test_version_flag() {
    prism()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("codeprysm"));
}

// ============================================================================
// Global Options Tests
// ============================================================================

#[test]
fn test_global_options_in_help() {
    prism()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("--workspace"))
        .stdout(predicate::str::contains("--config"))
        .stdout(predicate::str::contains("--verbose"))
        .stdout(predicate::str::contains("--quiet"))
        .stdout(predicate::str::contains("--qdrant-url"));
}

#[test]
fn test_conflicting_verbose_quiet_not_prevented() {
    // clap doesn't prevent both by default, but our code handles it
    // This just verifies both flags are accepted
    prism()
        .args(["--verbose", "--quiet", "--help"])
        .assert()
        .success();
}

// ============================================================================
// Init Command Tests
// ============================================================================

#[test]
fn test_init_help() {
    prism()
        .args(["init", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Initialize"))
        .stdout(predicate::str::contains("--force"))
        .stdout(predicate::str::contains("--no-index"))
        .stdout(predicate::str::contains("--queries"))
        .stdout(predicate::str::contains("--no-components"))
        .stdout(predicate::str::contains("--ci"));
}

#[test]
fn test_init_ci_flag() {
    // Just testing parsing, not execution
    prism()
        .args(["init", "--ci", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("CI/CD mode"));
}

#[test]
fn test_init_accepts_path() {
    // Just testing parsing, not execution
    prism()
        .args(["init", "/some/path", "--help"])
        .assert()
        .success();
}

// ============================================================================
// Update Command Tests
// ============================================================================

#[test]
fn test_update_help() {
    prism()
        .args(["update", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Update"))
        .stdout(predicate::str::contains("--force"))
        .stdout(predicate::str::contains("--reindex"))
        .stdout(predicate::str::contains("--index-only"));
}

// ============================================================================
// Search Command Tests
// ============================================================================

#[test]
fn test_search_help() {
    prism()
        .args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("Search"))
        .stdout(predicate::str::contains("--limit"))
        .stdout(predicate::str::contains("--mode"))
        .stdout(predicate::str::contains("--types"))
        .stdout(predicate::str::contains("--min-score"))
        .stdout(predicate::str::contains("--output"))
        .stdout(predicate::str::contains("--snippets"))
        .stdout(predicate::str::contains("--files-only"));
}

#[test]
fn test_search_mode_values() {
    prism()
        .args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("hybrid"))
        .stdout(predicate::str::contains("semantic"))
        .stdout(predicate::str::contains("code"));
}

#[test]
fn test_search_requires_query() {
    prism()
        .args(["search"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

// ============================================================================
// Graph Command Tests
// ============================================================================

#[test]
fn test_graph_help() {
    prism()
        .args(["graph", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("stats"))
        .stdout(predicate::str::contains("node"))
        .stdout(predicate::str::contains("find"))
        .stdout(predicate::str::contains("edges"))
        .stdout(predicate::str::contains("connected"))
        .stdout(predicate::str::contains("code"));
}

#[test]
fn test_graph_stats_help() {
    prism()
        .args(["graph", "stats", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("statistics"));
}

#[test]
fn test_graph_node_requires_id() {
    prism()
        .args(["graph", "node"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_graph_find_help() {
    prism()
        .args(["graph", "find", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("pattern"))
        .stdout(predicate::str::contains("--node-type"))
        .stdout(predicate::str::contains("--limit"));
}

#[test]
fn test_graph_edges_help() {
    prism()
        .args(["graph", "edges", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--direction"))
        .stdout(predicate::str::contains("--edge-type"));
}

#[test]
fn test_graph_connected_help() {
    prism()
        .args(["graph", "connected", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--direction"))
        .stdout(predicate::str::contains("--edge-type"));
}

#[test]
fn test_graph_code_help() {
    prism()
        .args(["graph", "code", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--context"));
}

// ============================================================================
// Components Command Tests
// ============================================================================

#[test]
fn test_components_help() {
    prism()
        .args(["components", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("deps"))
        .stdout(predicate::str::contains("affected"))
        .stdout(predicate::str::contains("graph"));
}

#[test]
fn test_components_list_help() {
    prism()
        .args(["components", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--component-type"))
        .stdout(predicate::str::contains("--roots-only"))
        .stdout(predicate::str::contains("--json"));
}

#[test]
fn test_components_deps_help() {
    prism()
        .args(["components", "deps", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--all"))
        .stdout(predicate::str::contains("--reverse"))
        .stdout(predicate::str::contains("--depth"));
}

#[test]
fn test_components_affected_help() {
    prism()
        .args(["components", "affected", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--base"))
        .stdout(predicate::str::contains("--json"));
}

#[test]
fn test_components_graph_help() {
    prism()
        .args(["components", "graph", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--format"))
        .stdout(predicate::str::contains("--include"))
        .stdout(predicate::str::contains("--exclude"));
}

// ============================================================================
// Workspace Command Tests
// ============================================================================

#[test]
fn test_workspace_help() {
    prism()
        .args(["workspace", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("add"))
        .stdout(predicate::str::contains("remove"))
        .stdout(predicate::str::contains("use"))
        .stdout(predicate::str::contains("discover"));
}

#[test]
fn test_workspace_list_help() {
    prism()
        .args(["workspace", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--json"));
}

#[test]
fn test_workspace_add_requires_args() {
    prism()
        .args(["workspace", "add"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_workspace_remove_requires_name() {
    prism()
        .args(["workspace", "remove"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_workspace_use_requires_name() {
    prism()
        .args(["workspace", "use"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_workspace_discover_help() {
    prism()
        .args(["workspace", "discover", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--depth"));
}

// ============================================================================
// Status Command Tests
// ============================================================================

#[test]
fn test_status_help() {
    prism()
        .args(["status", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--show-config"))
        .stdout(predicate::str::contains("--json"));
}

// ============================================================================
// Doctor Command Tests
// ============================================================================

#[test]
fn test_doctor_help() {
    prism()
        .args(["doctor", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("health check"))
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--check"));
}

#[test]
fn test_doctor_check_option() {
    // Verify --check accepts values (just parsing, not execution)
    prism()
        .args(["doctor", "--check", "workspace", "--help"])
        .assert()
        .success();
}

// ============================================================================
// Clean Command Tests
// ============================================================================

#[test]
fn test_clean_help() {
    prism()
        .args(["clean", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--force"))
        .stdout(predicate::str::contains("--dry-run"))
        .stdout(predicate::str::contains("--local-only"))
        .stdout(predicate::str::contains("--backend-only"))
        .stdout(predicate::str::contains("--json"));
}

#[test]
fn test_clean_dry_run_option() {
    prism()
        .args(["clean", "--dry-run", "--help"])
        .assert()
        .success();
}

#[test]
fn test_clean_force_option() {
    prism()
        .args(["clean", "--force", "--help"])
        .assert()
        .success();
}

// ============================================================================
// Backend Command Tests
// ============================================================================

#[test]
fn test_backend_help() {
    prism()
        .args(["backend", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("start"))
        .stdout(predicate::str::contains("stop"))
        .stdout(predicate::str::contains("status"));
}

#[test]
fn test_backend_start_help() {
    prism()
        .args(["backend", "start", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--image"))
        .stdout(predicate::str::contains("--grpc-port"))
        .stdout(predicate::str::contains("--rest-port"))
        .stdout(predicate::str::contains("--storage"))
        .stdout(predicate::str::contains("--force"));
}

#[test]
fn test_backend_stop_help() {
    prism()
        .args(["backend", "stop", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--remove"));
}

#[test]
fn test_backend_status_help() {
    prism()
        .args(["backend", "status", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--json"));
}

// ============================================================================
// Config Command Tests
// ============================================================================

#[test]
fn test_config_help() {
    prism()
        .args(["config", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("get"))
        .stdout(predicate::str::contains("set"))
        .stdout(predicate::str::contains("path"));
}

#[test]
fn test_config_list_help() {
    prism()
        .args(["config", "list", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--json"))
        .stdout(predicate::str::contains("--effective"));
}

#[test]
fn test_config_get_help() {
    prism()
        .args(["config", "get", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--json"));
}

#[test]
fn test_config_get_requires_key() {
    prism()
        .args(["config", "get"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_config_set_help() {
    prism()
        .args(["config", "set", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--global"));
}

#[test]
fn test_config_set_requires_key_and_value() {
    prism()
        .args(["config", "set"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("required"));
}

#[test]
fn test_config_path_help() {
    prism()
        .args(["config", "path", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--json"));
}

// ============================================================================
// Mcp Command Tests
// ============================================================================

#[test]
fn test_mcp_help() {
    prism()
        .args(["mcp", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("MCP server"))
        .stdout(predicate::str::contains("--root"))
        .stdout(predicate::str::contains("--codeprysm-dir"))
        .stdout(predicate::str::contains("--repo-id"))
        .stdout(predicate::str::contains("--queries"))
        .stdout(predicate::str::contains("--no-auto-generate"))
        .stdout(predicate::str::contains("--log-file"))
        .stdout(predicate::str::contains("--debug"));
}

#[test]
fn test_mcp_with_root() {
    // Just testing parsing, not execution
    prism()
        .args(["mcp", "--root", "/some/path", "--help"])
        .assert()
        .success();
}

#[test]
fn test_mcp_with_all_options() {
    prism()
        .args([
            "mcp",
            "--root",
            "/some/path",
            "--codeprysm-dir",
            "/some/.codeprysm",
            "--repo-id",
            "my-repo",
            "--queries",
            "/custom/queries",
            "--no-auto-generate",
            "--log-file",
            "/tmp/prism.log",
            "--debug",
            "--help",
        ])
        .assert()
        .success();
}

// ============================================================================
// Error Handling Tests
// ============================================================================

#[test]
fn test_unknown_command() {
    prism()
        .args(["nonexistent"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unrecognized"));
}

#[test]
fn test_unknown_option() {
    prism()
        .args(["--nonexistent-option"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("unexpected"));
}
