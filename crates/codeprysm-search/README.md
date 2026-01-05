# codeprysm-search

[![Crates.io](https://img.shields.io/crates/v/codeprysm-search.svg)](https://crates.io/crates/codeprysm-search)
[![Documentation](https://docs.rs/codeprysm-search/badge.svg)](https://docs.rs/codeprysm-search)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

Semantic code search with vector embeddings and Qdrant.

Part of the [CodePrism](https://github.com/codeprysm/codeprysm) project.

## Features

- **Semantic Search**: Natural language queries using vector embeddings
- **Hybrid Search**: Combines semantic and keyword matching with score fusion
- **GPU Acceleration**: Metal (macOS) and CUDA (Linux/Windows) support
- **Qdrant Integration**: Scalable vector database for production use
- **Code-Optimized Embeddings**: Uses Jina embeddings tuned for code

## Installation

```toml
[dependencies]
codeprysm-search = "0.1"

# With GPU acceleration
codeprysm-search = { version = "0.1", features = ["metal"] }  # macOS
codeprysm-search = { version = "0.1", features = ["cuda"] }   # Linux/Windows
```

## Usage

```rust
use codeprysm_search::{QdrantClient, SearchQuery};

// Connect to Qdrant
let client = QdrantClient::new("http://localhost:6334").await?;

// Search for code
let results = client.search(SearchQuery {
    query: "authentication handler".to_string(),
    limit: 10,
    ..Default::default()
}).await?;

for result in results {
    println!("{}: {} (score: {:.2})", result.file_path, result.name, result.score);
}
```

## GPU Acceleration

GPU provides 7-9x faster inference for embedding generation:

| Device | Per-iteration | Notes |
|--------|---------------|-------|
| CPU | 170-180ms | Default |
| Metal GPU | 20-25ms | macOS Apple Silicon |
| CUDA GPU | 15-20ms | NVIDIA GPUs |

## License

MIT License - see [LICENSE](https://github.com/codeprysm/codeprysm/blob/main/LICENSE)
