# codeprysm-mcp

[![Crates.io](https://img.shields.io/crates/v/codeprysm-mcp.svg)](https://crates.io/crates/codeprysm-mcp)
[![Documentation](https://docs.rs/codeprysm-mcp/badge.svg)](https://docs.rs/codeprysm-mcp)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

MCP server for AI-powered code exploration.

Part of the [CodePrism](https://github.com/codeprysm/codeprysm) project.

## Features

- **Model Context Protocol**: Standard interface for AI assistants
- **Semantic Code Search**: Natural language queries via `search_graph_nodes`
- **Graph Navigation**: Traverse code relationships and dependencies
- **Auto-Indexing**: Automatically generates graph if missing
- **Incremental Updates**: Keeps index in sync with code changes

## Installation

```toml
[dependencies]
codeprysm-mcp = "0.1"

# With GPU acceleration
codeprysm-mcp = { version = "0.1", features = ["metal"] }  # macOS
codeprysm-mcp = { version = "0.1", features = ["cuda"] }   # Linux/Windows
```

## Usage

### As a Library

```rust
use codeprysm_mcp::PrismServer;
use std::path::Path;

// Create and run the MCP server
let server = PrismServer::new(
    Path::new("/path/to/repo"),
    "http://localhost:6334",
).await?;

server.run().await?;
```

### VS Code Integration

Create `.vscode/mcp.json`:

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

### Claude Desktop Integration

Add to `claude_desktop_config.json`:

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

## MCP Tools

| Tool | Description |
|------|-------------|
| `search_graph_nodes` | Find code by name or description (semantic + code search) |
| `get_node_info` | Get metadata about a code entity (type, file, lines) |
| `read_code` | Read source code for a node or file range |
| `find_references` | Find code that uses/calls this node (incoming) |
| `find_outgoing_references` | Find what this node calls/uses (dependencies) |
| `find_definitions` | Find entities defined inside this node |
| `find_call_chain` | Trace execution paths (upstream/downstream) |
| `find_module_structure` | Explore directory organization |
| `sync_repository` | Trigger re-indexing after code changes |
| `get_index_status` | Check indexing status and progress |

## License

MIT License - see [LICENSE](https://github.com/codeprysm/codeprysm/blob/main/LICENSE)
