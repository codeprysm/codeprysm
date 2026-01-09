//! CodePrysm MCP - MCP server exposing code graph tools to AI assistants
//!
//! This crate provides an MCP (Model Context Protocol) server that exposes
//! code graph navigation and semantic search capabilities to AI assistants.
//!
//! # Tools
//!
//! - **Search**: `search_graph_nodes` - find code by name or description (code/info/hybrid modes)
//! - **Metadata**: `get_node_info` - get entity type, file, line numbers
//! - **Code**: `read_code` - view source code for nodes or file ranges
//! - **Navigation**: `find_references`, `find_outgoing_references`, `find_definitions`, `find_call_chain`
//! - **Exploration**: `find_module_structure` - understand directory organization
//! - **Sync**: `sync_repository`, `get_index_status` - keep index current

pub mod error;
pub mod server;
pub mod tools;

// Re-exports
pub use error::{McpError, Result};
pub use server::{PrismServer, ServerConfig};
