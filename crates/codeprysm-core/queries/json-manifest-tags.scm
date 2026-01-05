; JSON Manifest Tags (package.json, vcpkg.json)
;
; Captures for component extraction from JSON manifest files.
; Used by TagCategory::Manifest for building Component nodes and DependsOn edges.

; ============================================================================
; Component Name
; ============================================================================

; Package name (top-level "name" field)
(pair
  key: (string (string_content) @_key)
  value: (string (string_content) @manifest.component.name)
  (#eq? @_key "name"))

; Package version (top-level "version" field)
(pair
  key: (string (string_content) @_key)
  value: (string (string_content) @manifest.component.version)
  (#eq? @_key "version"))

; ============================================================================
; Workspace Detection (for monorepo roots)
; ============================================================================

; npm/yarn/pnpm workspaces array - identifies workspace root
(pair
  key: (string (string_content) @_key)
  value: (array
    (string (string_content) @manifest.workspace.member))
  (#eq? @_key "workspaces"))

; ============================================================================
; Dependencies (all forms for analysis)
; ============================================================================

; Regular dependencies - string form: "dep": "version"
(pair
  key: (string (string_content) @_outer_key)
  value: (object
    (pair
      key: (string (string_content) @manifest.dependency.name)
      value: (string (string_content) @manifest.dependency.version)))
  (#eq? @_outer_key "dependencies"))

; Dev dependencies - string form
(pair
  key: (string (string_content) @_outer_key)
  value: (object
    (pair
      key: (string (string_content) @manifest.dependency.dev.name)
      value: (string (string_content) @manifest.dependency.dev.version)))
  (#eq? @_outer_key "devDependencies"))

; ============================================================================
; Local/Workspace Dependencies (for DependsOn edges)
; ============================================================================

; Local dependencies with workspace:* or file: or link: protocols
; These are the dependencies we track as DependsOn edges
(pair
  key: (string (string_content) @_outer_key)
  value: (object
    (pair
      key: (string (string_content) @manifest.dependency.local.name)
      value: (string (string_content) @manifest.dependency.local.version)
      (#match? @manifest.dependency.local.version "^(workspace:|file:|link:)")))
  (#eq? @_outer_key "dependencies"))

; ============================================================================
; vcpkg.json specific patterns
; ============================================================================

; vcpkg dependencies - simple string in array
(pair
  key: (string (string_content) @_key)
  value: (array
    (string (string_content) @manifest.dependency.vcpkg.simple))
  (#eq? @_key "dependencies"))

; vcpkg dependencies - object form with name field
(pair
  key: (string (string_content) @_outer_key)
  value: (array
    (object
      (pair
        key: (string (string_content) @_inner_key)
        value: (string (string_content) @manifest.dependency.vcpkg.name))
      (#eq? @_inner_key "name")))
  (#eq? @_outer_key "dependencies"))
