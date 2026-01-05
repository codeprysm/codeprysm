# CodePrysm Docker

Docker image for running CodePrysm MCP server.

## Quick Start

### Build the Image

```bash
docker build -f docker/Dockerfile -t codeprysm:local .
```

Or use just:

```bash
just docker-build
```

### Run the MCP Server

```bash
# Mount your code repository
docker run -it --rm \
  -v /path/to/your/repo:/data \
  codeprysm:local mcp --root /data
```

### With Qdrant

For full semantic search functionality, run with Qdrant:

```bash
# Start Qdrant
docker run -d --name qdrant -p 6333:6333 -p 6334:6334 \
  -v qdrant_storage:/qdrant/storage \
  qdrant/qdrant:latest

# Run CodePrysm with Qdrant connection
docker run -it --rm \
  --network host \
  -v /path/to/your/repo:/data \
  codeprysm:local mcp --root /data --qdrant-url http://localhost:6334
```

## Available Commands

The container uses `codeprysm` as the entrypoint. You can run any CodePrysm command:

```bash
# Generate graph only
docker run -it --rm -v /path/to/repo:/data codeprysm:local init --root /data

# Get statistics
docker run -it --rm -v /path/to/repo:/data codeprysm:local stats --codeprysm-dir /data/.codeprysm

# Search
docker run -it --rm \
  --network host \
  -v /path/to/repo:/data \
  codeprysm:local search "function that handles authentication"
```

## Docker Compose

Example `docker-compose.yml`:

```yaml
version: '3.8'

services:
  qdrant:
    image: qdrant/qdrant:latest
    ports:
      - "6333:6333"
      - "6334:6334"
    volumes:
      - qdrant_storage:/qdrant/storage

  codeprysm:
    build:
      context: ..
      dockerfile: docker/Dockerfile
    depends_on:
      - qdrant
    volumes:
      - /path/to/your/repo:/data
    command: ["mcp", "--root", "/data", "--qdrant-url", "http://qdrant:6334"]

volumes:
  qdrant_storage:
```

## Building for Different Platforms

```bash
# Build for linux/amd64
docker build --platform linux/amd64 -f docker/Dockerfile -t codeprysm:amd64 .

# Build for linux/arm64
docker build --platform linux/arm64 -f docker/Dockerfile -t codeprysm:arm64 .
```

## Environment Variables

- `RUST_LOG`: Set logging level (e.g., `info`, `debug`, `trace`)

## Notes

- The image runs as a non-root user (`codeprysm`) for security
- Data is stored in `/data` inside the container
- Mount your repository to `/data` for analysis
