//! CodePrysm Backend - Abstraction layer for code search and indexing
//!
//! This crate provides a unified interface for:
//! - Semantic code search
//! - Graph node queries
//! - Index management
//! - Multi-workspace support
//!
//! ## Backend Types
//!
//! - [`LocalBackend`]: Direct access to file system and Qdrant for local operations
//! - [`RemoteBackend`]: HTTP client for connecting to a CodePrysm server (future)
//! - [`MultiWorkspaceBackend`]: Aggregates operations across multiple workspaces
//!
//! ## Workspace Registry
//!
//! The [`WorkspaceRegistry`] manages multiple registered workspaces and provides:
//! - Workspace registration and persistence
//! - Active workspace selection
//! - Cross-workspace search support
//!
//! ## Example
//!
//! ```ignore
//! use codeprysm_backend::{Backend, LocalBackend, WorkspaceRegistry, MultiWorkspaceBackend};
//! use codeprysm_config::PrismConfig;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Single workspace usage
//!     let config = PrismConfig::default();
//!     let backend = LocalBackend::new(&config, "/path/to/workspace").await?;
//!     let results = backend.search("authentication logic", 10, None).await?;
//!
//!     // Multi-workspace usage
//!     let registry = WorkspaceRegistry::new().await?;
//!     registry.register("project-a", "/path/to/project-a").await?;
//!     registry.register("project-b", "/path/to/project-b").await?;
//!
//!     let multi = MultiWorkspaceBackend::new(Arc::new(registry));
//!     let results = multi.search("authentication", 20, None).await?;
//!
//!     Ok(())
//! }
//! ```

mod error;
mod local;
mod multi;
mod registry;
mod remote;
mod traits;
mod types;

pub use codeprysm_search::{ModelStatus, ProviderStatus};
pub use error::BackendError;
pub use local::LocalBackend;
pub use multi::MultiWorkspaceBackend;
pub use registry::{WorkspaceInfo, WorkspaceRegistry};
pub use remote::RemoteBackend;
pub use traits::Backend;
pub use types::*;

/// Result type for backend operations.
pub type Result<T> = std::result::Result<T, BackendError>;
