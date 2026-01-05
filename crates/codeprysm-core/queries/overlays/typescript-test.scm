; TypeScript Test Detection Overlay
; Detects Jest, Mocha, Vitest, and other common test patterns
; (Same patterns as JavaScript since test frameworks use identical syntax)

; Test suite: describe() blocks
(call_expression
  function: (identifier) @_describe
  arguments: (arguments
    (string) @name.definition.container.type.scope.test
    [(arrow_function) (function_expression)])
  (#eq? @_describe "describe")) @definition.container.type.scope.test

; Test case: it() blocks
(call_expression
  function: (identifier) @_it
  arguments: (arguments
    (string) @name.definition.callable.function.scope.test
    [(arrow_function) (function_expression)])
  (#eq? @_it "it")) @definition.callable.function.scope.test

; Test case: test() blocks (Jest/Vitest)
(call_expression
  function: (identifier) @_test
  arguments: (arguments
    (string) @name.definition.callable.function.scope.test
    [(arrow_function) (function_expression)])
  (#eq? @_test "test")) @definition.callable.function.scope.test

; Lifecycle hooks: beforeEach
(call_expression
  function: (identifier) @_beforeEach
  arguments: (arguments
    [(arrow_function) (function_expression)] @definition.callable.function.scope.fixture)
  (#eq? @_beforeEach "beforeEach")) @name.definition.callable.function.scope.fixture

; Lifecycle hooks: afterEach
(call_expression
  function: (identifier) @_afterEach
  arguments: (arguments
    [(arrow_function) (function_expression)] @definition.callable.function.scope.fixture)
  (#eq? @_afterEach "afterEach")) @name.definition.callable.function.scope.fixture

; Lifecycle hooks: beforeAll
(call_expression
  function: (identifier) @_beforeAll
  arguments: (arguments
    [(arrow_function) (function_expression)] @definition.callable.function.scope.fixture)
  (#eq? @_beforeAll "beforeAll")) @name.definition.callable.function.scope.fixture

; Lifecycle hooks: afterAll
(call_expression
  function: (identifier) @_afterAll
  arguments: (arguments
    [(arrow_function) (function_expression)] @definition.callable.function.scope.fixture)
  (#eq? @_afterAll "afterAll")) @name.definition.callable.function.scope.fixture
