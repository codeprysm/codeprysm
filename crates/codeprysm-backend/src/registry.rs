//! Workspace registry for multi-workspace support.
//!
//! Manages multiple registered workspaces with persistent storage in global config.
//! Provides workspace registration, activation, and discovery.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use codeprysm_config::{ConfigLoader, PrismConfig, WorkspaceConfig};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

use crate::error::BackendError;
use crate::local::LocalBackend;

/// Information about a registered workspace.
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    /// Workspace name (user-defined or derived from path)
    pub name: String,

    /// Absolute path to workspace root
    pub path: PathBuf,

    /// Whether a graph exists for this workspace
    pub has_graph: bool,

    /// Whether the search index exists
    pub has_index: bool,

    /// Whether this is the active workspace
    pub is_active: bool,
}

/// Registry for managing multiple workspaces.
///
/// Provides:
/// - Workspace registration and persistence
/// - Active workspace selection
/// - Backend creation for individual workspaces
pub struct WorkspaceRegistry {
    /// Configuration loader (handles global config persistence)
    config_loader: ConfigLoader,

    /// Current configuration state
    config: Arc<RwLock<PrismConfig>>,

    /// Cached backends for registered workspaces
    backends: Arc<RwLock<HashMap<String, Arc<LocalBackend>>>>,
}

