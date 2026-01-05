//! Doctor command - Comprehensive health check with recommendations
//!
//! Provides detailed health diagnostics for:
//! - Workspace initialization state
//! - Backend (Qdrant) connectivity
//! - Graph validity and content
//! - Search index status and freshness
//! - Embedding model availability
//! - Schema version compatibility

use std::collections::HashMap;
use std::fmt;
use std::path::Path;

use anyhow::Result;
use clap::Args;
use codeprysm_backend::Backend;
use serde::{Deserialize, Serialize};

use super::{create_backend, load_config, resolve_workspace};
use crate::GlobalOptions;

/// Current expected schema versions
const EXPECTED_MANIFEST_VERSION: &str = "1.0";
const EXPECTED_PARTITION_VERSION: &str = "1.1";

/// Manifest structure for reading schema version
#[derive(Debug, Deserialize)]
struct ManifestInfo {
    schema_version: String,
    #[serde(default)]
    partitions: HashMap<String, String>,
}

/// Arguments for the doctor command
#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Output as JSON for CI/CD integration
    #[arg(long)]
    json: bool,

    /// Check specific component only (workspace, backend, graph, index, models, schema)
    #[arg(long)]
    check: Option<String>,
}

/// Health check status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CheckStatus {
    /// Check passed
    Pass,
    /// Check passed with warnings
    Warn,
    /// Check failed
    Fail,
    /// Check was skipped (e.g., dependency failed)
    Skip,
}

impl fmt::Display for CheckStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CheckStatus::Pass => write!(f, "✓"),
            CheckStatus::Warn => write!(f, "!"),
            CheckStatus::Fail => write!(f, "✗"),
            CheckStatus::Skip => write!(f, "-"),
        }
    }
}

/// Result of a single health check
#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    /// Name of the check
    pub name: String,
    /// Check status
    pub status: CheckStatus,
    /// Human-readable message
    pub message: String,
    /// Recommendation if check failed or warned
    #[serde(skip_serializing_if = "Option::is_none")]
    pub recommendation: Option<String>,
    /// Additional details (for verbose output)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl CheckResult {
    fn pass(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Pass,
            message: message.into(),
            recommendation: None,
            details: None,
        }
    }

    fn warn(
        name: impl Into<String>,
        message: impl Into<String>,
        recommendation: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Warn,
            message: message.into(),
            recommendation: Some(recommendation.into()),
            details: None,
        }
    }

    fn fail(
        name: impl Into<String>,
        message: impl Into<String>,
        recommendation: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Fail,
            message: message.into(),
            recommendation: Some(recommendation.into()),
            details: None,
        }
    }

    fn skip(name: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            status: CheckStatus::Skip,
            message: message.into(),
            recommendation: None,
            details: None,
        }
    }

    fn with_details(mut self, details: serde_json::Value) -> Self {
        self.details = Some(details);
        self
    }
}

/// Comprehensive health report
#[derive(Debug, Clone, Serialize)]
pub struct HealthReport {
    /// Overall health status
    pub overall: CheckStatus,
    /// Individual check results
    pub checks: Vec<CheckResult>,
    /// Summary counts
    pub summary: HealthSummary,
}

/// Summary of health check results
#[derive(Debug, Clone, Serialize)]
pub struct HealthSummary {
    pub passed: usize,
    pub warnings: usize,
    pub failed: usize,
    pub skipped: usize,
}

impl HealthReport {
    fn new(checks: Vec<CheckResult>) -> Self {
        let mut passed = 0;
        let mut warnings = 0;
        let mut failed = 0;
        let mut skipped = 0;

        for check in &checks {
            match check.status {
                CheckStatus::Pass => passed += 1,
                CheckStatus::Warn => warnings += 1,
                CheckStatus::Fail => failed += 1,
                CheckStatus::Skip => skipped += 1,
            }
        }

        let overall = if failed > 0 {
            CheckStatus::Fail
        } else if warnings > 0 {
            CheckStatus::Warn
        } else {
            CheckStatus::Pass
        };

        Self {
            overall,
            checks,
            summary: HealthSummary {
                passed,
                warnings,
                failed,
                skipped,
            },
        }
    }
}

