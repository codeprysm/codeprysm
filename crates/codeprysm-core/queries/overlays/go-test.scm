; Go Test Detection Overlay
; Detects Go testing patterns: Test*, Benchmark*, Example* functions

; Test functions: func TestXxx(t *testing.T)
(function_declaration
  name: (identifier) @name.definition.callable.function.scope.test
  parameters: (parameter_list
    (parameter_declaration
      type: (pointer_type
        (qualified_type
          package: (package_identifier) @_pkg
          name: (type_identifier) @_t))))
  (#match? @name.definition.callable.function.scope.test "^Test")
  (#eq? @_pkg "testing")
  (#eq? @_t "T")) @definition.callable.function.scope.test

; Benchmark functions: func BenchmarkXxx(b *testing.B)
(function_declaration
  name: (identifier) @name.definition.callable.function.scope.benchmark
  parameters: (parameter_list
    (parameter_declaration
      type: (pointer_type
        (qualified_type
          package: (package_identifier) @_pkg
          name: (type_identifier) @_b))))
  (#match? @name.definition.callable.function.scope.benchmark "^Benchmark")
  (#eq? @_pkg "testing")
  (#eq? @_b "B")) @definition.callable.function.scope.benchmark

; Example functions: func ExampleXxx()
(function_declaration
  name: (identifier) @name.definition.callable.function.scope.example
  (#match? @name.definition.callable.function.scope.example "^Example")) @definition.callable.function.scope.example

; Fuzz functions: func FuzzXxx(f *testing.F) (Go 1.18+)
(function_declaration
  name: (identifier) @name.definition.callable.function.scope.test
  parameters: (parameter_list
    (parameter_declaration
      type: (pointer_type
        (qualified_type
          package: (package_identifier) @_pkg
          name: (type_identifier) @_f))))
  (#match? @name.definition.callable.function.scope.test "^Fuzz")
  (#eq? @_pkg "testing")
  (#eq? @_f "F")) @definition.callable.function.scope.test

; TestMain function (test setup/teardown)
(function_declaration
  name: (identifier) @name.definition.callable.function.scope.fixture
  parameters: (parameter_list
    (parameter_declaration
      type: (pointer_type
        (qualified_type
          package: (package_identifier) @_pkg
          name: (type_identifier) @_m))))
  (#eq? @name.definition.callable.function.scope.fixture "TestMain")
  (#eq? @_pkg "testing")
  (#eq? @_m "M")) @definition.callable.function.scope.fixture