impl WorkspaceRegistry {
    /// Create a new workspace registry.
    ///
    /// Loads the global configuration to populate initial workspace list.
    pub async fn new() -> Result<Self, BackendError> {
        let mut config_loader = ConfigLoader::new();

        // Load global config (or use defaults)
        let config = config_loader
            .load_global()
            .map_err(|e| BackendError::with_context("loading global config", e.to_string()))?
            .unwrap_or_default();

        Ok(Self {
            config_loader,
            config: Arc::new(RwLock::new(config)),
            backends: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create a registry with a specific configuration.
    ///
    /// Useful for testing or when configuration is already loaded.
    pub fn with_config(config: PrismConfig) -> Self {
        Self {
            config_loader: ConfigLoader::new(),
            config: Arc::new(RwLock::new(config)),
            backends: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a registry with a custom config loader.
    ///
    /// Useful for testing with custom config directories.
    pub fn with_loader(config_loader: ConfigLoader, config: PrismConfig) -> Self {
        Self {
            config_loader,
            config: Arc::new(RwLock::new(config)),
            backends: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register a workspace.
    ///
    /// # Arguments
    /// * `name` - Unique name for the workspace
    /// * `path` - Path to workspace root (will be canonicalized)
    ///
    /// # Returns
    /// The canonicalized path that was registered.
    pub async fn register(
        &self,
        name: impl Into<String>,
        path: impl AsRef<Path>,
    ) -> Result<PathBuf, BackendError> {
        let name = name.into();
        let path = path.as_ref();

        // Canonicalize path
        let canonical_path = path
            .canonicalize()
            .map_err(|e| BackendError::with_context("canonicalizing path", e.to_string()))?;

        // Verify path exists and is a directory
        if !canonical_path.is_dir() {
            return Err(BackendError::with_context(
                "registering workspace",
                format!("'{}' is not a directory", canonical_path.display()),
            ));
        }

        info!("Registering workspace '{}' at {:?}", name, canonical_path);

        // Update config
        {
            let mut config = self.config.write().await;
            config
                .workspace
                .workspaces
                .insert(name.clone(), canonical_path.clone());
        }

        // Persist to global config
        self.save_config().await?;

        // Invalidate cached backend for this workspace
        {
            let mut backends = self.backends.write().await;
            backends.remove(&name);
        }

        Ok(canonical_path)
    }

    /// Unregister a workspace.
    ///
    /// Removes the workspace from the registry. Does not delete any files.
    pub async fn unregister(&self, name: &str) -> Result<bool, BackendError> {
        let removed = {
            let mut config = self.config.write().await;
            config.workspace.workspaces.remove(name).is_some()
        };

        if removed {
            info!("Unregistered workspace '{}'", name);

            // If this was the active workspace, clear active
            {
                let mut config = self.config.write().await;
                if config.workspace.active.as_deref() == Some(name) {
                    config.workspace.active = None;
                }
            }

            // Persist changes
            self.save_config().await?;

            // Remove cached backend
            {
                let mut backends = self.backends.write().await;
                backends.remove(name);
            }
        } else {
            warn!("Workspace '{}' not found in registry", name);
        }

        Ok(removed)
    }

    /// Get the active workspace name.
    pub async fn active(&self) -> Option<String> {
        let config = self.config.read().await;
        config.workspace.active.clone()
    }

    /// Set the active workspace.
    ///
    /// The workspace must be registered.
    pub async fn set_active(&self, name: &str) -> Result<(), BackendError> {
        // Verify workspace exists
        let exists = {
            let config = self.config.read().await;
            config.workspace.workspaces.contains_key(name)
        };

        if !exists {
            return Err(BackendError::with_context(
                "setting active workspace",
                format!("workspace '{}' not registered", name),
            ));
        }

        info!("Setting active workspace to '{}'", name);

        {
            let mut config = self.config.write().await;
            config.workspace.active = Some(name.to_string());
        }

        self.save_config().await
    }

    /// Clear the active workspace.
    pub async fn clear_active(&self) -> Result<(), BackendError> {
        {
            let mut config = self.config.write().await;
            config.workspace.active = None;
        }

        self.save_config().await
    }

    /// Get workspace path by name.
    pub async fn get(&self, name: &str) -> Option<PathBuf> {
        let config = self.config.read().await;
        config.workspace.workspaces.get(name).cloned()
    }

    /// Get the active workspace path.
    pub async fn active_path(&self) -> Option<PathBuf> {
        let config = self.config.read().await;
        config
            .workspace
            .active
            .as_ref()
            .and_then(|name| config.workspace.workspaces.get(name).cloned())
    }

    /// List all registered workspaces.
    pub async fn list(&self) -> Vec<WorkspaceInfo> {
        let config = self.config.read().await;
        let active = config.workspace.active.as_deref();

        config
            .workspace
            .workspaces
            .iter()
            .map(|(name, path)| {
                let prism_dir = config.prism_dir(path);
                let has_graph = prism_dir.join("manifest.json").exists();
                let has_index = has_graph; // Simplified check; could query Qdrant

                WorkspaceInfo {
                    name: name.clone(),
                    path: path.clone(),
                    has_graph,
                    has_index,
                    is_active: active == Some(name.as_str()),
                }
            })
            .collect()
    }

    /// Get workspace count.
    pub async fn count(&self) -> usize {
        let config = self.config.read().await;
        config.workspace.workspaces.len()
    }

    /// Check if cross-workspace search is enabled.
    pub async fn cross_workspace_search_enabled(&self) -> bool {
        let config = self.config.read().await;
        config.workspace.cross_workspace_search
    }

    /// Enable or disable cross-workspace search.
    pub async fn set_cross_workspace_search(&self, enabled: bool) -> Result<(), BackendError> {
        {
            let mut config = self.config.write().await;
            config.workspace.cross_workspace_search = enabled;
        }

        self.save_config().await
    }

    /// Get or create a backend for a workspace.
    ///
    /// Backends are cached for reuse.
    pub async fn backend(&self, name: &str) -> Result<Arc<LocalBackend>, BackendError> {
        // Check cache first
        {
            let backends = self.backends.read().await;
            if let Some(backend) = backends.get(name) {
                return Ok(Arc::clone(backend));
            }
        }

        // Get workspace path
        let path = self.get(name).await.ok_or_else(|| {
            BackendError::with_context(
                "getting backend",
                format!("workspace '{}' not registered", name),
            )
        })?;

        // Get config
        let config = self.config.read().await.clone();

        // Create backend
        let backend = LocalBackend::with_repo_id(&config, &path, name).await?;
        let backend = Arc::new(backend);

        // Cache it
        {
            let mut backends = self.backends.write().await;
            backends.insert(name.to_string(), Arc::clone(&backend));
        }

        debug!("Created backend for workspace '{}'", name);
        Ok(backend)
    }

    /// Get backend for the active workspace.
    pub async fn active_backend(&self) -> Result<Arc<LocalBackend>, BackendError> {
        let name = self.active().await.ok_or_else(|| {
            BackendError::with_context("getting active backend", "no active workspace set")
        })?;

        self.backend(&name).await
    }

    /// Get all backends for registered workspaces.
    ///
    /// Only returns backends for workspaces that have graphs.
    pub async fn all_backends(&self) -> Result<Vec<Arc<LocalBackend>>, BackendError> {
        let workspaces = self.list().await;
        let mut backends = Vec::new();

        for ws in workspaces {
            if ws.has_graph {
                match self.backend(&ws.name).await {
                    Ok(backend) => backends.push(backend),
                    Err(e) => {
                        warn!("Failed to create backend for '{}': {}", ws.name, e);
                    }
                }
            }
        }

        Ok(backends)
    }

    /// Get the current configuration.
    pub async fn config(&self) -> PrismConfig {
        self.config.read().await.clone()
    }

    /// Get the workspace configuration section.
    pub async fn workspace_config(&self) -> WorkspaceConfig {
        self.config.read().await.workspace.clone()
    }

    /// Save the current configuration to global config file.
    async fn save_config(&self) -> Result<(), BackendError> {
        let config = self.config.read().await.clone();

        self.config_loader
            .save_global(&config)
            .map_err(|e| BackendError::with_context("saving global config", e.to_string()))?;

        debug!("Saved workspace registry to global config");
        Ok(())
    }

    /// Auto-discover workspaces from a directory.
    ///
    /// Searches for directories containing `.codeprysm/manifest.json` up to max_depth.
    pub async fn discover(
        &self,
        root: impl AsRef<Path>,
        max_depth: usize,
    ) -> Result<Vec<PathBuf>, BackendError> {
        let root = root.as_ref();
        let mut discovered = Vec::new();

        self.discover_recursive(root, 0, max_depth, &mut discovered)?;

        info!(
            "Discovered {} workspaces under {:?}",
            discovered.len(),
            root
        );
        Ok(discovered)
    }

    #[allow(clippy::only_used_in_recursion)]
    fn discover_recursive(
        &self,
        dir: &Path,
        depth: usize,
        max_depth: usize,
        discovered: &mut Vec<PathBuf>,
    ) -> Result<(), BackendError> {
        if depth > max_depth {
            return Ok(());
        }

        // Check if this directory has a prism graph
        let prism_manifest = dir.join(".codeprysm").join("manifest.json");
        if prism_manifest.exists() {
            discovered.push(dir.to_path_buf());
        }

        // Recurse into subdirectories
        let entries = std::fs::read_dir(dir)
            .map_err(|e| BackendError::with_context("reading directory", e.to_string()))?;

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                // Skip hidden directories and common excludes
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
                if !name.starts_with('.') && !matches!(name, "node_modules" | "target" | "vendor") {
                    self.discover_recursive(&path, depth + 1, max_depth, discovered)?;
                }
            }
        }

        Ok(())
    }

    /// Register all discovered workspaces.
    ///
    /// Names are derived from directory names or specified prefix.
    pub async fn register_discovered(
        &self,
        paths: &[PathBuf],
        prefix: Option<&str>,
    ) -> Result<usize, BackendError> {
        let mut registered = 0;

        for path in paths {
            let name = if let Some(prefix) = prefix {
                format!(
                    "{}/{}",
                    prefix,
                    path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("unknown")
                )
            } else {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("unknown")
                    .to_string()
            };

            match self.register(&name, path).await {
                Ok(_) => {
                    registered += 1;
                    info!("Registered '{}' at {:?}", name, path);
                }
                Err(e) => {
                    warn!("Failed to register {:?}: {}", path, e);
                }
            }
        }

        Ok(registered)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_workspace(temp: &TempDir, name: &str, with_graph: bool) -> PathBuf {
        let ws_dir = temp.path().join(name);
        std::fs::create_dir_all(&ws_dir).unwrap();

        if with_graph {
            let prism_dir = ws_dir.join(".codeprysm");
            std::fs::create_dir_all(&prism_dir).unwrap();
            std::fs::write(prism_dir.join("manifest.json"), "{}").unwrap();
        }

        ws_dir
    }

    #[tokio::test]
    async fn test_register_workspace() {
        let temp = TempDir::new().unwrap();
        let ws_path = create_workspace(&temp, "test-project", false);

        let registry = WorkspaceRegistry::with_config(PrismConfig::default());

        let result = registry.register("test", &ws_path).await;
        assert!(result.is_ok());

        let path = registry.get("test").await;
        assert!(path.is_some());
        assert_eq!(path.unwrap(), ws_path.canonicalize().unwrap());
    }

    #[tokio::test]
    async fn test_unregister_workspace() {
        let temp = TempDir::new().unwrap();
        let ws_path = create_workspace(&temp, "test-project", false);

        let registry = WorkspaceRegistry::with_config(PrismConfig::default());
        registry.register("test", &ws_path).await.unwrap();

        let removed = registry.unregister("test").await.unwrap();
        assert!(removed);

        let path = registry.get("test").await;
        assert!(path.is_none());
    }

    #[tokio::test]
    async fn test_active_workspace() {
        let temp = TempDir::new().unwrap();
        let ws_path = create_workspace(&temp, "test-project", false);

        let registry = WorkspaceRegistry::with_config(PrismConfig::default());
        registry.register("test", &ws_path).await.unwrap();

        // Initially no active workspace
        assert!(registry.active().await.is_none());

        // Set active
        registry.set_active("test").await.unwrap();
        assert_eq!(registry.active().await, Some("test".to_string()));

        // Clear active
        registry.clear_active().await.unwrap();
        assert!(registry.active().await.is_none());
    }

    #[tokio::test]
    async fn test_set_active_unregistered_fails() {
        let registry = WorkspaceRegistry::with_config(PrismConfig::default());

        let result = registry.set_active("nonexistent").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_list_workspaces() {
        let temp = TempDir::new().unwrap();
        let ws1 = create_workspace(&temp, "project-a", true);
        let ws2 = create_workspace(&temp, "project-b", false);

        let registry = WorkspaceRegistry::with_config(PrismConfig::default());
        registry.register("a", &ws1).await.unwrap();
        registry.register("b", &ws2).await.unwrap();
        registry.set_active("a").await.unwrap();

        let list = registry.list().await;
        assert_eq!(list.len(), 2);

        let a = list.iter().find(|w| w.name == "a").unwrap();
        assert!(a.has_graph);
        assert!(a.is_active);

        let b = list.iter().find(|w| w.name == "b").unwrap();
        assert!(!b.has_graph);
        assert!(!b.is_active);
    }

    #[tokio::test]
    async fn test_discover_workspaces() {
        let temp = TempDir::new().unwrap();

        // Create some nested workspaces
        create_workspace(&temp, "projects/rust-app", true);
        create_workspace(&temp, "projects/python-lib", true);
        create_workspace(&temp, "projects/node-app", false); // No graph
        create_workspace(&temp, "archived/old-project", true);

        let registry = WorkspaceRegistry::with_config(PrismConfig::default());
        let discovered = registry.discover(temp.path(), 3).await.unwrap();

        // Should find workspaces with graphs
        assert_eq!(discovered.len(), 3);
    }

    #[tokio::test]
    async fn test_register_nonexistent_fails() {
        let registry = WorkspaceRegistry::with_config(PrismConfig::default());

        let result = registry.register("test", "/nonexistent/path").await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_cross_workspace_search_toggle() {
        let registry = WorkspaceRegistry::with_config(PrismConfig::default());

        // Default is enabled
        assert!(registry.cross_workspace_search_enabled().await);

        // Disable
        registry.set_cross_workspace_search(false).await.unwrap();
        assert!(!registry.cross_workspace_search_enabled().await);

        // Re-enable
        registry.set_cross_workspace_search(true).await.unwrap();
        assert!(registry.cross_workspace_search_enabled().await);
    }

    #[tokio::test]
    async fn test_workspace_count() {
        let temp = TempDir::new().unwrap();
        let ws1 = create_workspace(&temp, "p1", false);
        let ws2 = create_workspace(&temp, "p2", false);

        let registry = WorkspaceRegistry::with_config(PrismConfig::default());
        assert_eq!(registry.count().await, 0);

        registry.register("a", &ws1).await.unwrap();
        assert_eq!(registry.count().await, 1);

        registry.register("b", &ws2).await.unwrap();
        assert_eq!(registry.count().await, 2);

        registry.unregister("a").await.unwrap();
        assert_eq!(registry.count().await, 1);
    }

    #[tokio::test]
    async fn test_unregister_active_clears_active() {
        let temp = TempDir::new().unwrap();
        let ws_path = create_workspace(&temp, "test", false);

        let registry = WorkspaceRegistry::with_config(PrismConfig::default());
        registry.register("test", &ws_path).await.unwrap();
        registry.set_active("test").await.unwrap();

        assert_eq!(registry.active().await, Some("test".to_string()));

        registry.unregister("test").await.unwrap();

        // Active should be cleared
        assert!(registry.active().await.is_none());
    }
}
