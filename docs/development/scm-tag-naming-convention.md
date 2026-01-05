# Tree-Sitter SCM Tag Naming Convention

## Overview

This document defines the declarative tag naming convention used in tree-sitter query (`.scm`) files for the Code Graph Generator. This convention enables language-agnostic processing by encoding semantic information directly in tag names, eliminating the need for hardcoded language knowledge in Rust code.

## Core Principle

**All semantic information lives in the `.scm` files.** The Rust processing code is generic and language-agnostic - it simply parses tag strings to extract node type, kind, and subtype information.

## Tag Structure

### Definition Tags

```
@definition.{node_type}.{kind}[.{subtype}]
@name.definition.{node_type}.{kind}[.{subtype}]
```

- `definition` - Indicates this is a definition (not a reference)
- `node_type` - One of: `container`, `callable`, `data`
- `kind` - The semantic category (see tables below)
- `subtype` - (Optional) Language-specific refinement

### Reference Tags

```
@reference.{node_type}.{kind}[.{subtype}]
@name.reference.{node_type}.{kind}[.{subtype}]
```

- `reference` - Indicates this is a reference (usage of a definition)
- Rest follows same pattern as definitions

### File Tags

```
@definition.file
@name.definition.file
```

Special case for file-level nodes (no kind/subtype).

## Node Types

### Container Nodes

**Purpose**: Organizational structures that contain other entities.

| Kind | Subtype | Description | Languages | Example |
|------|---------|-------------|-----------|---------|
| `module` | - | Logical module/package | Python, Go, Rust | `@definition.container.module` |
| `namespace` | - | Namespace declaration | C++, C# | `@definition.container.namespace` |
| `package` | - | Package declaration | Java, Go | `@definition.container.package` |
| `type` | `class` | Class definition | Python, TypeScript, C++, C#, Java | `@definition.container.type.class` |
| `type` | `struct` | Struct definition | Go, Rust, C, C++ | `@definition.container.type.struct` |
| `type` | `interface` | Interface/protocol | Go, TypeScript, C#, Java, Rust (trait) | `@definition.container.type.interface` |
| `type` | `enum` | Enumeration | Rust, C++, C#, Java, TypeScript | `@definition.container.type.enum` |
| `type` | `union` | Union type | C, C++, Rust | `@definition.container.type.union` |
| `type` | `alias` | Type alias | TypeScript, Rust | `@definition.container.type.alias` |
| `type` | `trait` | Trait definition | Rust | `@definition.container.type.trait` |

### Callable Nodes

**Purpose**: Executable code blocks (functions, methods, constructors).

| Kind | Subtype | Description | Languages | Example |
|------|---------|-------------|-----------|---------|
| `function` | - | Regular function | All | `@definition.callable.function` |
| `function` | `async` | Async function | Python, JavaScript, TypeScript, C# | `@definition.callable.function.async` |
| `function` | `generator` | Generator function | Python, JavaScript | `@definition.callable.function.generator` |
| `method` | - | Class/object method | All OOP languages | `@definition.callable.method` |
| `method` | `async` | Async method | Python, JavaScript, TypeScript, C# | `@definition.callable.method.async` |
| `method` | `static` | Static method | Python, Java, C++, C# | `@definition.callable.method.static` |
| `constructor` | - | Constructor/initializer | Java, C++, C#, TypeScript | `@definition.callable.constructor` |

### Data Nodes

**Purpose**: Data storage and values (variables, fields, constants, parameters).

| Kind | Subtype | Description | Languages | Example |
|------|---------|-------------|-----------|---------|
| `constant` | - | Module/global constant | All | `@definition.data.constant` |
| `constant` | `enum` | Enum member | C, C++, C#, Java | `@definition.data.constant.enum` |
| `value` | - | Module-level variable | Python, JavaScript | `@definition.data.value` |
| `field` | - | Class instance field | All OOP languages | `@definition.data.field` |
| `field` | `static` | Class static field | Java, C++, C#, TypeScript | `@definition.data.field.static` |
| `field` | `const` | Constant field | Rust, C++ | `@definition.data.field.const` |
| `property` | - | Property with getter/setter | C#, TypeScript, Python | `@definition.data.property` |
| `parameter` | - | Function parameter | All | `@definition.data.parameter` |
| `local` | - | Local variable | All | `@definition.data.local` |

