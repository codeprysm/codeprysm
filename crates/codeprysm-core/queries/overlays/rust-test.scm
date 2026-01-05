; Rust Test Detection Overlay
; Detects #[test], #[cfg(test)], and #[bench] patterns
;
; Note: In tree-sitter-rust 0.23+, attribute_item nodes are siblings to the
; items they annotate, not children. This limits our ability to capture
; test functions via attributes alone.
;
; Alternative approach: We match function_item patterns within test modules
; (declaration_list of mod_item) and rely on the module detection to infer
; test scope. Direct #[test] attribute detection requires post-processing.

; Test modules with #[cfg(test)] - the attribute is a sibling to mod_item
; We detect test modules by name convention (tests, test) since the attribute
; relationship is not directly queryable in tree-sitter 0.23+
(mod_item
  name: (identifier) @_name
  (#match? @_name "^tests?$")
  body: (declaration_list
    (function_item
      name: (identifier) @name.definition.callable.function.scope.test) @definition.callable.function.scope.test))

; Functions inside impl blocks within test modules
(mod_item
  name: (identifier) @_name
  (#match? @_name "^tests?$")
  body: (declaration_list
    (impl_item
      body: (declaration_list
        (function_item
          name: (identifier) @name.definition.callable.function.scope.test) @definition.callable.function.scope.test))))
