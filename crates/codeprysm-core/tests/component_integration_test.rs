//! Integration tests for ComponentBuilder
//!
//! Tests component discovery and dependency edge creation across multiple languages.

use codeprysm_core::builder::ComponentBuilder;
use codeprysm_core::graph::{EdgeType, NodeType, PetCodeGraph};
use std::path::Path;

/// Helper to get the path to component_repos fixtures
fn fixture_path(name: &str) -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/component_repos")
        .join(name)
}

/// Helper to count edges of a specific type
fn count_edges(graph: &PetCodeGraph, edge_type: EdgeType) -> usize {
    graph.edges_by_type(edge_type).count()
}

/// Helper to get component node IDs
fn component_node_ids(graph: &PetCodeGraph) -> Vec<String> {
    graph
        .iter_nodes()
        .filter(|n| n.node_type == NodeType::Container && n.kind == Some("component".to_string()))
        .map(|n| n.id.clone())
        .collect()
}

/// Helper to find DependsOn edges from a component
fn depends_on_targets(graph: &PetCodeGraph, from_id: &str) -> Vec<String> {
    graph
        .edges_by_type(EdgeType::DependsOn)
        .filter(|(from, _to, _data)| from.id == from_id)
        .map(|(_from, to, _data)| to.id.clone())
        .collect()
}

// ============================================================================
// Rust Workspace Tests
// ============================================================================

#[test]
fn test_rust_workspace_discovers_all_crates() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("rust-workspace");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    // Should find: root workspace + 3 crates (core, utils, cli)
    assert_eq!(
        components.len(),
        4,
        "Expected 4 components (workspace + 3 crates)"
    );

    let names: Vec<_> = components.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"myapp-core"), "Should find myapp-core");
    assert!(names.contains(&"myapp-utils"), "Should find myapp-utils");
    assert!(names.contains(&"myapp-cli"), "Should find myapp-cli");
}

#[test]
fn test_rust_workspace_creates_dependency_edges() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("rust-workspace");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    let mut graph = PetCodeGraph::new();
    builder
        .add_to_graph(&mut graph, "rust-workspace", &components)
        .expect("Failed to add to graph");

    // Check DependsOn edges
    let depends_on_count = count_edges(&graph, EdgeType::DependsOn);

    // Expected dependencies:
    // - myapp-utils -> myapp-core (path = "../core")
    // - myapp-cli -> myapp-core (path = "../core")
    // - myapp-cli -> myapp-utils (path = "../utils")
    assert_eq!(depends_on_count, 3, "Expected 3 DependsOn edges");

    // Verify specific dependencies
    let cli_id = "component:rust-workspace:crates/cli".to_string();
    let utils_id = "component:rust-workspace:crates/utils".to_string();
    let core_id = "component:rust-workspace:crates/core".to_string();

    let cli_deps = depends_on_targets(&graph, &cli_id);
    assert!(cli_deps.contains(&core_id), "CLI should depend on core");
    assert!(cli_deps.contains(&utils_id), "CLI should depend on utils");

    let utils_deps = depends_on_targets(&graph, &utils_id);
    assert!(utils_deps.contains(&core_id), "Utils should depend on core");
}

#[test]
fn test_rust_workspace_marks_workspace_root() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("rust-workspace");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    // Find the workspace root component
    let workspace_root = components
        .iter()
        .find(|c| c.directory.is_empty() || c.directory == ".")
        .expect("Should find workspace root");

    assert!(
        workspace_root.info.is_workspace_root,
        "Root should be marked as workspace root"
    );
    assert!(
        !workspace_root.info.workspace_members.is_empty(),
        "Workspace root should have members"
    );
}

// ============================================================================
// npm Workspace Tests
// ============================================================================

