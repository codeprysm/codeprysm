// C# Test File - Demonstrating C# features

using System;
using System.Collections.Generic;
using System.Linq;
using System.Threading.Tasks;

namespace TestNamespace
{
    // Enum with values
    [Flags]
    public enum Status
    {
        Pending,
        Active,
        Completed,
        Failed
    }

    // Struct with attribute
    [Serializable]
    public struct Point
    {
        public int X { get; set; }
        public int Y { get; set; }

        public Point(int x, int y)
        {
            X = x;
            Y = y;
        }

        public double DistanceTo(Point other)
        {
            int dx = X - other.X;
            int dy = Y - other.Y;
            return Math.Sqrt(dx * dx + dy * dy);
        }
    }

    // Another struct with interface implementation
    public struct Rectangle : IEquatable<Rectangle>
    {
        public Point TopLeft { get; set; }
        public Point BottomRight { get; set; }

        public Rectangle(Point topLeft, Point bottomRight)
        {
            TopLeft = topLeft;
            BottomRight = bottomRight;
        }

        public int Width => BottomRight.X - TopLeft.X;
        public int Height => BottomRight.Y - TopLeft.Y;
        public int Area => Width * Height;

        public bool Equals(Rectangle other)
        {
            return TopLeft.Equals(other.TopLeft) && BottomRight.Equals(other.BottomRight);
        }
    }

    // Interface
    public interface IRepository<T>
    {
        T GetById(int id);
        IEnumerable<T> GetAll();
        void Add(T item);
        void Delete(int id);
    }

    // Class with properties and attributes
    [Serializable]
    [Description("Represents a person")]
    public class Person
    {
        // Auto-properties
        public int Id { get; set; }

        [Required]
        public string Name { get; set; }

        [Range(0, 150)]
        public int Age { get; set; }

        // Constructor
        public Person(string name, int age)
        {
            Name = name;
            Age = age;
        }

        // Method with attribute
        [Obsolete("Use GreetAsync instead")]
        public string Greet()
        {
            return $"Hello, I'm {Name}";
        }

        // Async method
        public async Task<string> GreetAsync()
        {
            await Task.Delay(100);
            return Greet();
        }
    }

    // Class implementing interface
    public class PersonRepository : IRepository<Person>
    {
        private List<Person> people = new List<Person>();

        public Person GetById(int id)
        {
            return people.FirstOrDefault(p => p.Id == id);
        }

        public IEnumerable<Person> GetAll()
        {
            return people.AsEnumerable();
        }

        public void Add(Person item)
        {
            people.Add(item);
        }

        public void Delete(int id)
        {
            var person = GetById(id);
            if (person != null)
            {
                people.Remove(person);
            }
        }

        // LINQ example
        public IEnumerable<Person> GetAdults()
        {
            return people.Where(p => p.Age >= 18)
                         .OrderBy(p => p.Name);
        }
    }

    // Generic class
    public class DataStore<T> where T : class
    {
        private Dictionary<string, T> data = new Dictionary<string, T>();

        public void Set(string key, T value)
        {
            data[key] = value;
        }

        public T Get(string key)
        {
            return data.ContainsKey(key) ? data[key] : null;
        }

        public bool Contains(string key)
        {
            return data.ContainsKey(key);
        }
    }

    // Static class with extension methods
    public static class StringExtensions
    {
        public static string Capitalize(this string str)
        {
            if (string.IsNullOrEmpty(str))
                return str;
            return char.ToUpper(str[0]) + str.Substring(1);
        }

        public static bool IsEmail(this string str)
        {
            return str.Contains("@");
        }
    }

    // Program entry point
    class Program
    {
        static async Task Main(string[] args)
        {
            var person = new Person("John", 30);
            Console.WriteLine(person.Greet());

            var greeting = await person.GreetAsync();
            Console.WriteLine(greeting);

            var repo = new PersonRepository();
            repo.Add(person);
        }
    }
}