/// Execute the doctor command
pub async fn execute(args: DoctorArgs, global: GlobalOptions) -> Result<()> {
    let mut checks = Vec::new();

    // Filter to specific check if requested
    let check_filter = args.check.as_deref();

    // 1. Check workspace
    let (workspace_ok, workspace_path) = if should_run_check(check_filter, "workspace") {
        check_workspace(&global, &mut checks).await
    } else {
        (true, None)
    };

    // 2. Check configuration
    if should_run_check(check_filter, "config") && workspace_ok {
        if let Some(ref path) = workspace_path {
            check_config(&global, path, &mut checks);
        }
    }

    // 3. Check backend connectivity (requires workspace)
    let backend_ok = if should_run_check(check_filter, "backend") && workspace_ok {
        check_backend(&global, &mut checks).await
    } else if !workspace_ok && should_run_check(check_filter, "backend") {
        checks.push(CheckResult::skip(
            "Backend",
            "Skipped (workspace check failed)",
        ));
        false
    } else {
        true
    };

    // 4. Check graph (requires backend)
    let graph_ok = if should_run_check(check_filter, "graph") && backend_ok {
        check_graph(&global, &mut checks).await
    } else if !backend_ok && should_run_check(check_filter, "graph") {
        checks.push(CheckResult::skip("Graph", "Skipped (backend check failed)"));
        false
    } else {
        true
    };

    // 5. Check search index (requires backend)
    if should_run_check(check_filter, "index") && backend_ok {
        check_index(&global, graph_ok, &mut checks).await;
    } else if !backend_ok && should_run_check(check_filter, "index") {
        checks.push(CheckResult::skip("Index", "Skipped (backend check failed)"));
    }

    // 6. Check embedding models (requires backend)
    if should_run_check(check_filter, "models") && backend_ok {
        check_models(&global, &mut checks).await;
    } else if !backend_ok && should_run_check(check_filter, "models") {
        checks.push(CheckResult::skip(
            "Models",
            "Skipped (backend check failed)",
        ));
    }

    // 7. Check schema versions (requires workspace)
    if should_run_check(check_filter, "schema") && workspace_ok {
        if let Some(ref path) = workspace_path {
            check_schema(path, &mut checks);
        }
    } else if !workspace_ok && should_run_check(check_filter, "schema") {
        checks.push(CheckResult::skip(
            "Schema",
            "Skipped (workspace check failed)",
        ));
    }

    // Build report
    let report = HealthReport::new(checks);

    // Output results
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_report(&report, global.verbose);
    }

    // Exit with error if any checks failed
    if report.overall == CheckStatus::Fail {
        std::process::exit(1);
    }

    Ok(())
}

fn should_run_check(filter: Option<&str>, check_name: &str) -> bool {
    filter.is_none() || filter == Some(check_name)
}

async fn check_workspace(
    global: &GlobalOptions,
    checks: &mut Vec<CheckResult>,
) -> (bool, Option<std::path::PathBuf>) {
    match resolve_workspace(global).await {
        Ok(path) => {
            let prism_dir = path.join(".codeprysm");
            let manifest_path = prism_dir.join("manifest.json");

            if !prism_dir.exists() {
                checks.push(CheckResult::fail(
                    "Workspace",
                    format!("Not initialized: {}", path.display()),
                    "Run 'codeprysm init' to initialize this workspace",
                ));
                (false, Some(path))
            } else if !manifest_path.exists() {
                checks.push(CheckResult::fail(
                    "Workspace",
                    "CodePrysm directory exists but manifest.json is missing",
                    "Run 'codeprysm init' to regenerate the workspace",
                ));
                (false, Some(path))
            } else {
                // Check manifest is valid JSON
                match std::fs::read_to_string(&manifest_path) {
                    Ok(content) => {
                        if serde_json::from_str::<serde_json::Value>(&content).is_ok() {
                            checks.push(
                                CheckResult::pass(
                                    "Workspace",
                                    format!("Initialized at {}", path.display()),
                                )
                                .with_details(serde_json::json!({
                                    "path": path,
                                    "prism_dir": prism_dir,
                                })),
                            );
                            (true, Some(path))
                        } else {
                            checks.push(CheckResult::fail(
                                "Workspace",
                                "manifest.json is corrupted (invalid JSON)",
                                "Run 'codeprysm init' to regenerate the workspace",
                            ));
                            (false, Some(path))
                        }
                    }
                    Err(e) => {
                        checks.push(CheckResult::fail(
                            "Workspace",
                            format!("Cannot read manifest.json: {}", e),
                            "Check file permissions or run 'codeprysm init' to regenerate",
                        ));
                        (false, Some(path))
                    }
                }
            }
        }
        Err(e) => {
            checks.push(CheckResult::fail(
                "Workspace",
                format!("Cannot resolve workspace: {}", e),
                "Specify a workspace with --workspace or cd to a valid directory",
            ));
            (false, None)
        }
    }
}

