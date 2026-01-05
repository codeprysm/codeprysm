//! CodePrysm MCP - MCP server exposing code graph tools to AI assistants
//!
//! This crate provides an MCP (Model Context Protocol) server that exposes
//! code graph navigation and semantic search capabilities to AI assistants.
//!
//! # Features
//!
//! - **Hybrid search**: Semantic + code search via `search_graph_nodes`
//! - **Graph navigation**: References, definitions, call chains
//! - **Code reading**: Source file access with line ranges
//! - **Auto-sync**: Background repository synchronization

pub mod error;
pub mod server;
pub mod tools;

// Re-exports
pub use error::{McpError, Result};
pub use server::{PrismServer, ServerConfig};
