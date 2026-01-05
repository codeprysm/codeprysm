# CodePrysm - Code Graph Generator and Semantic Search

# List available commands
default:
    @just --list

##############################################################################################
# Main Commands
##############################################################################################

# Install project dependencies and setup
setup:
    cargo build --workspace --release

# Initialize code search system (generate graph only, indexing happens in MCP server)
init repo_dir="." codeprysm_dir="./.codeprysm":
    #!/usr/bin/env bash
    echo "Initializing code graph for {{repo_dir}}..."
    echo "Output: {{codeprysm_dir}} (partitioned storage)"
    ./target/release/codeprysm-core generate --repo {{repo_dir}} --output {{codeprysm_dir}}

# Start MCP server for interactive code analysis
mcp root="." codeprysm_dir="" qdrant_url="http://localhost:6334":
    #!/usr/bin/env bash
    echo "Starting MCP server..."
    echo "Root: {{root}}, Qdrant: {{qdrant_url}}"
    codeprysm_dir_arg=""
    if [ -n "{{codeprysm_dir}}" ]; then
        codeprysm_dir_arg="--codeprysm-dir {{codeprysm_dir}}"
        echo "CodePrysm dir: {{codeprysm_dir}}"
    fi
    ./target/release/codeprysm mcp --root {{root}} $codeprysm_dir_arg --qdrant-url {{qdrant_url}}

# Update graph incrementally
update repo_dir="." codeprysm_dir="./.codeprysm":
    #!/usr/bin/env bash
    echo "Updating code graph for {{repo_dir}}..."
    ./target/release/codeprysm-core update --repo {{repo_dir}} --codeprysm-dir {{codeprysm_dir}}

##############################################################################################
# Qdrant (Vector Database)
##############################################################################################

# Start Qdrant in Docker (required for search)
qdrant-start:
    #!/usr/bin/env bash
    echo "Starting Qdrant..."
    docker run -d --name qdrant -p 6333:6333 -p 6334:6334 \
        -v $(pwd)/.codeprysm/qdrant:/qdrant/storage \
        qdrant/qdrant:latest
    echo "Qdrant started at http://localhost:6333 (REST) and http://localhost:6334 (gRPC)"

# Stop Qdrant
qdrant-stop:
    #!/usr/bin/env bash
    echo "Stopping Qdrant..."
    docker stop qdrant && docker rm qdrant
    echo "Qdrant stopped"

# Check Qdrant status
qdrant-status:
    #!/usr/bin/env bash
    docker ps --filter name=qdrant --format "table {{{{.Names}}}}\t{{{{.Status}}}}\t{{{{.Ports}}}}" || echo "Qdrant not running"

##############################################################################################
# Search Commands
##############################################################################################

# Index a code graph to Qdrant for semantic search
index codeprysm_dir="./.codeprysm" root="." repo_id="":
    #!/usr/bin/env bash
    echo "Indexing code graph to Qdrant..."
    repo_id_arg=""
    if [ -n "{{repo_id}}" ]; then
        repo_id_arg="--repo-id {{repo_id}}"
    fi
    ./target/release/codeprysm-search index --codeprysm-dir {{codeprysm_dir}} --root {{root}} $repo_id_arg

# Search the indexed codebase
search query repo_id="" limit="10":
    #!/usr/bin/env bash
    repo_id_arg=""
    if [ -n "{{repo_id}}" ]; then
        repo_id_arg="--repo-id {{repo_id}}"
    fi
    ./target/release/codeprysm-search search "{{query}}" $repo_id_arg --limit {{limit}}

# Show index status for a repository
index-status repo_id="" root=".":
    #!/usr/bin/env bash
    repo_id_arg=""
    if [ -n "{{repo_id}}" ]; then
        repo_id_arg="--repo-id {{repo_id}}"
    fi
    ./target/release/codeprysm-search status --root {{root}} $repo_id_arg

