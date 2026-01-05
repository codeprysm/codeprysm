; C# Test Detection Overlay
; Detects NUnit, xUnit, and MSTest patterns

; NUnit [Test] attribute
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.test
  (#eq? @_attr "Test")) @definition.callable.method.scope.test

; NUnit [TestCase] attribute
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.test
  (#eq? @_attr "TestCase")) @definition.callable.method.scope.test

; xUnit [Fact] attribute
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.test
  (#eq? @_attr "Fact")) @definition.callable.method.scope.test

; xUnit [Theory] attribute
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.test
  (#eq? @_attr "Theory")) @definition.callable.method.scope.test

; MSTest [TestMethod] attribute
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.test
  (#eq? @_attr "TestMethod")) @definition.callable.method.scope.test

; MSTest [DataTestMethod] attribute
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.test
  (#eq? @_attr "DataTestMethod")) @definition.callable.method.scope.test

; NUnit [TestFixture] attribute on classes
(class_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.container.type.class.scope.test
  (#eq? @_attr "TestFixture")) @definition.container.type.class.scope.test

; MSTest [TestClass] attribute on classes
(class_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.container.type.class.scope.test
  (#eq? @_attr "TestClass")) @definition.container.type.class.scope.test

; NUnit [SetUp] attribute (fixture)
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.fixture
  (#eq? @_attr "SetUp")) @definition.callable.method.scope.fixture

; NUnit [TearDown] attribute (fixture)
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.fixture
  (#eq? @_attr "TearDown")) @definition.callable.method.scope.fixture

; MSTest [TestInitialize] attribute (fixture)
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.fixture
  (#eq? @_attr "TestInitialize")) @definition.callable.method.scope.fixture

; MSTest [TestCleanup] attribute (fixture)
(method_declaration
  (attribute_list
    (attribute
      name: (identifier) @_attr))
  name: (identifier) @name.definition.callable.method.scope.fixture
  (#eq? @_attr "TestCleanup")) @definition.callable.method.scope.fixture
