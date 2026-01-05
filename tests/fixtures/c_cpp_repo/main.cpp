// C++ Test File - Demonstrating C++ features

#include <iostream>
#include <string>
#include <vector>
#include <memory>

// Namespace
namespace TestNamespace {

// Simple function
int add(int a, int b) {
    return a + b;
}

// Function with default parameters
double multiply(double x, double y = 2.0) {
    return x * y;
}

// Template function
template<typename T>
T maximum(T a, T b) {
    return (a > b) ? a : b;
}

// Class definition
class Person {
private:
    std::string name;
    int age;

public:
    // Constructor
    Person(const std::string& n, int a) : name(n), age(a) {}
    
    // Destructor
    ~Person() {}
    
    // Getter methods
    std::string getName() const {
        return name;
    }
    
    int getAge() const {
        return age;
    }
    
    // Setter methods
    void setName(const std::string& n) {
        name = n;
    }
    
    void setAge(int a) {
        age = a;
    }
    
    // Method
    void greet() const {
        std::cout << "Hello, I'm " << name << std::endl;
    }
};

// Template class
template<typename T>
class Container {
private:
    std::vector<T> items;

public:
    void add(const T& item) {
        items.push_back(item);
    }
    
    T get(size_t index) const {
        return items[index];
    }
    
    size_t size() const {
        return items.size();
    }
};

// Derived class
class Employee : public Person {
private:
    std::string jobTitle;

public:
    Employee(const std::string& name, int age, const std::string& title)
        : Person(name, age), jobTitle(title) {}
    
    std::string getJobTitle() const {
        return jobTitle;
    }
    
    void work() {
        std::cout << "Working as " << jobTitle << std::endl;
    }
};

} // namespace TestNamespace

// Main function
int main() {
    TestNamespace::Person person("John", 30);
    person.greet();
    
    TestNamespace::Employee emp("Jane", 25, "Developer");
    emp.work();
    
    return 0;
}