fn check_config(global: &GlobalOptions, workspace: &Path, checks: &mut Vec<CheckResult>) {
    match load_config(global, workspace) {
        Ok(config) => {
            let local_config = workspace.join(".codeprysm").join("config.toml");
            let has_local = local_config.exists();

            let message = if has_local {
                "Configuration loaded (workspace config found)"
            } else {
                "Configuration loaded (using defaults)"
            };

            checks.push(
                CheckResult::pass("Config", message).with_details(serde_json::json!({
                    "qdrant_url": config.backend.qdrant.url,
                    "prism_dir": config.storage.prism_dir,
                    "has_local_config": has_local,
                    "exclude_patterns_count": config.analysis.exclude_patterns.len(),
                })),
            );
        }
        Err(e) => {
            checks.push(CheckResult::warn(
                "Config",
                format!("Configuration error: {}", e),
                "Check your configuration files for syntax errors",
            ));
        }
    }
}

async fn check_backend(global: &GlobalOptions, checks: &mut Vec<CheckResult>) -> bool {
    match create_backend(global).await {
        Ok(backend) => {
            // Check health (Qdrant connectivity)
            match backend.health_check().await {
                Ok(true) => {
                    checks.push(
                        CheckResult::pass("Backend", "Qdrant connected and healthy").with_details(
                            serde_json::json!({
                                "qdrant_url": &global.qdrant_url,
                            }),
                        ),
                    );
                    true
                }
                Ok(false) => {
                    checks.push(CheckResult::warn(
                        "Backend",
                        "Qdrant connected but reporting unhealthy",
                        "Check Qdrant server logs for issues",
                    ));
                    true // Still return true as connection works
                }
                Err(e) => {
                    let err_str = e.to_string();
                    let recommendation = if err_str.contains("connection refused")
                        || err_str.contains("Connection refused")
                    {
                        "Start Qdrant: docker run -p 6333:6333 -p 6334:6334 qdrant/qdrant"
                    } else if err_str.contains("timeout") {
                        "Qdrant is not responding. Check if the server is running and accessible."
                    } else {
                        "Check Qdrant URL and server status"
                    };

                    checks.push(CheckResult::fail(
                        "Backend",
                        format!("Cannot connect to Qdrant: {}", e),
                        recommendation,
                    ));
                    false
                }
            }
        }
        Err(e) => {
            checks.push(CheckResult::fail(
                "Backend",
                format!("Cannot create backend: {}", e),
                "Check workspace configuration and Qdrant availability",
            ));
            false
        }
    }
}

async fn check_graph(global: &GlobalOptions, checks: &mut Vec<CheckResult>) -> bool {
    match create_backend(global).await {
        Ok(backend) => match backend.graph_stats().await {
            Ok(stats) => {
                if stats.node_count == 0 {
                    checks.push(CheckResult::warn(
                        "Graph",
                        "Graph is empty (no nodes)",
                        "Run 'codeprysm init' to build the code graph",
                    ));
                    false
                } else if stats.file_count == 0 {
                    checks.push(CheckResult::warn(
                        "Graph",
                        format!("{} nodes but no files detected", stats.node_count),
                        "Check that source files match language patterns",
                    ));
                    false
                } else {
                    checks.push(
                        CheckResult::pass(
                            "Graph",
                            format!(
                                "{} nodes, {} edges, {} files, {} components",
                                stats.node_count,
                                stats.edge_count,
                                stats.file_count,
                                stats.component_count
                            ),
                        )
                        .with_details(serde_json::json!({
                            "node_count": stats.node_count,
                            "edge_count": stats.edge_count,
                            "file_count": stats.file_count,
                            "component_count": stats.component_count,
                            "nodes_by_type": stats.nodes_by_type,
                            "edges_by_type": stats.edges_by_type,
                        })),
                    );
                    true
                }
            }
            Err(e) => {
                checks.push(CheckResult::fail(
                    "Graph",
                    format!("Cannot load graph: {}", e),
                    "Run 'codeprysm init' to rebuild the code graph",
                ));
                false
            }
        },
        Err(_) => {
            // Backend error already reported
            false
        }
    }
}

