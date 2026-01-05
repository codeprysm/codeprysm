# Python Test Repository

This synthetic Python repository is designed to test Tree-Sitter AST parsing and entity extraction for Python code.

## Files

- `main.py`: Main module with various Python features
- `utils.py`: Utility functions and classes
- `models.py`: Data models and business logic

## Expected AST Entities

### Functions (Standalone)

From `main.py`:
- `simple_function`
- `function_with_multiple_params`
- `async_function`
- `async_generator`
- `decorator_example`
- `decorated_function`

From `utils.py`:
- `outer_function`
- `inner_function` (nested)
- `higher_order_function`
- `list_comprehension_example`
- `lambda_example`
- `file_manager`
- `exception_handling_example`

### Classes

From `main.py`:
- `Person` (dataclass)
- `Calculator`
- `GenericContainer`

From `utils.py`:
- `BaseClass`
- `DerivedClass`

From `models.py`:
- `Status` (Enum)
- `Task` (dataclass)
- `Project` (dataclass)

### Methods

From `Person` class:
- `greet`
- `update_email`

From `Calculator` class:
- `__init__`
- `add`
- `subtract`
- `reset`
- `current_value` (property)
- `square` (staticmethod)
- `from_string` (classmethod)

From `GenericContainer` class:
- `__init__`
- `add_item`
- `get_items`
- `filter_by_type`

From `BaseClass`:
- `__init__`
- `base_method`
- `overrideable_method`

From `DerivedClass`:
- `__init__`
- `overrideable_method` (override)
- `derived_method`

From `Task` dataclass:
- `mark_complete`
- `assign_to`

From `Project` dataclass:
- `add_task`
- `get_completed_tasks`
- `get_pending_tasks`

## Python Features Demonstrated

- ✅ Type hints (PEP 484)
- ✅ Async/await coroutines
- ✅ Async generators
- ✅ Decorators (custom and dataclass)
- ✅ Context managers
- ✅ Properties
- ✅ Static methods
- ✅ Class methods
- ✅ Inheritance
- ✅ Enumerations
- ✅ Lambda expressions
- ✅ List comprehensions
- ✅ Nested functions (closures)
- ✅ Exception handling
- ✅ Docstrings

## Entity Count Summary

- **Functions**: ~13 standalone functions
- **Classes**: ~7 classes (including Enum and dataclasses)
- **Methods**: ~25 methods
- **Total Entities**: ~45

## Usage in Tests

```python
@pytest.mark.parametrize("test_repo", ["python_repo"], indirect=True)
def test_python_parsing(test_repo):
    generator = CodeGraphGenerator(repo_path=str(test_repo))
    graph = generator.generate_graph()
    
    # Verify expected entities
    assert_entity_exists(graph, "simple_function", "FUNCTION")
    assert_entity_exists(graph, "Calculator", "CLASS")
    assert_entity_exists(graph, "add", "FUNCTION")  # Method
```
