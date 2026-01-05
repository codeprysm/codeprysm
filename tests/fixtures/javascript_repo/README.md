# JavaScript/TypeScript Test Repository

This synthetic JavaScript/TypeScript repository is designed to test Tree-Sitter AST parsing and entity extraction for JavaScript and TypeScript code.

## Files

- `main.js`: JavaScript ES6+ features (functions, classes, async, generators)
- `types.ts`: TypeScript type system (interfaces, generics, enums)
- `components.tsx`: React components with JSX/TSX and hooks

## Expected AST Entities

### Functions (JavaScript)

From `main.js`:
- `greetPerson`
- `doubleNumber` (arrow function)
- `processArray` (arrow function)
- `asyncDelay` (async function)
- `fetchData` (async arrow function)
- `numberGenerator` (generator function)
- `applyFunction`
- `createMultiplier`
- `processOptions`
- `sumAll`
- `formatName`

### Classes (JavaScript)

From `main.js`:
- `Person`
- `Employee` (extends Person)

### Functions (TypeScript)

From `types.ts`:
- `createUser`
- `findById` (generic)
- `processValue`
- `fetchUser` (async)
- `isUser` (type guard)

From `components.tsx`:
- `Button` (React FC)
- `Counter` (React FC)
- `UserList` (React FC)
- `DataFetcher` (React FC)
- `useLocalStorage` (custom hook)

### Classes (TypeScript)

From `types.ts`:
- `UserManager`
- `DataStore` (generic)
- `Task`

### Interfaces (TypeScript)

From `types.ts`:
- `User`
- `Repository` (generic)

From `components.tsx`:
- `ButtonProps`
- `CounterProps`
- `User` (duplicate for local use)
- `UserListProps`

### Enums (TypeScript)

From `types.ts`:
- `Status`

### Methods

From `Person` class (main.js):
- `constructor`
- `greet`
- `addYears`
- `createDefault` (static)
- `info` (getter)
- `newAge` (setter)

From `Employee` class (main.js):
- `constructor`
- `greet` (override)
- `getJobTitle`

From `UserManager` class (types.ts):
- `constructor`
- `addUser`
- `getUser`
- `getAllUsers`
- `validateUser` (private)

From `DataStore` class (types.ts):
- `set`
- `get`
- `has`
- `delete`
- `getAll`

From `Task` class (types.ts):
- `constructor`
- `activate`
- `complete`

## JavaScript/TypeScript Features Demonstrated

- ✅ ES6+ arrow functions
- ✅ Async/await
- ✅ Generator functions
- ✅ Classes with inheritance
- ✅ Static methods
- ✅ Getters and setters
- ✅ Default parameters
- ✅ Destructuring
- ✅ Spread operator
- ✅ Template literals
- ✅ TypeScript interfaces
- ✅ TypeScript generics
- ✅ TypeScript enums
- ✅ TypeScript type guards
- ✅ React functional components
- ✅ React hooks (useState, useEffect, useCallback)
- ✅ Custom hooks
- ✅ JSX/TSX syntax

## Entity Count Summary

- **Functions**: ~16 functions
- **Classes**: ~5 classes
- **Interfaces**: ~6 interfaces
- **Enums**: ~1 enum
- **Methods**: ~18 methods
- **Total Entities**: ~46

## Usage in Tests

```python
@pytest.mark.parametrize("test_repo", ["javascript_repo"], indirect=True)
def test_javascript_parsing(test_repo):
    generator = CodeGraphGenerator(repo_path=str(test_repo))
    graph = generator.generate_graph()
    
    # Verify JavaScript entities
    assert_entity_exists(graph, "greetPerson", "FUNCTION")
    assert_entity_exists(graph, "Person", "CLASS")
    
    # Verify TypeScript entities
    assert_entity_exists(graph, "User", "INTERFACE")
    assert_entity_exists(graph, "Status", "ENUM")
```

## Notes

- React and type definition imports will show errors without dependencies installed
- This is expected for test fixtures - the AST parser doesn't need actual dependencies
- The fixtures are designed to test parsing, not runtime execution
