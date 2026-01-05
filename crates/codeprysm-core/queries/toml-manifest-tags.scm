; TOML Manifest Tags (Cargo.toml, pyproject.toml)
;
; Captures for component extraction from TOML manifest files.
; Used by TagCategory::Manifest for building Component nodes and DependsOn edges.

; ============================================================================
; Cargo.toml - Package Name and Version
; ============================================================================

; [package] name = "crate-name"
(table
  (bare_key) @_section
  (pair
    (bare_key) @_key
    (string) @manifest.component.name.cargo)
  (#eq? @_section "package")
  (#eq? @_key "name"))

; [package] version = "0.1.0"
(table
  (bare_key) @_section
  (pair
    (bare_key) @_key
    (string) @manifest.component.version.cargo)
  (#eq? @_section "package")
  (#eq? @_key "version"))

; ============================================================================
; Cargo.toml - Workspace Detection
; ============================================================================

; [workspace] members = ["crates/*"]
(table
  (bare_key) @_section
  (pair
    (bare_key) @_key
    (array
      (string) @manifest.workspace.member.cargo))
  (#eq? @_section "workspace")
  (#eq? @_key "members"))

; Marks this as workspace root
(table
  (bare_key) @manifest.workspace.root.cargo
  (#eq? @manifest.workspace.root.cargo "workspace"))

; ============================================================================
; Cargo.toml - Dependencies
; ============================================================================

; Simple string dependency: dep = "version"
(table
  (bare_key) @_section
  (pair
    (bare_key) @manifest.dependency.cargo.name
    (string) @manifest.dependency.cargo.version)
  (#eq? @_section "dependencies"))

; Dev dependencies section
(table
  (bare_key) @_section
  (pair
    (bare_key) @manifest.dependency.cargo.dev.name
    (string) @manifest.dependency.cargo.dev.version)
  (#eq? @_section "dev-dependencies"))

; Build dependencies section
(table
  (bare_key) @_section
  (pair
    (bare_key) @manifest.dependency.cargo.build.name
    (string) @manifest.dependency.cargo.build.version)
  (#eq? @_section "build-dependencies"))

; ============================================================================
; Cargo.toml - Local Path Dependencies (for DependsOn edges)
; ============================================================================

; Path dependency: dep = { path = "../sibling" }
(table
  (bare_key) @_section
  (pair
    (bare_key) @manifest.dependency.cargo.path.name
    (inline_table
      (pair
        (bare_key) @_key
        (string) @manifest.dependency.cargo.path.value)
      (#eq? @_key "path")))
  (#eq? @_section "dependencies"))

; Dev path dependency
(table
  (bare_key) @_section
  (pair
    (bare_key) @manifest.dependency.cargo.path.dev.name
    (inline_table
      (pair
        (bare_key) @_key
        (string) @manifest.dependency.cargo.path.dev.value)
      (#eq? @_key "path")))
  (#eq? @_section "dev-dependencies"))

; ============================================================================
; pyproject.toml - PEP 621 Project Name
; ============================================================================

; [project] name = "package-name"
(table
  (bare_key) @_section
  (pair
    (bare_key) @_key
    (string) @manifest.component.name.python)
  (#eq? @_section "project")
  (#eq? @_key "name"))

; [project] version = "1.0.0"
(table
  (bare_key) @_section
  (pair
    (bare_key) @_key
    (string) @manifest.component.version.python)
  (#eq? @_section "project")
  (#eq? @_key "version"))

; ============================================================================
; pyproject.toml - Poetry Name (tool.poetry section)
; ============================================================================

; [tool.poetry] name = "package-name"
(table
  (dotted_key) @_section
  (pair
    (bare_key) @_key
    (string) @manifest.component.name.poetry)
  (#eq? @_section "tool.poetry")
  (#eq? @_key "name"))

; ============================================================================
; pyproject.toml - Poetry Path Dependencies
; ============================================================================

; Poetry path dependency: dep = { path = "../sibling" }
(table
  (dotted_key) @_section
  (pair
    (bare_key) @manifest.dependency.poetry.path.name
    (inline_table
      (pair
        (bare_key) @_key
        (string) @manifest.dependency.poetry.path.value)
      (#eq? @_key "path")))
  (#match? @_section "^tool\\.poetry"))
