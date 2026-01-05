# CodePrysm Docker Image
# Multi-stage build for minimal runtime image

# Build stage
FROM rust:1.85-slim AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

# Copy workspace configuration
COPY Cargo.toml Cargo.lock ./

# Copy all crates
COPY crates/ ./crates/

# Build release binary
RUN cargo build --release --package codeprysm-cli

# Runtime stage
FROM debian:bookworm-slim

# Install runtime dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Create non-root user for security
RUN groupadd -r codeprysm && useradd -r -g codeprysm codeprysm

# Create data directory
RUN mkdir -p /data && chown codeprysm:codeprysm /data

# Copy binary from build stage
COPY --from=builder /app/target/release/codeprysm /usr/local/bin/codeprysm

# Set ownership
RUN chown codeprysm:codeprysm /usr/local/bin/codeprysm

# Switch to non-root user
USER codeprysm

# Set working directory
WORKDIR /data

# Expose MCP server port (if using HTTP transport)
EXPOSE 8000

# Default command starts MCP server
ENTRYPOINT ["codeprysm"]
CMD ["mcp", "--root", "/data"]
