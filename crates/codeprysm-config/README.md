# codeprysm-config

[![Crates.io](https://img.shields.io/crates/v/codeprysm-config.svg)](https://crates.io/crates/codeprysm-config)
[![Documentation](https://docs.rs/codeprysm-config/badge.svg)](https://docs.rs/codeprysm-config)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Configuration loading for CodePrism.

Part of the [CodePrism](https://github.com/codeprysm/codeprysm) project.

## Features

- **TOML Configuration**: Human-readable configuration files
- **Hierarchical Loading**: Project-local and global config support
- **Environment Override**: Environment variables can override config values
- **Sensible Defaults**: Works out of the box with no configuration

## Installation

```toml
[dependencies]
codeprysm-config = "0.1"
```

## Usage

```rust
use codeprysm_config::Config;
use std::path::Path;

// Load configuration for a repository
let config = Config::load(Path::new("/path/to/repo"))?;

println!("Qdrant URL: {}", config.qdrant.url);
println!("Exclude patterns: {:?}", config.indexing.exclude);
```

## Configuration Files

CodePrism looks for configuration in this order:

1. `.codeprysm/config.toml` - Repository-local configuration
2. `~/.config/codeprysm/config.toml` - Global user configuration

### Example Configuration

```toml
[qdrant]
url = "http://localhost:6334"
collection = "codeprysm"

[indexing]
exclude = [
    "**/node_modules/**",
    "**/vendor/**",
    "**/target/**",
    "**/.git/**"
]

[embedding]
batch_size = 32
model = "jinaai/jina-embeddings-v2-base-code"
```

## License

MIT License - see [LICENSE](https://github.com/codeprysm/codeprysm/blob/main/LICENSE)
