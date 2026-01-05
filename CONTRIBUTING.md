# Contributing to CodePrysm

Thank you for your interest in contributing to CodePrysm! This document provides guidelines and information for contributors.

## Getting Started

### Prerequisites

- Rust 1.85 or later
- Docker (for Qdrant vector database)
- [just](https://github.com/casey/just) command runner

### Development Setup

1. Clone the repository:
   ```bash
   git clone https://github.com/codeprysm/codeprysm.git
   cd codeprysm
   ```

2. Build the project:
   ```bash
   just setup
   ```

3. Run tests:
   ```bash
   just rust-test
   ```

## How to Contribute

### Reporting Bugs

If you find a bug, please open an issue on GitHub with:

- A clear, descriptive title
- Steps to reproduce the issue
- Expected behavior vs actual behavior
- Your environment (OS, Rust version, etc.)
- Any relevant logs or error messages

### Suggesting Features

Feature requests are welcome! Please open an issue with:

- A clear description of the feature
- The problem it solves or use case
- Any implementation ideas you have

### Pull Requests

1. Fork the repository
2. Create a feature branch (`git checkout -b feature/amazing-feature`)
3. Make your changes
4. Run tests and linting:
   ```bash
   just rust-test
   just rust-lint
   just rust-fmt-check
   ```
5. Commit your changes with a descriptive message
6. Push to your fork
7. Open a Pull Request

#### PR Requirements

- All tests must pass
- Code must pass `cargo clippy` without warnings
- Code must be formatted with `cargo fmt`
- Include tests for new functionality
- Update documentation as needed

### Code Style

- Follow Rust idioms and best practices
- Use meaningful variable and function names
- Add comments for complex logic
- Keep functions focused and reasonably sized

### Commit Messages

- Use clear, descriptive commit messages
- Start with a verb (Add, Fix, Update, Remove, etc.)
- Reference issues when applicable (`Fixes #123`)

## Project Structure

```
codeprysm/
├── crates/
│   ├── codeprysm-core/     # Graph generation, tree-sitter parsing
│   ├── codeprysm-search/   # Vector search, embeddings
│   ├── codeprysm-mcp/      # MCP server
│   ├── codeprysm-cli/      # Command-line interface
│   ├── codeprysm-config/   # Configuration management
│   └── codeprysm-backend/  # Backend abstraction
├── tests/fixtures/         # Test repositories
├── docs/                   # Documentation
└── docker/                 # Docker configuration
```

## Testing

- Unit tests are in each crate's `src/` directory
- Integration tests are in each crate's `tests/` directory
- Test fixtures are in `tests/fixtures/`

Run specific test suites:
```bash
just rust-test                    # All tests
just rust-test-integration        # Integration tests
just rust-test-integration-lang python  # Language-specific tests
```

## Documentation

- Update README.md for user-facing changes
- Update CLAUDE.md for developer guidance changes
- Add/update docs in `docs/` for feature documentation

## Questions?

Feel free to open an issue for questions or join discussions on GitHub.

## License

By contributing, you agree that your contributions will be licensed under the MIT License.
