# Getting Started (Docker)

This guide covers running CodePrism using Docker containers. For native installation, see the [Getting Started Guide](./getting-started.md).

## Table of Contents

1. [Prerequisites](#1-prerequisites)
2. [Start Qdrant](#2-start-qdrant)
3. [Build or Pull CodePrism Image](#3-build-or-pull-codeprysm-image)
4. [Run Initial Indexing](#4-run-initial-indexing)
5. [Start MCP Server](#5-start-mcp-server)
6. [VS Code Integration](#6-vs-code-integration)

## 1. Prerequisites

- Docker installed and running
- Your code repository accessible from the host machine

## 2. Start Qdrant

CodePrism requires Qdrant for semantic search:

```bash
docker run -d --name qdrant \
  -p 6333:6333 -p 6334:6334 \
  -v qdrant_storage:/qdrant/storage \
  qdrant/qdrant:latest
```

## 3. Build or Pull CodePrism Image

### Build from Source

```bash
git clone https://github.com/codeprysm/codeprysm.git
cd codeprysm
docker build -f docker/Dockerfile -t codeprysm:local .
```

Or using just:

```bash
just docker-build
```

## 4. Run Initial Indexing

Add `.codeprysm/` to your repository's `.gitignore`:

```bash
echo ".codeprysm/" >> /path/to/your/repo/.gitignore
```

Run the initial indexing:

```bash
docker run -it --rm \
  --network host \
  -v /path/to/your/repo:/data \
  codeprysm:local init --root /data --qdrant-url http://localhost:6334
```

This creates a `.codeprysm/` directory in your repository containing the code graph and semantic index.

### Exclude Unwanted Files

Use glob patterns to exclude specific files or directories:

```bash
docker run -it --rm \
  --network host \
  -v /path/to/your/repo:/data \
  codeprysm:local init --root /data --exclude "**/node_modules/**" "**/vendor/**"
```

Files matching `.gitignore` patterns are automatically excluded.

## 5. Start MCP Server

### Interactive Mode

```bash
docker run -it --rm \
  --network host \
  -v /path/to/your/repo:/data \
  codeprysm:local mcp --root /data --qdrant-url http://localhost:6334
```

### Background Mode

```bash
docker run -d --name codeprysm-mcp \
  --network host \
  -v /path/to/your/repo:/data \
  codeprysm:local mcp --root /data --qdrant-url http://localhost:6334
```

## 6. VS Code Integration

Create `.vscode/mcp.json` in your repository:

```json
{
    "servers": {
        "codeprysm": {
            "type": "stdio",
            "command": "docker",
            "args": [
                "run", "--rm", "-i",
                "--network", "host",
                "-v", "${workspaceFolder}:/data",
                "codeprysm:local",
                "mcp", "--root", "/data", "--qdrant-url", "http://localhost:6334"
            ]
        }
    }
}
```

**Note for Dev Containers**: The `${workspaceFolder}` variable may not work correctly in devcontainers. Use the absolute host path to your repository instead.

## Docker Compose

For a complete setup with Qdrant, create `docker-compose.yml`:

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
      context: .
      dockerfile: docker/Dockerfile
    depends_on:
      - qdrant
    volumes:
      - /path/to/your/repo:/data
    command: ["mcp", "--root", "/data", "--qdrant-url", "http://qdrant:6334"]
    stdin_open: true
    tty: true

volumes:
  qdrant_storage:
```

Run with:

```bash
docker compose up -d
```

## Verification

### Check Container Status

```bash
docker ps | grep -E "(qdrant|codeprysm)"
```

### View Statistics

```bash
docker run --rm \
  -v /path/to/your/repo:/data \
  codeprysm:local stats --prism-dir /data/.codeprysm
```

### Test Search

```bash
docker run --rm \
  --network host \
  -v /path/to/your/repo:/data \
  codeprysm:local search "authentication handler" --qdrant-url http://localhost:6334
```

## Troubleshooting

### Network Issues

If the container can't connect to Qdrant, ensure you're using `--network host` or that both containers are on the same Docker network.

### Permission Issues

The CodePrism container runs as a non-root user. Ensure the mounted volume has appropriate permissions:

```bash
chmod -R 755 /path/to/your/repo
```

### Volume Mount Issues

On macOS and Windows, ensure Docker has access to the directory you're mounting. Check Docker Desktop settings under Resources > File Sharing.
