; Python Test Detection Overlay
; Detects pytest and unittest test patterns

; Test functions: test_* pattern (pytest convention)
; Non-decorated test functions
(function_definition
  name: (identifier) @name.definition.callable.function.scope.test
  (#match? @name.definition.callable.function.scope.test "^test_")) @definition.callable.function.scope.test

; Decorated test functions (e.g., @pytest.mark.parametrize)
(decorated_definition
  (decorator)* @decorator
  definition: (function_definition
    name: (identifier) @name.definition.callable.function.scope.test
    (#match? @name.definition.callable.function.scope.test "^test_"))) @definition.callable.function.scope.test

; Test classes: Test* pattern (pytest/unittest convention)
; Non-decorated test classes
(class_definition
  name: (identifier) @name.definition.container.type.class.scope.test
  (#match? @name.definition.container.type.class.scope.test "^Test")) @definition.container.type.class.scope.test

; Decorated test classes
(decorated_definition
  (decorator)* @decorator
  definition: (class_definition
    name: (identifier) @name.definition.container.type.class.scope.test
    (#match? @name.definition.container.type.class.scope.test "^Test"))) @definition.container.type.class.scope.test

; Fixture functions: @pytest.fixture decorator
(decorated_definition
  (decorator
    (call
      function: (attribute
        object: (identifier) @_pytest
        attribute: (identifier) @_fixture)))
  definition: (function_definition
    name: (identifier) @name.definition.callable.function.scope.fixture)
  (#eq? @_pytest "pytest")
  (#eq? @_fixture "fixture")) @definition.callable.function.scope.fixture

; Simple @fixture decorator (when pytest is imported as fixture)
(decorated_definition
  (decorator
    (identifier) @_fixture_decorator)
  definition: (function_definition
    name: (identifier) @name.definition.callable.function.scope.fixture)
  (#eq? @_fixture_decorator "fixture")) @definition.callable.function.scope.fixture