#[test]
fn test_npm_workspace_discovers_all_packages() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("npm-workspace");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    // Should find: root + 3 packages (shared, web, api)
    assert_eq!(
        components.len(),
        4,
        "Expected 4 components (root + 3 packages)"
    );

    let names: Vec<_> = components.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.contains(&"@myapp/shared"),
        "Should find @myapp/shared"
    );
    assert!(names.contains(&"@myapp/web"), "Should find @myapp/web");
    assert!(names.contains(&"@myapp/api"), "Should find @myapp/api");
}

#[test]
fn test_npm_workspace_creates_dependency_edges() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("npm-workspace");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    let mut graph = PetCodeGraph::new();
    builder
        .add_to_graph(&mut graph, "npm-workspace", &components)
        .expect("Failed to add to graph");

    // Expected dependencies (workspace:*):
    // - @myapp/web -> @myapp/shared
    // - @myapp/api -> @myapp/shared
    let depends_on_count = count_edges(&graph, EdgeType::DependsOn);
    assert_eq!(depends_on_count, 2, "Expected 2 DependsOn edges");
}

// ============================================================================
// Python Monorepo Tests
// ============================================================================

#[test]
fn test_python_monorepo_discovers_all_packages() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("python-monorepo");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    // Should find: shared, api, cli (no root pyproject.toml)
    assert_eq!(components.len(), 3, "Expected 3 Python packages");

    let names: Vec<_> = components.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"myapp-shared"), "Should find myapp-shared");
    assert!(names.contains(&"myapp-api"), "Should find myapp-api");
    assert!(names.contains(&"myapp-cli"), "Should find myapp-cli");
}

#[test]
fn test_python_monorepo_creates_poetry_path_deps() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("python-monorepo");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    let mut graph = PetCodeGraph::new();
    builder
        .add_to_graph(&mut graph, "python-monorepo", &components)
        .expect("Failed to add to graph");

    // Expected dependencies (poetry path deps):
    // - myapp-api -> myapp-shared (path = "../shared")
    // - myapp-cli -> myapp-shared (path = "../shared")
    // - myapp-cli -> myapp-api (path = "../api")
    let depends_on_count = count_edges(&graph, EdgeType::DependsOn);
    assert_eq!(depends_on_count, 3, "Expected 3 DependsOn edges");

    // Verify CLI has both dependencies
    let cli_id = "component:python-monorepo:cli".to_string();
    let cli_deps = depends_on_targets(&graph, &cli_id);
    assert_eq!(cli_deps.len(), 2, "CLI should have 2 dependencies");
}

// ============================================================================
// .NET Solution Tests
// ============================================================================

#[test]
fn test_dotnet_solution_discovers_all_projects() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("dotnet-solution");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    // Should find: Shared, Api, Web
    assert_eq!(components.len(), 3, "Expected 3 .NET projects");

    let names: Vec<_> = components.iter().map(|c| c.name.as_str()).collect();
    assert!(names.contains(&"MyApp.Shared"), "Should find MyApp.Shared");
    assert!(names.contains(&"MyApp.Api"), "Should find MyApp.Api");
    assert!(names.contains(&"MyApp.Web"), "Should find MyApp.Web");
}

#[test]
fn test_dotnet_solution_creates_project_reference_edges() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("dotnet-solution");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    let mut graph = PetCodeGraph::new();
    builder
        .add_to_graph(&mut graph, "dotnet-solution", &components)
        .expect("Failed to add to graph");

    // Expected dependencies (ProjectReference):
    // - MyApp.Api -> MyApp.Shared
    // - MyApp.Web -> MyApp.Shared
    // - MyApp.Web -> MyApp.Api
    let depends_on_count = count_edges(&graph, EdgeType::DependsOn);
    assert_eq!(depends_on_count, 3, "Expected 3 DependsOn edges");

    // Verify Web has both dependencies
    let web_id = "component:dotnet-solution:Web".to_string();
    let web_deps = depends_on_targets(&graph, &web_id);
    assert_eq!(web_deps.len(), 2, "Web should have 2 dependencies");
}