## Metadata Encoding

Some semantic information cannot be encoded in tag strings and must be extracted from AST attributes. The `NodeMetadata` struct supports these optional fields:

```rust
pub struct NodeMetadata {
    pub visibility: Option<String>,    // "public", "private", "protected", "internal"
    pub is_async: Option<bool>,        // true for async functions/methods
    pub is_static: Option<bool>,       // true for static methods/fields
    pub is_abstract: Option<bool>,     // true for abstract classes/methods
    pub is_virtual: Option<bool>,      // true for virtual methods (C++)
    pub decorators: Option<Vec<String>>, // Python decorators, C# attributes
    pub modifiers: Option<Vec<String>>,  // Language-specific modifiers
}
```

## Language-Specific Examples

### Python

```scheme
; Classes
(class_definition
  name: (identifier) @name.definition.container.type.class) @definition.container.type.class

; Functions (regular)
(function_definition
  name: (identifier) @name.definition.callable.function) @definition.callable.function

; Async functions
(function_definition
  (async)
  name: (identifier) @name.definition.callable.function.async) @definition.callable.function.async

; Class fields (assignments in class body)
(class_definition
  body: (block
    (expression_statement
      (assignment
        left: (identifier) @name.definition.data.field)))) @definition.data.field

; Instance attributes (self.x = ...)
(assignment
  left: (attribute
    object: (identifier) @_self
    attribute: (identifier) @name.definition.data.field)
  (#eq? @_self "self")) @definition.data.field

; Constants (module-level, all caps)
(assignment
  left: (identifier) @name.definition.data.constant
  (#match? @name.definition.data.constant "^[A-Z_]+$")) @definition.data.constant

; Parameters
(parameters
  (identifier) @name.definition.data.parameter) @definition.data.parameter
```

### Go

```scheme
; Structs
(type_declaration
  (type_spec
    name: (type_identifier) @name.definition.container.type.struct
    type: (struct_type))) @definition.container.type.struct

; Interfaces
(type_declaration
  (type_spec
    name: (type_identifier) @name.definition.container.type.interface
    type: (interface_type))) @definition.container.type.interface

; Functions
(function_declaration
  name: (identifier) @name.definition.callable.function) @definition.callable.function

; Methods
(method_declaration
  name: (field_identifier) @name.definition.callable.method) @definition.callable.method

; Struct fields
(field_declaration
  name: (field_identifier) @name.definition.data.field) @definition.data.field

; Package declaration
(package_clause
  (package_identifier) @name.definition.container.module) @definition.container.module
```

### TypeScript

```scheme
; Classes
(class_declaration
  name: (type_identifier) @name.definition.container.type.class) @definition.container.type.class

; Interfaces
(interface_declaration
  name: (type_identifier) @name.definition.container.type.interface) @definition.container.type.interface

; Type aliases
(type_alias_declaration
  name: (type_identifier) @name.definition.container.type.alias) @definition.container.type.alias

; Functions
(function_declaration
  name: (identifier) @name.definition.callable.function) @definition.callable.function

; Methods
(method_definition
  name: (property_identifier) @name.definition.callable.method) @definition.callable.method

; Class properties
(public_field_definition
  name: (property_identifier) @name.definition.data.field) @definition.data.field

; Private fields
(private_field_definition
  name: (property_identifier) @name.definition.data.field) @definition.data.field
```

### Rust

