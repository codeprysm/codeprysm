# Multi-Language Test Repository

This repository combines samples from multiple programming languages to test:
- Multi-language repository processing
- Language detection from file extensions
- Cross-language indexing and search

## Files

- `module.py`: Python sample
- `module.js`: JavaScript sample
- `module.ts`: TypeScript sample
- `module.cpp`: C++ sample
- `module.cs`: C# sample

## Expected Entities by Language

### Python (module.py)
- `python_function` (FUNCTION)
- `PythonClass` (CLASS)
- `__init__` (METHOD)
- `get_value` (METHOD)

### JavaScript (module.js)
- `jsFunction` (FUNCTION)
- `JSClass` (CLASS)
- `constructor` (METHOD)
- `getValue` (METHOD)
- `arrowFunc` (FUNCTION)

### TypeScript (module.ts)
- `Data` (INTERFACE)
- `tsFunction` (FUNCTION)
- `TSClass` (CLASS)
- `constructor` (METHOD)
- `getValue` (METHOD)

### C++ (module.cpp)
- `cppFunction` (FUNCTION)
- `CPPClass` (CLASS)
- `CPPClass` (METHOD - constructor)
- `getValue` (METHOD)

### C# (module.cs)
- `CSharpClass` (CLASS)
- `CSharpClass` (METHOD - constructor)
- `GetValue` (METHOD)
- `CSharpHelper` (CLASS)
- `CSharpFunction` (METHOD)

## Total Expected Entities: ~24

## Usage

```python
@pytest.mark.parametrize("test_repo", ["multi_lang_repo"], indirect=True)
def test_multi_language_parsing(test_repo):
    generator = CodeGraphGenerator(repo_path=str(test_repo))
    graph = generator.generate_graph()
    
    # Verify Python entities
    assert_entity_exists(graph, "python_function", "FUNCTION", "module.py")
    
    # Verify JavaScript entities
    assert_entity_exists(graph, "jsFunction", "FUNCTION", "module.js")
    
    # Verify TypeScript entities
    assert_entity_exists(graph, "Data", "INTERFACE", "module.ts")
    
    # Verify C++ entities
    assert_entity_exists(graph, "cppFunction", "FUNCTION", "module.cpp")
    
    # Verify C# entities
    assert_entity_exists(graph, "CSharpClass", "CLASS", "module.cs")
```