async fn check_index(global: &GlobalOptions, graph_ok: bool, checks: &mut Vec<CheckResult>) {
    let backend = match create_backend(global).await {
        Ok(b) => b,
        Err(_) => return, // Backend error already reported
    };

    match backend.index_status().await {
        Ok(status) => {
            if !status.exists {
                if graph_ok {
                    checks.push(CheckResult::warn(
                        "Index",
                        "Search index not created",
                        "Run 'codeprysm update --reindex' to create the search index",
                    ));
                } else {
                    checks.push(CheckResult::skip(
                        "Index",
                        "Search index not created (graph needs rebuild first)",
                    ));
                }
            } else if status.entity_count == 0 {
                checks.push(CheckResult::warn(
                    "Index",
                    "Search index exists but is empty",
                    "Run 'codeprysm update --reindex' to rebuild the search index",
                ));
            } else {
                // Check if index might be stale
                let message = format!(
                    "{} entities indexed ({} semantic, {} code)",
                    status.entity_count, status.semantic_count, status.code_count
                );

                // Check for potential staleness based on counts
                let graph_stats = backend.graph_stats().await.ok();
                let is_potentially_stale = graph_stats
                    .as_ref()
                    .map(|gs| {
                        // If node count differs significantly from entity count, might be stale
                        let diff = (gs.node_count as i64 - status.entity_count as i64).abs();
                        diff > (gs.node_count as i64 / 10) // More than 10% difference
                    })
                    .unwrap_or(false);

                if is_potentially_stale {
                    checks.push(CheckResult::warn(
                        "Index",
                        format!("{} (may be stale)", message),
                        "Run 'codeprysm update --reindex' to sync index with graph",
                    ));
                } else {
                    checks.push(CheckResult::pass("Index", message).with_details(
                        serde_json::json!({
                            "entity_count": status.entity_count,
                            "semantic_count": status.semantic_count,
                            "code_count": status.code_count,
                            "version": status.version,
                        }),
                    ));
                }
            }
        }
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("collection") && err_str.contains("not found") {
                checks.push(CheckResult::warn(
                    "Index",
                    "Search collection not found in Qdrant",
                    "Run 'codeprysm update --reindex' to create the search index",
                ));
            } else {
                checks.push(CheckResult::fail(
                    "Index",
                    format!("Cannot check index status: {}", e),
                    "Check Qdrant connection and try again",
                ));
            }
        }
    }
}

async fn check_models(global: &GlobalOptions, checks: &mut Vec<CheckResult>) {
    let backend = match create_backend(global).await {
        Ok(b) => b,
        Err(_) => return, // Backend error already reported
    };

    match backend.check_provider().await {
        Ok(provider_status) => {
            // Convert to ModelStatus for backward compatibility with existing checks
            let status: codeprysm_backend::ModelStatus = provider_status.into();
            let mut issues = Vec::new();

            if !status.semantic_available {
                issues.push("semantic model unavailable");
            }
            if !status.code_available {
                issues.push("code model unavailable");
            }

            if issues.is_empty() {
                let loaded_status = match (status.semantic_loaded, status.code_loaded) {
                    (true, true) => "both loaded",
                    (true, false) => "semantic loaded, code available",
                    (false, true) => "semantic available, code loaded",
                    (false, false) => "both available (not loaded)",
                };

                checks.push(
                    CheckResult::pass(
                        "Models",
                        format!(
                            "Embedding models ready ({}), device: {}",
                            loaded_status, status.device
                        ),
                    )
                    .with_details(serde_json::json!({
                        "semantic_available": status.semantic_available,
                        "code_available": status.code_available,
                        "semantic_loaded": status.semantic_loaded,
                        "code_loaded": status.code_loaded,
                        "device": status.device,
                    })),
                );
            } else {
                let mut recommendation =
                    String::from("Models will be downloaded on first search. ");
                if let Some(ref err) = status.semantic_error {
                    recommendation.push_str(&format!("Semantic error: {}. ", err));
                }
                if let Some(ref err) = status.code_error {
                    recommendation.push_str(&format!("Code error: {}. ", err));
                }
                recommendation.push_str("Ensure HuggingFace Hub is accessible.");

                checks.push(
                    CheckResult::warn(
                        "Models",
                        format!("Issues: {}", issues.join(", ")),
                        recommendation,
                    )
                    .with_details(serde_json::json!({
                        "semantic_error": status.semantic_error,
                        "code_error": status.code_error,
                        "device": status.device,
                    })),
                );
            }
        }
        Err(e) => {
            checks.push(CheckResult::warn(
                "Models",
                format!("Cannot check model status: {}", e),
                "Model status check failed but search may still work",
            ));
        }
    }
}