```scheme
; Structs
(struct_item
  name: (type_identifier) @name.definition.container.type.struct) @definition.container.type.struct

; Enums
(enum_item
  name: (type_identifier) @name.definition.container.type.enum) @definition.container.type.enum

; Traits
(trait_item
  name: (type_identifier) @name.definition.container.type.trait) @definition.container.type.trait

; Functions
(function_item
  name: (identifier) @name.definition.callable.function) @definition.callable.function

; Struct fields
(field_declaration
  name: (field_identifier) @name.definition.data.field) @definition.data.field

; Enum variants
(enum_variant
  name: (identifier) @name.definition.data.constant.enum) @definition.data.constant.enum
```

### C#

```scheme
; Classes
(class_declaration
  name: (identifier) @name.definition.container.type.class) @definition.container.type.class

; Interfaces
(interface_declaration
  name: (identifier) @name.definition.container.type.interface) @definition.container.type.interface

; Structs
(struct_declaration
  name: (identifier) @name.definition.container.type.struct) @definition.container.type.struct

; Methods
(method_declaration
  name: (identifier) @name.definition.callable.method) @definition.callable.method

; Properties
(property_declaration
  name: (identifier) @name.definition.data.property) @definition.data.property

; Fields
(field_declaration
  (variable_declaration
    (variable_declarator
      name: (identifier) @name.definition.data.field))) @definition.data.field
```

### C/C++

```scheme
; Structs (C and C++)
(struct_specifier
  name: (type_identifier) @name.definition.container.type.struct) @definition.container.type.struct

; Unions
(union_specifier
  name: (type_identifier) @name.definition.container.type.union) @definition.container.type.union

; Classes (C++ only)
(class_specifier
  name: (type_identifier) @name.definition.container.type.class) @definition.container.type.class

; Functions
(function_definition
  declarator: (function_declarator
    declarator: (identifier) @name.definition.callable.function)) @definition.callable.function

; Struct/class fields
(field_declaration
  declarator: (field_identifier) @name.definition.data.field) @definition.data.field
```

### JavaScript

```scheme
; Classes
(class_declaration
  name: (identifier) @name.definition.container.type.class) @definition.container.type.class

; Functions
(function_declaration
  name: (identifier) @name.definition.callable.function) @definition.callable.function

; Async functions
(function_declaration
  (async)
  name: (identifier) @name.definition.callable.function.async) @definition.callable.function.async

; Methods
(method_definition
  name: (property_identifier) @name.definition.callable.method) @definition.callable.method

; Class fields
(field_definition
  property: (property_identifier) @name.definition.data.field) @definition.data.field
```

## Tag Parsing Algorithm

The Rust processing code parses tags using this algorithm:

```rust
/// Parse a tag string into components.
///
/// Returns (category, node_type, kind, subtype)
///
/// Examples:
///   "definition.container.type.struct" -> ("definition", "container", Some("type"), Some("struct"))
///   "definition.callable.function" -> ("definition", "callable", Some("function"), None)
///   "reference.data.field" -> ("reference", "data", Some("field"), None)
fn parse_tag_string(tag: &str) -> Result<(String, String, Option<String>, Option<String>)> {
    let parts: Vec<&str> = tag.split('.').collect();

    if parts.len() < 2 {
        return Err(anyhow!("Invalid tag format: {}", tag));
    }

    let category = parts[0].to_string(); // "definition" or "reference"

    if parts[1] == "file" {
        return Ok((category, "FILE".to_string(), None, None));
    }

    let node_type = parts[1].to_string(); // "container", "callable", "data"
    let kind = parts.get(2).map(|s| s.to_string());
    let subtype = parts.get(3).map(|s| s.to_string());

    Ok((category, node_type, kind, subtype))
}
```

## Mapping to Graph Schema

Tags map to the graph schema defined in `crates/codeprysm-core/src/types.rs`:

| Tag Pattern | Node Type | Kind Enum | Subtype |
|-------------|-----------|-----------|---------|
| `definition.container.{kind}[.{sub}]` | `"Container"` | `ContainerKind.{KIND}` | `{sub}` or `None` |
| `definition.callable.{kind}[.{sub}]` | `"Callable"` | `CallableKind.{KIND}` | `{sub}` or `None` |
| `definition.data.{kind}[.{sub}]` | `"Data"` | `DataKind.{KIND}` | `{sub}` or `None` |
| `definition.file` | `"FILE"` | N/A | N/A |