##############################################################################################
# Graph Commands
##############################################################################################

# Generate code graph only
generate-graph repo_dir output="./.codeprysm":
    #!/usr/bin/env bash
    echo "Generating code graph for {{repo_dir}}..."
    ./target/release/codeprysm-core generate --repo {{repo_dir}} --output {{output}}

# Show graph statistics and analysis
stats codeprysm_dir="./.codeprysm":
    #!/usr/bin/env bash
    echo "Analyzing graph statistics for {{codeprysm_dir}}..."
    ./target/release/codeprysm-core stats --codeprysm-dir {{codeprysm_dir}} --detailed

# Update repository incrementally (faster than full rebuild)
update-repo repo_dir codeprysm_dir="./.codeprysm":
    #!/usr/bin/env bash
    echo "Performing incremental update for {{repo_dir}}..."
    ./target/release/codeprysm-core update --repo {{repo_dir}} --codeprysm-dir {{codeprysm_dir}}

# Force full rebuild of repository
rebuild-repo repo_dir codeprysm_dir="./.codeprysm":
    #!/usr/bin/env bash
    echo "Performing full rebuild for {{repo_dir}}..."
    ./target/release/codeprysm-core update --repo {{repo_dir}} --codeprysm-dir {{codeprysm_dir}} --force

##############################################################################################
# Docker
##############################################################################################

# Build MCP Server Docker image for local testing
docker-build:
    docker build -f docker/Dockerfile -t codeprysm:local .

##############################################################################################
# Rust Development
##############################################################################################

# Check all Rust crates compile
rust-check:
    cargo check --workspace

# Build all Rust crates (debug)
rust-build:
    cargo build --workspace

# Build all Rust crates (release)
rust-build-release:
    cargo build --workspace --release

# Build with Metal/MPS GPU acceleration (macOS)
rust-build-metal:
    cargo build -p codeprysm-core --release
    cargo build -p codeprysm-cli --release --features metal

# Build with CUDA GPU acceleration (Linux/Windows)
rust-build-cuda:
    cargo build -p codeprysm-core --release
    cargo build -p codeprysm-cli --release --features cuda

# Build with both Metal and CUDA (for distribution)
rust-build-gpu:
    cargo build -p codeprysm-core --release
    cargo build -p codeprysm-cli --release --features metal,cuda

# Run all Rust tests
rust-test:
    cargo test --workspace

# Run Rust tests with output
rust-test-verbose:
    cargo test --workspace -- --nocapture

# Format Rust code
rust-fmt:
    cargo fmt --all

# Check Rust formatting
rust-fmt-check:
    cargo fmt --all -- --check

# Run Rust linter (clippy)
rust-lint:
    cargo clippy --workspace -- -D warnings

# Clean Rust build artifacts
rust-clean:
    cargo clean

# Run integration tests for all languages
rust-test-integration:
    cargo test --package codeprysm-core --test integration -- --nocapture

# Run integration tests for a specific language
rust-test-integration-lang lang:
    cargo test --package codeprysm-core --test integration {{lang}} -- --nocapture

# Run integration test summary
rust-test-integration-summary:
    cargo test --package codeprysm-core --test integration test_all_fixtures_summary -- --nocapture

# Run nightly integration tests (real-world repos, requires --ignored)
rust-test-integration-nightly:
    cargo test --package codeprysm-core --test integration -- --ignored --nocapture

# Run real-world repo tests (Tier 2 - clones GitHub repos)
rust-test-realworld:
    cargo test --package codeprysm-core --test realworld_tests -- --ignored --nocapture

# Run real-world test for a specific language
rust-test-realworld-lang lang:
    cargo test --package codeprysm-core --test realworld_tests {{lang}} -- --ignored --nocapture

# Run real-world test summary
rust-test-realworld-summary:
    cargo test --package codeprysm-core --test realworld_tests test_realworld_all_summary -- --ignored --nocapture