fn check_schema(workspace: &Path, checks: &mut Vec<CheckResult>) {
    let prism_dir = workspace.join(".codeprysm");
    let manifest_path = prism_dir.join("manifest.json");

    // Read and parse manifest
    let manifest = match std::fs::read_to_string(&manifest_path) {
        Ok(content) => match serde_json::from_str::<ManifestInfo>(&content) {
            Ok(m) => m,
            Err(e) => {
                checks.push(CheckResult::fail(
                    "Schema",
                    format!("Cannot parse manifest: {}", e),
                    "Run 'codeprysm init' to regenerate the workspace",
                ));
                return;
            }
        },
        Err(e) => {
            checks.push(CheckResult::fail(
                "Schema",
                format!("Cannot read manifest: {}", e),
                "Run 'codeprysm init' to regenerate the workspace",
            ));
            return;
        }
    };

    // Check manifest schema version
    let manifest_version_ok = manifest.schema_version == EXPECTED_MANIFEST_VERSION;

    // Check partition schema versions
    let partitions_dir = prism_dir.join("partitions");
    let mut partition_issues = Vec::new();
    let mut partitions_checked = 0;

    if partitions_dir.exists() {
        for (partition_id, filename) in &manifest.partitions {
            let partition_path = partitions_dir.join(filename);
            if partition_path.exists() {
                if let Ok(conn) = rusqlite::Connection::open(&partition_path) {
                    match conn.query_row(
                        "SELECT value FROM partition_metadata WHERE key = 'schema_version'",
                        [],
                        |row| row.get::<_, String>(0),
                    ) {
                        Ok(version) => {
                            partitions_checked += 1;
                            if version != EXPECTED_PARTITION_VERSION {
                                partition_issues.push(format!(
                                    "{}: v{} (expected v{})",
                                    partition_id, version, EXPECTED_PARTITION_VERSION
                                ));
                            }
                        }
                        Err(_) => {
                            partition_issues
                                .push(format!("{}: missing schema version", partition_id));
                        }
                    }
                }
            }
        }
    }

    // Build result
    if manifest_version_ok && partition_issues.is_empty() && partitions_checked > 0 {
        checks.push(
            CheckResult::pass(
                "Schema",
                format!(
                    "Versions OK: manifest v{}, {} partitions v{}",
                    manifest.schema_version, partitions_checked, EXPECTED_PARTITION_VERSION
                ),
            )
            .with_details(serde_json::json!({
                "manifest_version": manifest.schema_version,
                "partition_version": EXPECTED_PARTITION_VERSION,
                "partitions_checked": partitions_checked,
            })),
        );
    } else if !manifest_version_ok {
        checks.push(CheckResult::warn(
            "Schema",
            format!(
                "Manifest version {} (expected {})",
                manifest.schema_version, EXPECTED_MANIFEST_VERSION
            ),
            "Run 'codeprysm update --force' to update schema",
        ));
    } else if !partition_issues.is_empty() {
        checks.push(CheckResult::warn(
            "Schema",
            format!(
                "Partition version mismatch: {}",
                partition_issues.join(", ")
            ),
            "Run 'codeprysm update --force' to rebuild partitions",
        ));
    } else if partitions_checked == 0 {
        checks.push(CheckResult::warn(
            "Schema",
            "No partitions found to check",
            "Run 'codeprysm init' to build the code graph",
        ));
    }
}

