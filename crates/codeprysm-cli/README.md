# codeprysm-cli

[![Crates.io](https://img.shields.io/crates/v/codeprysm-cli.svg)](https://crates.io/crates/codeprysm-cli)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

CLI for CodePrysm code analysis and search.

Part of the [CodePrysm](https://github.com/codeprysm/codeprysm) project.

## Installation

```bash
cargo install codeprysm-cli

# With GPU acceleration (recommended)
cargo install codeprysm-cli --features metal  # macOS
cargo install codeprysm-cli --features cuda   # Linux/Windows
```

## Quick Start

```bash
# Start Qdrant (required for semantic search)
docker run -d --name qdrant -p 6333:6333 -p 6334:6334 qdrant/qdrant:latest

# Initialize your codebase
cd /path/to/your/repo
codeprysm init

# Search your code
codeprysm search "authentication handler"

# Start MCP server for AI assistants
codeprysm mcp
```

## Commands

### `init`

Generate code graph and semantic index:

```bash
codeprysm init --root /path/to/repo
```

### `update`

Incrementally update the index:

```bash
codeprysm update --root /path/to/repo
```

### `search`

Search for code entities:

```bash
codeprysm search "function that handles errors"
codeprysm search --kind callable "authentication"
codeprysm search --limit 20 "database connection"
```

### `mcp`

Start the MCP server:

```bash
codeprysm mcp --root /path/to/repo --qdrant-url http://localhost:6334
```

### `stats`

Show index statistics:

```bash
codeprysm stats --codeprysm-dir .codeprysm
```

## Configuration

CodePrysm looks for configuration in:
1. `.codeprysm/config.toml` in the repository
2. `~/.config/codeprysm/config.toml` global config

Example configuration:

```toml
[qdrant]
url = "http://localhost:6334"

[indexing]
exclude = ["**/node_modules/**", "**/vendor/**"]
```

## License

MIT License - see [LICENSE](https://github.com/codeprysm/codeprysm/blob/main/LICENSE)
