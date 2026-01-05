# codeprysm-backend

[![Crates.io](https://img.shields.io/crates/v/codeprysm-backend.svg)](https://crates.io/crates/codeprysm-backend)
[![Documentation](https://docs.rs/codeprysm-backend/badge.svg)](https://docs.rs/codeprysm-backend)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Backend abstraction layer for CodePrism.

Part of the [CodePrism](https://github.com/codeprysm/codeprysm) project.

## Features

- **Unified API**: Single interface for graph and search operations
- **Async Support**: Full async/await support with Tokio
- **Lazy Loading**: On-demand loading of graph partitions
- **Connection Pooling**: Efficient Qdrant connection management

## Installation

```toml
[dependencies]
codeprysm-backend = "0.1"

# With GPU acceleration
codeprysm-backend = { version = "0.1", features = ["metal"] }  # macOS
codeprysm-backend = { version = "0.1", features = ["cuda"] }   # Linux/Windows
```

## Usage

```rust
use codeprysm_backend::Backend;
use std::path::Path;

// Create a backend instance
let backend = Backend::new(
    Path::new("/path/to/repo"),
    "http://localhost:6334",
).await?;

// Search for code
let results = backend.search("authentication handler", 10).await?;

// Get node details
let node = backend.get_node(&node_id).await?;

// Find relationships
let relationships = backend.get_relationships(&node_id).await?;
```

## Architecture

The backend coordinates between:

- **codeprysm-core**: Graph storage and traversal
- **codeprysm-search**: Vector search and embeddings
- **codeprysm-config**: Configuration management

```
┌─────────────────────────────────────────┐
│           codeprysm-backend             │
├─────────────┬─────────────┬─────────────┤
│ codeprysm-  │ codeprysm-  │ codeprysm-  │
│    core     │   search    │   config    │
└─────────────┴─────────────┴─────────────┘
```

## License

MIT License - see [LICENSE](https://github.com/codeprysm/codeprysm/blob/main/LICENSE)