Example mappings:

```rust
// Tag: @definition.container.type.struct
let node_type = NodeType::Container;
let kind = ContainerKind::Type;
let subtype = Some("struct".to_string());

// Tag: @definition.callable.function.async
let node_type = NodeType::Callable;
let kind = CallableKind::Function;
let subtype = Some("async".to_string());

// Tag: @definition.data.field
let node_type = NodeType::Data;
let kind = DataKind::Field;
let subtype: Option<String> = None;
```

## Reference Tags

Reference tags follow the same structure but indicate usage rather than definition:

```scheme
; Function call
(call_expression
  function: (identifier) @name.reference.callable.function) @reference.callable.function

; Variable access
(identifier) @reference.data.field

; Type usage
(type_identifier) @reference.container.type
```

References are used to generate `USES` relationships in the graph.

## Design Rationale

### Why Declarative Tags?

1. **Language Independence**: No Rust code changes needed for new languages
2. **Explicit Semantics**: Semantic meaning visible in `.scm` files
3. **Easy Validation**: Tag structure enforces consistency
4. **Discoverability**: Developers can understand tag meaning without reading Rust code
5. **Extensibility**: New kinds/subtypes added without code changes

### Why Three Node Types?

The Container/Callable/Data taxonomy covers all code entities:

- **Container**: "Holds other things" (modules, classes, namespaces)
- **Callable**: "Can be executed" (functions, methods, constructors)
- **Data**: "Stores values" (variables, fields, constants, parameters)

This is simpler than language-specific types (class, struct, interface, trait) while preserving semantic distinctions via kind/subtype.

### Why Optional Subtypes?

Subtypes provide language-specific refinement without complicating the core model:

- **Required kind**: Broad semantic category (e.g., "type", "function", "field")
- **Optional subtype**: Language-specific detail (e.g., "struct" vs "interface" vs "trait")

This allows filtering by kind ("show me all types") while preserving distinctions ("show me only structs").

## Migration Guide

### Converting Existing Queries

**Old (v1) approach:**
```scheme
(class_definition
  name: (identifier) @name.definition.class) @definition.class
```

**New (v2) approach:**
```scheme
(class_definition
  name: (identifier) @name.definition.container.type.class) @definition.container.type.class
```

### Adding New Languages

To add a new language:

1. Create `crates/codeprysm-core/queries/{language}-tags.scm`
2. Define queries using the tag convention
3. No Rust code changes needed
4. Run `just init` to test

Example minimal query file:

```scheme
; {language}-tags.scm

; Classes
(class_definition
  name: (identifier) @name.definition.container.type.class) @definition.container.type.class

; Functions
(function_definition
  name: (identifier) @name.definition.callable.function) @definition.callable.function

; Methods
(method_definition
  name: (identifier) @name.definition.callable.method) @definition.callable.method

; Fields
(field_declaration
  name: (identifier) @name.definition.data.field) @definition.data.field
```

## Validation

Valid tag examples:
- ✅ `@definition.container.module`
- ✅ `@definition.container.type.struct`
- ✅ `@definition.callable.function`
- ✅ `@definition.callable.method.async`
- ✅ `@definition.data.field`
- ✅ `@definition.data.field.const`
- ✅ `@reference.container.type`
- ✅ `@definition.file`

Invalid tag examples:
- ❌ `@definition.class` (missing node_type/kind)
- ❌ `@definition.container` (missing kind)
- ❌ `@definition.async.function` (wrong order)
- ❌ `@container.type.struct` (missing category)

## References

- Graph Schema: `crates/codeprysm-core/src/types.rs`
- Tag Parser Implementation: `crates/codeprysm-core/src/parser.rs`
- Query Files: `crates/codeprysm-core/queries/*.scm`
- Overlay Files: `crates/codeprysm-core/queries/overlays/*.scm`

---
*Last Updated: 2025-01-05*
*Schema Version: 2.0*
