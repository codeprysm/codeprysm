; C++ Test Detection Overlay
; Detects Google Test (gtest), Catch2, and Boost.Test patterns

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

; Google Test: TYPED_TEST() macro
(call_expression
  function: (identifier) @_typed_test
  arguments: (argument_list
    (identifier) @_suite
    (identifier) @name.definition.callable.function.scope.test)
  (#eq? @_typed_test "TYPED_TEST")) @definition.callable.function.scope.test

; Catch2: TEST_CASE() macro
(call_expression
  function: (identifier) @_test_case
  (#eq? @_test_case "TEST_CASE")) @definition.callable.function.scope.test

; Catch2: SECTION() macro
(call_expression
  function: (identifier) @_section
  (#eq? @_section "SECTION")) @definition.callable.function.scope.test

; Catch2: TEMPLATE_TEST_CASE() macro
(call_expression
  function: (identifier) @_template_test_case
  (#eq? @_template_test_case "TEMPLATE_TEST_CASE")) @definition.callable.function.scope.test

; Boost.Test: BOOST_AUTO_TEST_CASE() macro
(call_expression
  function: (identifier) @_boost_test
  (#eq? @_boost_test "BOOST_AUTO_TEST_CASE")) @definition.callable.function.scope.test

; Boost.Test: BOOST_FIXTURE_TEST_CASE() macro
(call_expression
  function: (identifier) @_boost_fixture_test
  (#eq? @_boost_fixture_test "BOOST_FIXTURE_TEST_CASE")) @definition.callable.function.scope.test

; Test fixture classes (inheriting from ::testing::Test)
(class_specifier
  name: (type_identifier) @name.definition.container.type.class.scope.fixture
  (base_class_clause
    (type_identifier) @_base)
  (#eq? @_base "Test")) @definition.container.type.class.scope.fixture

; Benchmark: BENCHMARK macro (Google Benchmark)
(call_expression
  function: (identifier) @_benchmark
  (#eq? @_benchmark "BENCHMARK")) @definition.callable.function.scope.benchmark