// ============================================================================
// Go Workspace Tests
// ============================================================================

#[test]
fn test_go_workspace_discovers_all_modules() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("go-workspace");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    // Should find: shared, api, cmd
    assert_eq!(components.len(), 3, "Expected 3 Go modules");

    let names: Vec<_> = components.iter().map(|c| c.name.as_str()).collect();
    assert!(
        names.iter().any(|n| n.contains("shared")),
        "Should find shared module"
    );
    assert!(
        names.iter().any(|n| n.contains("api")),
        "Should find api module"
    );
    assert!(
        names.iter().any(|n| n.contains("cmd")),
        "Should find cmd module"
    );
}

#[test]
fn test_go_workspace_creates_replace_directive_edges() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("go-workspace");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    let mut graph = PetCodeGraph::new();
    builder
        .add_to_graph(&mut graph, "go-workspace", &components)
        .expect("Failed to add to graph");

    // Expected dependencies (replace directives):
    // - api -> shared (replace => ../shared)
    // - cmd -> shared (replace => ../shared)
    // - cmd -> api (replace => ../api)
    let depends_on_count = count_edges(&graph, EdgeType::DependsOn);
    assert_eq!(
        depends_on_count, 3,
        "Expected 3 DependsOn edges from replace directives"
    );

    // Verify cmd has both dependencies
    let cmd_id = "component:go-workspace:cmd".to_string();
    let cmd_deps = depends_on_targets(&graph, &cmd_id);
    assert_eq!(cmd_deps.len(), 2, "CMD should have 2 dependencies");
}

// ============================================================================
// Cross-cutting Tests
// ============================================================================

#[test]
fn test_component_nodes_have_correct_metadata() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("rust-workspace");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    let mut graph = PetCodeGraph::new();
    builder
        .add_to_graph(&mut graph, "rust-workspace", &components)
        .expect("Failed to add to graph");

    // Verify all component nodes have correct type and kind
    for node_id in component_node_ids(&graph) {
        let node = graph.get_node(&node_id).expect("Node should exist");
        assert_eq!(
            node.node_type,
            NodeType::Container,
            "Component should be Container"
        );
        assert_eq!(
            node.kind,
            Some("component".to_string()),
            "Component should have kind='component'"
        );

        // Verify metadata
        let metadata = &node.metadata;
        assert!(
            metadata.manifest_path.is_some(),
            "Component should have manifest_path"
        );
    }
}

#[test]
fn test_path_index_is_populated() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("rust-workspace");

    let components = builder
        .discover_components(&root, &[])
        .expect("Failed to discover components");

    // Path index should be empty before add_to_graph
    assert!(
        builder.path_index().is_empty(),
        "Path index should be empty before add_to_graph"
    );

    let mut graph = PetCodeGraph::new();
    builder
        .add_to_graph(&mut graph, "rust-workspace", &components)
        .expect("Failed to add to graph");

    // Path index should be populated after add_to_graph
    assert!(
        !builder.path_index().is_empty(),
        "Path index should be populated after add_to_graph"
    );

    // Should have entries for each component directory
    assert!(
        builder.path_index().len() >= 3,
        "Path index should have at least 3 entries for crates"
    );
}

#[test]
fn test_exclude_patterns_work() {
    let mut builder = ComponentBuilder::new().expect("Failed to create builder");
    let root = fixture_path("rust-workspace");

    // Exclude the cli crate
    let exclude = vec!["**/cli/**".to_string()];
    let components = builder
        .discover_components(&root, &exclude)
        .expect("Failed to discover components");

    // Should find: root + core + utils (not cli)
    assert_eq!(components.len(), 3, "Expected 3 components (cli excluded)");

    let names: Vec<_> = components.iter().map(|c| c.name.as_str()).collect();
    assert!(!names.contains(&"myapp-cli"), "CLI should be excluded");
}
