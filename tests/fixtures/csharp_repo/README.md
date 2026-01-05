# C# Test Repository

## Files
- `Program.cs`: C# features including interfaces, properties, LINQ, async/await, generics

## Expected AST Entities

### Interfaces
- `IRepository<T>`

### Classes
- `Person`
- `PersonRepository`
- `DataStore<T>` (generic)
- `StringExtensions` (static)
- `Program`

### Methods
From Person:
- Constructor
- `Greet`
- `GreetAsync` (async)

From PersonRepository:
- `GetById`
- `GetAll`
- `Add`
- `Delete`
- `GetAdults` (with LINQ)

From DataStore:
- `Set`
- `Get`
- `Contains`

From StringExtensions:
- `Capitalize` (extension)
- `IsEmail` (extension)

From Program:
- `Main` (async)

## C# Features
- ✅ Interfaces
- ✅ Properties
- ✅ LINQ
- ✅ Async/await
- ✅ Generics
- ✅ Extension methods
- ✅ Namespaces

## Entity Count: ~25
