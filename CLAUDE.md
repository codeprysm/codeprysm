# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

CodePrism is a Tree-sitter Code Graph Generator that analyzes code repositories and generates relationship graphs using Tree-sitter AST. The system transforms source code into a searchable knowledge graph enabling semantic code search, dependency analysis, and intelligent code navigation.

### Architecture

The system operates in three main phases:
1. **Code Graph Generation** - Parse source files using Tree-sitter AST into a graph structure (`codeprysm-core`)
2. **Semantic Indexing** - Create embeddings for semantic search using Qdrant (`codeprysm-search`)
3. **MCP Server Integration** - Expose capabilities to AI assistants via MCP protocol (`codeprysm-mcp`)

### Core Components

- **codeprysm-core** (`crates/codeprysm-core/`): Graph generation, tree-sitter parsing, merkle trees
- **codeprysm-search** (`crates/codeprysm-search/`): Qdrant vector search, fastembed embeddings, hybrid search
- **codeprysm-mcp** (`crates/codeprysm-mcp/`): MCP server using rmcp SDK
- **codeprysm-cli** (`crates/codeprysm-cli/`): Unified command-line interface
- **codeprysm-config** (`crates/codeprysm-config/`): Configuration loading and management
- **codeprysm-backend** (`crates/codeprysm-backend/`): Backend abstraction layer

## Development Commands

### Essential Commands (using just)

```bash
# Install dependencies and setup (builds Rust release binaries)
just setup

# Start Qdrant vector database (required for search)
just qdrant-start

# Initialize code search system (generate graph)
just init [repo_dir]
# Outputs to: ./.codeprysm/ (partitioned storage: manifest.json + partitions/)

# Start MCP server for interactive code analysis
just mcp [repo_dir] [qdrant_url]
# Default: just mcp . http://localhost:6334

# Update graph incrementally (faster than full rebuild)
just update [repo_dir]

# Run Rust tests
just rust-test

# Lint code
just rust-lint

# Format code
just rust-fmt
```

### Graph Commands

```bash
# Generate code graph only
just generate-graph <repo_dir>
# Output: ./.codeprysm/ (partitioned storage: manifest.json + partitions/)

# Show graph statistics
just stats
# Uses: ./.codeprysm/manifest.json

# Update repository incrementally
just update-repo <repo_dir>

# Force full rebuild of repository
just rebuild-repo <repo_dir>
```

### Qdrant Commands

```bash
# Start Qdrant in Docker (required for MCP server)
just qdrant-start

# Stop Qdrant
just qdrant-stop

# Check Qdrant status
just qdrant-status
```

## Key Files and Purposes

- `crates/codeprysm-core/src/`: Graph generation, tree-sitter parsing, merkle trees
  - `builder.rs`: GraphBuilder for constructing code graphs
  - `parser.rs`: Tree-sitter parsing and tag extraction
  - `merkle.rs`: Merkle tree change detection
  - `incremental.rs`: Incremental graph updates
  - `main.rs`: CLI binary (`codeprysm-core`)
- `crates/codeprysm-search/src/`: Vector search with Qdrant
  - `client.rs`: Qdrant client wrapper
  - `embeddings.rs`: fastembed integration
  - `hybrid.rs`: Hybrid search (semantic + keyword)
- `crates/codeprysm-mcp/src/`: MCP server library
  - `server.rs`: PrismServer with MCP tools
  - `tools.rs`: Tool implementations
- `crates/codeprysm-core/queries/*.scm`: Tree-sitter query files

### Configuration

- `justfile`: Command automation and shortcuts
- `Cargo.toml`: Rust workspace configuration
- `docker/Dockerfile`: Docker image for MCP server deployment

## Developer Setup

1. Install prerequisites: Docker, Rust 1.85+, just
2. Build: `just setup`
3. Start Qdrant: `just qdrant-start`
4. Initialize graph: `just init`
5. Start MCP server: `just mcp`

### Rust Development

```bash
# Check Rust crates compile
just rust-check

# Build Rust crates (debug)
just rust-build

# Build Rust crates (release)
just rust-build-release

# Run Rust tests
just rust-test

# Format and lint
just rust-fmt
just rust-lint
```

