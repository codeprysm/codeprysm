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

## Memory-Efficient Indexing

For large repositories (>10K nodes), use streaming mode to bound memory usage:

```rust
use codeprysm_search::GraphIndexer;
use codeprysm_core::lazy::manager::LazyGraphManager;

// Open graph with memory budget
let manager = LazyGraphManager::open_with_memory_budget(
    &prism_dir,
    Some(512 * 1024 * 1024), // 512MB
)?;

// Stream index partition-by-partition
let stats = indexer.index_graph_lazy(&manager).await?;
println!(
    "Indexed {} nodes across {} partitions",
    stats.total_indexed,
    stats.partitions_processed
);
```

### CLI Usage

```bash
# Auto-detect streaming mode for large repos
codeprysm init

# Force streaming mode
codeprysm init --streaming on

# With memory budget
codeprysm init --streaming on --max-index-memory 8GB
```

### Memory Comparison

| Mode | Memory Usage | Best For |
|------|--------------|----------|
| In-memory | O(total_nodes) | Small repos (<10K nodes) |
| Streaming | O(max_partition) | Large repos, constrained memory |

A 50K-node repo uses ~50GB in-memory but <500MB streaming.

## GPU Acceleration

GPU provides 7-9x faster inference for embedding generation:

| Device | Per-iteration | Notes |
|--------|---------------|-------|
| CPU | 170-180ms | Default |
| Metal GPU | 20-25ms | macOS Apple Silicon |
| CUDA GPU | 15-20ms | NVIDIA GPUs |

## License

MIT License - see [LICENSE](https://github.com/codeprysm/codeprysm/blob/main/LICENSE)
