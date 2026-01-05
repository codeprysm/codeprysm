# Test Fixtures

This directory contains synthetic test repositories used for integration testing. Each test repository is a minimal, self-contained codebase designed to validate specific aspects of the code-search functionality.

## Directory Structure

```
fixtures/
├── README.md                 # This file
├── python_repo/              # Python test repository
├── javascript_repo/          # JavaScript/TypeScript test repository
├── c_cpp_repo/              # C/C++ test repository
├── csharp_repo/             # C# test repository
├── go_repo/                 # Go test repository
├── ruby_repo/               # Ruby test repository
├── java_repo/               # Java test repository
└── multi_lang_repo/         # Mixed-language repository
```

## Test Repository Guidelines

### Size Requirements

- **Maximum total size**: 10MB for all fixtures combined
- **Maximum file count per repo**: 100 files
- **Recommended file count per repo**: 10-20 files

### Content Requirements

Each test repository should include:

1. **Diverse entity types**: Functions, classes, methods, interfaces, etc.
2. **Realistic structure**: Mimic real-world code organization patterns
3. **Edge cases**: Include constructs that might challenge the parser
4. **Documentation**: Comments explaining the purpose of each test file

### Language-Specific Requirements

#### Python Repository (`python_repo/`)

Should include files demonstrating:
- Functions (standalone and nested)
- Classes with methods
- Async functions and coroutines
- Decorators
- Type hints (PEP 484)
- Docstrings (Google/NumPy style)
- Module-level imports

Expected entities: Functions, classes, methods, decorators

#### JavaScript/TypeScript Repository (`javascript_repo/`)

Should include files demonstrating:
- ES6+ functions (arrow functions, async/await)
- Classes with constructor and methods
- React/JSX components
- TypeScript interfaces and type definitions
- Generics
- Import/export statements

Expected entities: Functions, classes, interfaces, components

#### C/C++ Repository (`c_cpp_repo/`)

Should include files demonstrating:
- Functions (with prototypes)
- Structs and classes
- Templates
- Macros and preprocessor directives
- Namespaces
- Header files (.h) and implementation files (.cpp)

Expected entities: Functions, classes, structs, templates, macros

#### C# Repository (`csharp_repo/`)

Should include files demonstrating:
- Classes with properties and methods
- Interfaces
- LINQ expressions
- Async/await patterns
- Generics
- Namespaces

Expected entities: Classes, interfaces, methods, properties

#### Go Repository (`go_repo/`)

Should include files demonstrating:
- Functions
- Interfaces
- Structs with methods
- Goroutines
- Packages

Expected entities: Functions, interfaces, structs, methods

#### Ruby Repository (`ruby_repo/`)

Should include files demonstrating:
- Classes and modules
- Methods (instance and class methods)
- Blocks and procs
- Mixins

Expected entities: Classes, modules, methods

#### Java Repository (`java_repo/`)

Should include files demonstrating:
- Classes and interfaces
- Generics
- Annotations
- Packages

Expected entities: Classes, interfaces, methods, annotations

#### Multi-Language Repository (`multi_lang_repo/`)

A mixed repository combining samples from all languages to test:
- Language detection from file extensions
- Cross-language repository processing
- Mixed language indexing and search

Expected entities: All types from supported languages

## Usage in Tests

### Using Test Repositories

Test repositories are accessed via the `test_repo` fixture:

```python
import pytest

@pytest.mark.parametrize("test_repo", ["python_repo"], indirect=True)
def test_python_parsing(test_repo):
    """Test Python repository parsing."""
    # test_repo is a Path to the copied repository
    assert (test_repo / "main.py").exists()
```

### Fixture Parameters

- **fixture_name**: Name of the repository directory (e.g., "python_repo")
- The fixture automatically copies the repository to a temporary location
- Tests can modify files without affecting the original fixtures

### Example Usage

