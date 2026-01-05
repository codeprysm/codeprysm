; Go Module Manifest Tags (go.mod)
;
; Captures for component extraction from go.mod files.
; Used by TagCategory::Manifest for building Component nodes and DependsOn edges.

; ============================================================================
; Module Name (Component Name)
; ============================================================================

; module github.com/org/repo
(module_directive
  (module_path) @manifest.component.name.gomod)

; ============================================================================
; Go Version
; ============================================================================

(go_directive
  (go_version) @manifest.component.version.gomod)

; ============================================================================
; Required Dependencies
; ============================================================================

; Multi-line require block:
; require (
;     github.com/pkg/errors v0.9.1
; )
(require_directive_multi
  (require_spec
    path: (module_path) @manifest.dependency.gomod.name
    version: (version) @manifest.dependency.gomod.version))

; Single-line require:
; require github.com/pkg/errors v0.9.1
(require_directive_single
  (require_spec
    path: (module_path) @manifest.dependency.gomod.single.name
    version: (version) @manifest.dependency.gomod.single.version))

; ============================================================================
; Local Replace Directives (for DependsOn edges)
; ============================================================================

; Single-line replace with local path:
; replace github.com/myorg/shared => ../shared
(replace_directive_single
  (replace_spec
    from_path: (module_path) @manifest.dependency.gomod.replace.from
    to_path: (file_path) @manifest.dependency.gomod.replace.local))

; Multi-line replace block with local paths:
; replace (
;     github.com/myorg/shared => ../shared
; )
(replace_directive_multi
  (replace_spec
    from_path: (module_path) @manifest.dependency.gomod.replace.multi.from
    to_path: (file_path) @manifest.dependency.gomod.replace.multi.local))

; ============================================================================
; Exclude Directives (for awareness)
; ============================================================================

(exclude_directive_single
  (exclude_spec
    path: (module_path) @manifest.exclude.gomod.name
    version: (version) @manifest.exclude.gomod.version))

(exclude_directive_multi
  (exclude_spec
    path: (module_path) @manifest.exclude.gomod.multi.name
    version: (version) @manifest.exclude.gomod.multi.version))
