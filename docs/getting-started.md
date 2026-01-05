# Getting Started

CodePrysm is a powerful tool for analyzing code repositories and generating relationship graphs using Tree-sitter abstract syntax trees. This system transforms source code into a searchable knowledge graph, enabling semantic code search, dependency analysis, and intelligent code navigation.

## Table of Contents

1. [Install Prerequisites](#1-install-prerequisites)
2. [Install CodePrysm](#2-install-codeprysm)
3. [Start Qdrant](#3-start-qdrant)
4. [Run Initial Indexing](#4-run-initial-indexing)
5. [Setup MCP Server](#5-setup-mcp-server)
6. [Verify Installation](#6-verify-installation)

For Docker-based setup, see the [Docker Setup Guide](./getting-started-docker.md).

## 1. Install Prerequisites

- **Rust 1.85+**: [Install Rust](https://rustup.rs/)
- **Docker**: Required for running Qdrant vector database

Optional:
- **[just](https://github.com/casey/just)**: Command runner for development shortcuts

## 2. Install CodePrysm

### From crates.io (Recommended)

```bash
cargo install codeprysm-cli
```

### From Source

```bash
git clone https://github.com/codeprysm/codeprysm.git
cd codeprysm
cargo build --release
```

The binary will be available at `./target/release/codeprysm`.

### GPU Acceleration (Optional)

For faster embedding generation, build with GPU support:

```bash
# macOS (Apple Silicon)
cargo install codeprysm-cli --features metal

# Linux (NVIDIA)
cargo install codeprysm-cli --features cuda
```

## 3. Start Qdrant

CodePrysm uses Qdrant for semantic search. Start it with Docker:

```bash
docker run -d --name qdrant \
  -p 6333:6333 -p 6334:6334 \
  -v qdrant_storage:/qdrant/storage \
  qdrant/qdrant:latest
```

Or using just:

```bash
just qdrant-start
```

## 4. Run Initial Indexing

Navigate to your repository and run the initial indexing:

```bash
cd /path/to/your/repo
codeprysm init
```

This creates a `.codeprysm/` directory containing:
- Code graph with all entities and relationships
- Merkle tree for incremental updates
- Semantic embeddings indexed in Qdrant

The `.codeprysm/` directory is automatically added to `.gitignore`.

### Incremental Updates

After the initial indexing, use `update` for faster incremental updates:

```bash
codeprysm update
```

## 5. Setup MCP Server

### Option A: VS Code Integration

Create `.vscode/mcp.json` in your repository:

```json
{
    "servers": {
        "codeprysm": {
            "type": "stdio",
            "command": "codeprysm",
            "args": ["mcp"]
        }
    }
}
```

### Option B: Claude Desktop Integration

Add to your Claude Desktop configuration (`~/Library/Application Support/Claude/claude_desktop_config.json` on macOS):

```json
{
  "mcpServers": {
    "codeprysm": {
      "command": "codeprysm",
      "args": ["mcp", "--root", "/path/to/your/repo"]
    }
  }
}
```

### Option C: Manual MCP Server

Start the MCP server manually:

```bash
codeprysm mcp --root /path/to/your/repo --qdrant-url http://localhost:6334
```

## 6. Verify Installation

### Check Qdrant Connection

```bash
codeprysm stats
```

### Test Search

```bash
codeprysm search "function that handles authentication"
```

### In VS Code

1. Open the Chat panel
2. Select 'Agent' chat mode
3. Test the search: `#search_graph_nodes authentication functions`

You should see relevant code snippets from your codebase.

## Troubleshooting

### Qdrant Connection Issues

Ensure Qdrant is running:

```bash
docker ps | grep qdrant
```

Check Qdrant health:

```bash
curl http://localhost:6333/health
```

### Slow Indexing

- Enable GPU acceleration with `--features metal` (macOS) or `--features cuda` (Linux)
- For large repositories (>10K files), initial indexing may take 5-20 minutes

### Memory Issues

For very large repositories:
- Ensure sufficient RAM (4GB+ for medium repos, 16GB+ for large repos)
- Use incremental updates (`codeprysm update`) instead of full rebuilds

## Next Steps

- [Code Graph Generation](./features/code-graph-generation.md) - How the code graph works
- [Semantic Search Guide](./features/semantic-search-guide.md) - Search best practices
- [MCP Server Integration](./features/mcp-server-integration.md) - Advanced MCP configuration