```python
def test_graph_generation(test_repo, tmp_test_dir):
    """Test code graph generation on test repository."""
    from src_legacy.core.code_graph_generator import CodeGraphGenerator
    
    # Generate graph from test repository
    generator = CodeGraphGenerator(repo_path=str(test_repo))
    graph_data = generator.generate_graph()
    
    # Validate graph structure
    assert len(graph_data["nodes"]) > 0
    assert len(graph_data["edges"]) > 0
```

## Adding New Test Repositories

To add a new test repository:

1. **Create directory**: `mkdir tests/fixtures/new_repo/`
2. **Add source files**: Create synthetic code files following language guidelines
3. **Document entities**: Add a `README.md` in the repo listing expected entities
4. **Verify size**: Ensure total size < 1MB (use `du -sh tests/fixtures/new_repo/`)
5. **Update this file**: Add documentation for the new repository
6. **Test the fixture**: Write a test using the new repository

Example:

```bash
# Create new test repository
mkdir -p tests/fixtures/new_repo/src
touch tests/fixtures/new_repo/src/main.ext
touch tests/fixtures/new_repo/README.md

# Verify size
du -sh tests/fixtures/new_repo/

# Update documentation
echo "## New Repository" >> tests/fixtures/README.md
```

## Maintenance Guidelines

### Updating Fixtures

- Keep fixtures minimal - only add code necessary for testing
- Update documentation when adding new files or entities
- Ensure backward compatibility - existing tests should continue passing
- Version control all fixture changes for test reproducibility

### Best Practices

1. **Deterministic content**: Avoid randomness in generated code
2. **Self-contained**: Each repo should be independent
3. **Documented**: Include comments explaining test scenarios
4. **Realistic**: Mirror real-world code patterns where possible
5. **Minimal**: Only include what's necessary for validation

## Common Issues

### Fixture Not Found

```python
pytest.skip(f"Test fixture '{fixture_name}' not found")
```

**Solution**: Ensure the fixture directory exists in `tests/fixtures/`

### Fixture Too Large

**Solution**: Remove unnecessary files or split into multiple smaller fixtures

### Parser Errors

**Solution**: Validate syntax of test files using language-specific linters

## Verification

To verify all fixtures are valid:

```bash
# Check total size
du -sh tests/fixtures/

# List all repositories
ls -l tests/fixtures/

# Validate fixture structure
python -c "from tests.conftest import test_repo; print('Fixtures OK')"
```

## Expected Entity Counts (Approximate)

| Repository | Files | Functions | Classes | Interfaces | Total Entities |
|------------|-------|-----------|---------|------------|----------------|
| python_repo | 10-15 | 20-30 | 5-10 | 0 | 30-50 |
| javascript_repo | 10-15 | 15-25 | 5-10 | 3-5 | 25-45 |
| c_cpp_repo | 15-20 | 20-30 | 5-10 | 0 | 30-50 |
| csharp_repo | 10-15 | 20-30 | 5-10 | 3-5 | 30-50 |
| go_repo | 8-12 | 15-25 | 0 | 3-5 | 20-35 |
| ruby_repo | 8-12 | 15-25 | 5-8 | 0 | 25-40 |
| java_repo | 10-15 | 20-30 | 5-10 | 3-5 | 30-50 |
| multi_lang_repo | 20-30 | 50-80 | 15-25 | 5-10 | 80-130 |

These counts are guidelines - actual counts may vary based on testing needs.

## Test Data Integrity

All test fixtures are:
- **Version controlled**: Committed to git for reproducibility
- **Read-only during tests**: Copied to temporary locations before use
- **Validated**: Structure verified in CI/CD pipeline
- **Documented**: Expected entities documented for test assertions

## Future Enhancements

Potential future additions:
- Edge case repositories (empty files, large files, deeply nested structures)
- Language-specific feature repositories (Python decorators, TypeScript generics)
- Error case repositories (syntax errors, encoding issues)
- Performance test repositories (1000+ files for stress testing)
