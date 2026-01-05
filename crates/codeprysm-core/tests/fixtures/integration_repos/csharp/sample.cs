/**
 * Sample C# module for integration testing.
 *
 * This module demonstrates various C# language features for
 * graph generation validation including classes, interfaces, and generics.
 */

using System;
using System.Collections.Generic;
using System.Threading.Tasks;

namespace IntegrationTests
{
    // Module-level constant
    public static class Constants
    {
        public const int MaxItems = 100;
    }

    // Enum definition
    public enum UserRole
    {
        Admin,
        User,
        Guest
    }

    // Interface definition
    public interface ICalculator
    {
        int Value { get; }
        int Add(int amount);
        int Multiply(int factor);
    }

    // Generic interface
    public interface IRepository<T> where T : class
    {
        Task<T?> FindById(string id);
        Task<IEnumerable<T>> FindAll();
        Task<T> Save(T item);
        Task<bool> Delete(string id);
    }

    // Class implementing interface
    public class Calculator : ICalculator
    {
        private int _value;
        private readonly List<int> _history;

        public Calculator(int initialValue = 0)
        {
            _value = initialValue;
            _history = new List<int>();
        }

        public int Value => _value;

        public IReadOnlyList<int> History => _history.AsReadOnly();

        public int Add(int amount)
        {
            _value += amount;
            _history.Add(amount);
            return _value;
        }

        public int Multiply(int factor)
        {
            _value *= factor;
            return _value;
        }

        public static int Square(int x) => x * x;
    }

    // Class with async methods
    public class AsyncProcessor
    {
        private readonly string _name;
        private int _processedCount;

        public AsyncProcessor(string name)
        {
            _name = name;
            _processedCount = 0;
        }

        public string Name => _name;
        public int ProcessedCount => _processedCount;

        public async Task<string> ProcessItem(string item)
        {
            await Task.Delay(10);
            _processedCount++;
            return $"{_name}:{item}";
        }

        public async Task<List<string>> ProcessBatch(IEnumerable<string> items)
        {
            var results = new List<string>();
            foreach (var item in items)
            {
                var result = await ProcessItem(item);
                results.Add(result);
            }
            return results;
        }
    }

    // Generic class
    public class DataProcessor<T>
    {
        private readonly List<T> _data;

        public DataProcessor()
        {
            _data = new List<T>();
        }

        public void Add(T item)
        {
            _data.Add(item);
        }

        public List<TResult> Map<TResult>(Func<T, TResult> selector)
        {
            var results = new List<TResult>();
            foreach (var item in _data)
            {
                results.Add(selector(item));
            }
            return results;
        }

        public List<T> Filter(Func<T, bool> predicate)
        {
            var results = new List<T>();
            foreach (var item in _data)
            {
                if (predicate(item))
                {
                    results.Add(item);
                }
            }
            return results;
        }
    }

    // Abstract base class
    public abstract class BaseService
    {
        protected string Name { get; }

        protected BaseService(string name)
        {
            Name = name;
        }

        public abstract Task Initialize();

        public virtual string GetName() => Name;
    }

    // Concrete implementation
    public class UserService : BaseService
    {
        private readonly Dictionary<string, User> _users;

        public UserService() : base("UserService")
        {
            _users = new Dictionary<string, User>();
        }

        public override async Task Initialize()
        {
            await Task.CompletedTask;
        }

        public async Task<User> CreateUser(string name, string email)
        {
            var user = new User
            {
                Id = Guid.NewGuid().ToString(),
                Name = name,
                Email = email,
                CreatedAt = DateTime.UtcNow
            };
            _users[user.Id] = user;
            return await Task.FromResult(user);
        }
    }

    // Record type (C# 9+)
    public record User
    {
        public string Id { get; init; } = "";
        public string Name { get; init; } = "";
        public string Email { get; init; } = "";
        public DateTime CreatedAt { get; init; }
    }

    // Static class with extension methods
    public static class StringExtensions
    {
        public static string Reverse(this string input)
        {
            var chars = input.ToCharArray();
            Array.Reverse(chars);
            return new string(chars);
        }
    }

    // Standalone function (as static method)
    public static class Functions
    {
        public static int StandaloneFunction(string param)
        {
            return param?.Length ?? 0;
        }

        public static async Task<Dictionary<string, object>> AsyncStandalone(string url)
        {
            await Task.Delay(100);
            return new Dictionary<string, object> { { "url", url } };
        }
    }
}
