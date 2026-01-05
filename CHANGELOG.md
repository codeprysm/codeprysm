# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.0] - 2025-01-05

### Added

- **Core Graph Generation** (`codeprysm-core`)
  - Tree-sitter AST parsing for 8 languages: Python, JavaScript, TypeScript, C, C++, C#, Go, Rust
  - Semantic node types: Container (classes, structs), Callable (functions, methods), Data (fields, constants)
  - Edge types: CONTAINS (hierarchy), USES (dependencies), DEFINES (definitions)
  - Partitioned SQLite storage with lazy loading for large codebases
  - Merkle tree-based incremental updates
  - Declarative SCM tag system for language-agnostic processing

- **Semantic Search** (`codeprysm-search`)
  - Vector embeddings using JinaBERT (local, GPU-accelerated)
  - Hybrid search combining semantic and keyword matching
  - Qdrant integration for vector storage
  - Support for OpenAI and Azure ML embedding providers
  - Metal (macOS) and CUDA (Linux/Windows) GPU acceleration

- **MCP Server** (`codeprysm-mcp`)
  - Model Context Protocol server for AI assistant integration
  - Tools: semantic search, graph traversal, code navigation
  - Auto-indexing on startup

- **CLI** (`codeprysm-cli`)
  - `codeprysm init` - Generate code graph
  - `codeprysm mcp` - Start MCP server
  - `codeprysm search` - Search codebase
  - `codeprysm stats` - Graph statistics
  - `codeprysm update` - Incremental updates

- **Configuration** (`codeprysm-config`)
  - TOML configuration file support
  - Environment variable overrides

- **Backend Abstraction** (`codeprysm-backend`)
  - Local and remote backend support
  - Multi-backend workspace management

### Infrastructure

- Comprehensive CI/CD with GitHub Actions
- Multi-platform testing (Ubuntu, macOS)
- Docker support for containerized deployment
- MIT License

[Unreleased]: https://github.com/codeprysm/codeprysm/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/codeprysm/codeprysm/releases/tag/v0.1.0
