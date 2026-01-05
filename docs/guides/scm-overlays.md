# SCM Overlays: Adding Scope Metadata

This guide explains how to create Tree-sitter SCM overlay files that add scope metadata (like `test`, `fixture`) to code graph nodes.

## Overview

Overlays extend the base language `.scm` files (e.g., `python-tags.scm`) by adding semantic scope markers. They use the same Tree-sitter query syntax but with special capture name conventions.

**Key concept**: Overlays don't extract new entities—they add metadata to entities already captured by base patterns.

## Directory Structure

```
crates/codeprysm-core/queries/
├── python-tags.scm          # Base: extracts functions, classes, etc.
├── javascript-tags.scm      # Base: extracts functions, classes, etc.
└── overlays/
    ├── python-test.scm      # Overlay: marks test functions/classes
    ├── javascript-test.scm  # Overlay: marks test functions/classes
    └── csharp-test.scm      # Overlay: marks [Test] methods
```

## Capture Name Convention

The scope metadata is encoded in the capture name using a `.scope.` marker:

```
@name.definition.<type>.<subtype>.scope.<scope_value>
```

**Examples:**
- `@name.definition.callable.function.scope.test` → `scope: "test"`
- `@name.definition.container.type.class.scope.test` → `scope: "test"`
- `@name.definition.callable.method.scope.fixture` → `scope: "fixture"`

## Creating a Test Detection Overlay

### Step 1: Identify the Base Pattern

Check the base `.scm` file to understand which AST nodes are captured:

```scheme
; From python-tags.scm
(function_definition
  name: (identifier) @name.definition.callable.function) @definition.callable.function
```

### Step 2: Create the Overlay Pattern

Match the same AST structure but with a scope suffix:

```scheme
; python-test.scm
(function_definition
  name: (identifier) @name.definition.callable.function.scope.test
  (#match? @name.definition.callable.function.scope.test "^test_")) @definition.callable.function.scope.test
```

**Key points:**
- Use the **same type hierarchy** (`callable.function`) as the base
- Add `.scope.test` to both name and definition captures
- Use `#match?` or `#eq?` predicates to filter

### Step 3: Handle Decorated/Attributed Code

Many test frameworks use decorators or attributes:

**Python (decorators):**
```scheme
(decorated_definition
  (decorator)* @decorator
  definition: (function_definition
    name: (identifier) @name.definition.callable.function.scope.test
    (#match? @name.definition.callable.function.scope.test "^test_"))) @definition.callable.function.scope.test
```

**C# (attributes):**
```scheme
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.test
  (#eq? @_attr "Test")) @definition.callable.method.scope.test
```

## Supported Scope Values

| Scope | Purpose |
|-------|---------|
| `test` | Test functions/methods |
| `fixture` | Setup/teardown, pytest fixtures |
| `benchmark` | Performance tests |
| `example` | Go examples, documentation tests |

## Common Patterns by Language

### Python (pytest/unittest)
```scheme
; test_* functions
(function_definition
  name: (identifier) @name.definition.callable.function.scope.test
  (#match? @name.definition.callable.function.scope.test "^test_"))

; Test* classes
(class_definition
  name: (identifier) @name.definition.container.type.class.scope.test
  (#match? @name.definition.container.type.class.scope.test "^Test"))

; @pytest.fixture
(decorated_definition
  (decorator (call function: (attribute object: (identifier) @_p attribute: (identifier) @_f)))
  definition: (function_definition name: (identifier) @name.definition.callable.function.scope.fixture)
  (#eq? @_p "pytest") (#eq? @_f "fixture"))
```

### JavaScript/TypeScript (Jest/Vitest)
```scheme
; describe/test/it blocks
(call_expression
  function: (identifier) @_fn
  arguments: (arguments (string) @name.definition.callable.function.scope.test)
  (#match? @_fn "^(describe|test|it)$"))
```

### Go (testing package)
```scheme
; Test* functions
(function_declaration
  name: (identifier) @name.definition.callable.function.scope.test
  (#match? @name.definition.callable.function.scope.test "^Test"))

; Benchmark* functions
(function_declaration
  name: (identifier) @name.definition.callable.function.scope.benchmark
  (#match? @name.definition.callable.function.scope.benchmark "^Benchmark"))
```

### C# (NUnit/xUnit/MSTest)
```scheme
; [Test], [Fact], [TestMethod]
(method_declaration
  (attribute_list (attribute name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.test
  (#match? @_attr "^(Test|Fact|Theory|TestMethod)$"))
```

### Rust (#[test])
```scheme
(function_item
  (attribute_item (attribute (identifier) @_attr))
  name: (identifier) @name.definition.callable.function.scope.test
  (#eq? @_attr "test"))
```

## Testing Your Overlay

Run the overlay integration tests:

```bash
cargo test --package codeprysm-core overlay
```

Or build a graph and inspect scope metadata using the CLI:

```bash
codeprysm init /path/to/project
codeprysm status --json | jq '.nodes[] | select(.metadata.scope != null)'
```

## Troubleshooting

### Overlay patterns not matching

1. **Check AST structure**: Use `tree-sitter parse` or the Tree-sitter playground to see the actual AST
2. **Verify predicates**: `#match?` uses regex, `#eq?` is exact match
3. **Check capture names**: Must include both `@name.definition.*` and `@definition.*`

### Scope not appearing in graph

1. **Base pattern must match first**: Overlays add metadata to existing nodes
2. **Type hierarchy must match**: `callable.function` overlay won't match `callable.method` base

### Multiple scopes

Currently only one scope per entity is supported. The last matching overlay wins.

## File Naming Convention

```
{language}-{purpose}.scm
```

Examples:
- `python-test.scm` - Python test detection
- `go-test.scm` - Go test detection
- `csharp-test.scm` - C# test detection

## Adding Support for New Languages

1. Ensure base `{language}-tags.scm` exists in `crates/codeprysm-core/queries/`
2. Create `crates/codeprysm-core/queries/overlays/{language}-test.scm`
3. Add integration tests to `crates/codeprysm-core/tests/`
4. Run `cargo test --package codeprysm-core` to verify