fn print_report(report: &HealthReport, verbose: bool) {
    println!("Prism Health Check");
    println!("==================\n");

    for check in &report.checks {
        let status_icon = match check.status {
            CheckStatus::Pass => "\x1b[32m✓\x1b[0m", // Green
            CheckStatus::Warn => "\x1b[33m!\x1b[0m", // Yellow
            CheckStatus::Fail => "\x1b[31m✗\x1b[0m", // Red
            CheckStatus::Skip => "\x1b[90m-\x1b[0m", // Gray
        };

        println!("{} {}: {}", status_icon, check.name, check.message);

        if let Some(ref rec) = check.recommendation {
            println!("  → {}", rec);
        }

        if verbose {
            if let Some(ref details) = check.details {
                for (key, value) in details.as_object().unwrap_or(&serde_json::Map::new()) {
                    println!("    {}: {}", key, value);
                }
            }
        }
    }

    println!();

    // Summary
    let summary = &report.summary;
    let overall_icon = match report.overall {
        CheckStatus::Pass => "\x1b[32m✓\x1b[0m",
        CheckStatus::Warn => "\x1b[33m!\x1b[0m",
        CheckStatus::Fail => "\x1b[31m✗\x1b[0m",
        CheckStatus::Skip => "\x1b[90m-\x1b[0m",
    };

    println!(
        "Summary: {} passed, {} warnings, {} failed, {} skipped",
        summary.passed, summary.warnings, summary.failed, summary.skipped
    );

    let overall_msg = match report.overall {
        CheckStatus::Pass => "All checks passed",
        CheckStatus::Warn => "Passed with warnings",
        CheckStatus::Fail => "Some checks failed",
        CheckStatus::Skip => "Checks incomplete",
    };
    println!("{} {}", overall_icon, overall_msg);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_check_result_pass() {
        let result = CheckResult::pass("Test", "All good");
        assert_eq!(result.status, CheckStatus::Pass);
        assert!(result.recommendation.is_none());
    }

    #[test]
    fn test_check_result_fail() {
        let result = CheckResult::fail("Test", "Something wrong", "Fix it");
        assert_eq!(result.status, CheckStatus::Fail);
        assert_eq!(result.recommendation, Some("Fix it".to_string()));
    }

    #[test]
    fn test_check_result_warn() {
        let result = CheckResult::warn("Test", "Minor issue", "Consider fixing");
        assert_eq!(result.status, CheckStatus::Warn);
        assert_eq!(result.recommendation, Some("Consider fixing".to_string()));
    }

    #[test]
    fn test_check_result_skip() {
        let result = CheckResult::skip("Test", "Dependency failed");
        assert_eq!(result.status, CheckStatus::Skip);
        assert!(result.recommendation.is_none());
    }

    #[test]
    fn test_check_result_with_details() {
        let result =
            CheckResult::pass("Test", "OK").with_details(serde_json::json!({"key": "value"}));
        assert!(result.details.is_some());
    }

    #[test]
    fn test_health_report_all_pass() {
        let checks = vec![CheckResult::pass("A", "OK"), CheckResult::pass("B", "OK")];
        let report = HealthReport::new(checks);
        assert_eq!(report.overall, CheckStatus::Pass);
        assert_eq!(report.summary.passed, 2);
        assert_eq!(report.summary.failed, 0);
    }

    #[test]
    fn test_health_report_with_failure() {
        let checks = vec![
            CheckResult::pass("A", "OK"),
            CheckResult::fail("B", "Failed", "Fix"),
        ];
        let report = HealthReport::new(checks);
        assert_eq!(report.overall, CheckStatus::Fail);
        assert_eq!(report.summary.passed, 1);
        assert_eq!(report.summary.failed, 1);
    }

    #[test]
    fn test_health_report_warn_only() {
        let checks = vec![
            CheckResult::pass("A", "OK"),
            CheckResult::warn("B", "Warning", "Consider"),
        ];
        let report = HealthReport::new(checks);
        assert_eq!(report.overall, CheckStatus::Warn);
        assert_eq!(report.summary.warnings, 1);
    }

    #[test]
    fn test_should_run_check_no_filter() {
        assert!(should_run_check(None, "workspace"));
        assert!(should_run_check(None, "backend"));
    }

    #[test]
    fn test_should_run_check_with_filter() {
        assert!(should_run_check(Some("workspace"), "workspace"));
        assert!(!should_run_check(Some("workspace"), "backend"));
    }

    #[test]
    fn test_check_status_display() {
        assert_eq!(format!("{}", CheckStatus::Pass), "✓");
        assert_eq!(format!("{}", CheckStatus::Fail), "✗");
        assert_eq!(format!("{}", CheckStatus::Warn), "!");
        assert_eq!(format!("{}", CheckStatus::Skip), "-");
    }

    #[test]
    fn test_health_report_serialization() {
        let checks = vec![CheckResult::pass("Test", "OK")];
        let report = HealthReport::new(checks);
        let json = serde_json::to_string(&report).unwrap();
        assert!(json.contains("\"overall\":\"pass\""));
        assert!(json.contains("\"passed\":1"));
    }
}