**Rust Crate Structure:**
- `crates/codeprysm-core/` - Graph generation, tree-sitter parsing, merkle trees
- `crates/codeprysm-search/` - Qdrant client, embeddings, hybrid search
- `crates/codeprysm-mcp/` - MCP server with rmcp SDK

## Testing

```bash
# Run all Rust tests
just rust-test

# Run integration tests (all languages)
just rust-test-integration

# Run integration tests for specific language
just rust-test-integration-lang python
```

### Embedding/Search Tests (Important)

Tests involving embeddings require special handling:

```bash
# CRITICAL: Always use --release for embedding tests
# Debug builds are 50-100x slower due to unoptimized tensor operations

# Run e2e tests (requires Qdrant running)
just qdrant-start
cargo test --package codeprysm-mcp --test e2e_integration --features metal --release -- --ignored --test-threads=1
```

## GPU Acceleration

The embedding system (codeprysm-search) supports GPU acceleration via compile-time features:

```bash
# macOS (Apple Silicon) - use Metal
cargo build --features metal --release
cargo test --features metal --release

# Linux (NVIDIA) - use CUDA
cargo build --features cuda --release
cargo test --features cuda --release

# CPU only (default, no feature flag)
cargo build --release
```

## Performance Considerations

- **Default storage location**: `.codeprysm/` directory (automatically excluded from git)
- **Partitioned storage**: Graph stored as SQLite partitions (manifest.json + partitions/*.db)
- **Lazy loading**: Partitions loaded on-demand to reduce memory usage
- **Auto-generation**: MCP server auto-generates graph if missing on startup
- **Incremental updates**: Merkle tree-based change detection for faster updates
- **Parallel parsing**: Uses rayon for parallel file processing
- **Hybrid search**: Semantic (candle/jina) + keyword matching with score fusion
- **Multi-tenant**: Qdrant collections support multiple repositories

## Language Support

Currently supports: Python, JavaScript/TypeScript, C/C++, C#, Go, Rust

Query files in `crates/codeprysm-core/queries/` directory define AST patterns for each language.

**IMPORTANT**: When adding new language support or modifying parsing logic:
- Language-specific parsing rules MUST be implemented in `.scm` grammar files (in `crates/codeprysm-core/queries/` directory)
- The core parser code (`crates/codeprysm-core/src/parser.rs`) MUST remain generic and language-agnostic
- All language-specific patterns, node types, and extraction logic belong in the `.scm` files
- This separation ensures maintainability and makes adding new languages straightforward

## Graph Structure

### Node Types
- **Container**: Structural entities that contain other nodes
  - `kind="repository"`: Root node representing the entire repository (includes git metadata)
  - `kind="file"`: Source files (includes content hash for change detection)
  - `kind="type"`: Classes, structs, interfaces, enums
  - `kind="module"`, `kind="namespace"`, `kind="package"`: Organizational containers
- **Callable**: Executable entities (functions, methods, constructors)
- **Data**: Variables and fields (constants, fields, parameters, locals)

### Edge Types
- **CONTAINS**: Parent-child hierarchy (repository→files, files→classes, classes→methods)
- **USES**: Usage/reference relationships (function calls, type references)
- **DEFINES**: Definition relationships (class→fields, function→parameters)

### Graph Hierarchy
```
Container (kind=repository, name="codeprysm")  ← git metadata (remote, branch, commit)
├── Container (kind=file, name="src/lib.rs")  ← content hash
│   ├── Container (kind=type, name="Node")
│   │   ├── Callable (kind=method, name="new")
│   │   └── Data (kind=field, name="id")
│   └── Callable (kind=function, name="main")
└── Container (kind=file, name="src/parser.rs")
    └── ...
```

## Documentation

- `README.md`: Main project documentation
- `docs/getting-started.md`: User setup guide
- `docs/getting-started-docker.md`: Docker setup guide
- `docs/development/scm-tag-naming-convention.md`: SCM tag syntax reference
- `docs/guides/scm-overlays.md`: Adding scope metadata to code graphs
