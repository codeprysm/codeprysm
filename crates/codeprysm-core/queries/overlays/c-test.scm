; C/C++ Test Detection Overlay
; Detects Google Test (gtest), Catch2, and CppUnit patterns

; Google Test: TEST() macro
(call_expression
  function: (identifier) @_test
  arguments: (argument_list
    (identifier) @_suite
    (identifier) @name.definition.callable.function.scope.test)
  (#eq? @_test "TEST")) @definition.callable.function.scope.test

; Google Test: TEST_F() macro (fixture-based test)
(call_expression
  function: (identifier) @_test_f
  arguments: (argument_list
    (identifier) @_fixture
    (identifier) @name.definition.callable.function.scope.test)
  (#eq? @_test_f "TEST_F")) @definition.callable.function.scope.test

; Google Test: TEST_P() macro (parameterized test)
(call_expression
  function: (identifier) @_test_p
  arguments: (argument_list
    (identifier) @_suite
    (identifier) @name.definition.callable.function.scope.test)
  (#eq? @_test_p "TEST_P")) @definition.callable.function.scope.test

; Catch2: TEST_CASE() macro
(call_expression
  function: (identifier) @_test_case
  (#eq? @_test_case "TEST_CASE")) @definition.callable.function.scope.test

; Catch2: SECTION() macro (nested test section)
(call_expression
  function: (identifier) @_section
  (#eq? @_section "SECTION")) @definition.callable.function.scope.test

; CppUnit: CPPUNIT_TEST macro in test suite
(call_expression
  function: (identifier) @_cppunit_test
  (#eq? @_cppunit_test "CPPUNIT_TEST")) @definition.callable.function.scope.test

; Benchmark: BENCHMARK macro (Google Benchmark)
(call_expression
  function: (identifier) @_benchmark
  (#eq? @_benchmark "BENCHMARK")) @definition.callable.function.scope.benchmark
