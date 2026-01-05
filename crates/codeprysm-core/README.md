# codeprysm-core

[![Crates.io](https://img.shields.io/crates/v/codeprysm-core.svg)](https://crates.io/crates/codeprysm-core)
[![Documentation](https://docs.rs/codeprysm-core/badge.svg)](https://docs.rs/codeprysm-core)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Code graph generation using Tree-sitter AST parsing.

Part of the [CodePrism](https://github.com/codeprysm/codeprysm) project.

## Features

- **AST-Based Parsing**: Uses Tree-sitter for precise, language-agnostic parsing
- **Rich Code Graph**: Builds a graph with Container, Callable, and Data nodes
- **Relationship Types**: CONTAINS (hierarchy), USES (dependencies), DEFINES (definitions)
- **Incremental Updates**: Merkle tree-based change detection for fast updates
- **Multi-Language Support**: Python, JavaScript/TypeScript, C/C++, C#, Go, Rust

## Installation

```toml
[dependencies]
codeprysm-core = "0.1"
```

## Usage

```rust
use codeprysm_core::{GraphBuilder, GraphBuilderConfig};
use std::path::Path;

// Build a code graph from a repository
let config = GraphBuilderConfig::default();
let builder = GraphBuilder::new(config);
let graph = builder.build(Path::new("/path/to/repo"))?;

// Access nodes and edges
for node in graph.nodes() {
    println!("{}: {}", node.kind, node.name);
}
```

## Node Types

| Type | Description | Examples |
|------|-------------|----------|
| Container | Structural entities | Repository, File, Class, Module |
| Callable | Executable entities | Function, Method, Constructor |
| Data | Variables and fields | Field, Constant, Parameter |

## License

MIT License - see [LICENSE](https://github.com/codeprysm/codeprysm/blob/main/LICENSE)
