/**
 * Sample C++ module for integration testing.
 *
 * This module demonstrates various C++ language features for
 * graph generation validation including classes, templates, and namespaces.
 */

#include <iostream>
#include <vector>
#include <string>
#include <memory>
#include <functional>

// Module-level constant
constexpr int MAX_ITEMS = 100;

// Namespace definition
namespace math {

// Enum class
enum class Operation {
    Add,
    Subtract,
    Multiply,
    Divide
};

// Template function
template<typename T>
T square(T value) {
    return value * value;
}

// Template class
template<typename T>
class Container {
public:
    Container() = default;

    void add(const T& item) {
        items_.push_back(item);
    }

    T get(size_t index) const {
        return items_.at(index);
    }

    size_t size() const {
        return items_.size();
    }

private:
    std::vector<T> items_;
};

} // namespace math

// Abstract base class
class ICalculator {
public:
    virtual ~ICalculator() = default;
    virtual int getValue() const = 0;
    virtual void setValue(int value) = 0;
    virtual int add(int amount) = 0;
};

// Concrete class implementing interface
class Calculator : public ICalculator {
public:
    Calculator(int initialValue = 0)
        : value_(initialValue), history_() {}

    // Override methods
    int getValue() const override {
        return value_;
    }

    void setValue(int value) override {
        value_ = value;
    }

    int add(int amount) override {
        value_ += amount;
        history_.push_back(amount);
        return value_;
    }

    // Additional methods
    int multiply(int factor) {
        value_ *= factor;
        return value_;
    }

    // Static method
    static int squareStatic(int x) {
        return x * x;
    }

    // Const method
    const std::vector<int>& getHistory() const {
        return history_;
    }

private:
    int value_;
    std::vector<int> history_;
};

// Class with multiple inheritance
class AsyncProcessor {
public:
    explicit AsyncProcessor(const std::string& name)
        : name_(name), processedCount_(0) {}

    std::string processItem(const std::string& item) {
        processedCount_++;
        return name_ + ":" + item;
    }

    std::vector<std::string> processBatch(const std::vector<std::string>& items) {
        std::vector<std::string> results;
        results.reserve(items.size());
        for (const auto& item : items) {
            results.push_back(processItem(item));
        }
        return results;
    }

    int getProcessedCount() const {
        return processedCount_;
    }

private:
    std::string name_;
    int processedCount_;
};

// Template class with specialization
template<typename T>
class DataProcessor {
public:
    DataProcessor() : data_() {}

    void add(const T& item) {
        data_.push_back(item);
    }

    template<typename U>
    std::vector<U> map(std::function<U(const T&)> fn) {
        std::vector<U> result;
        result.reserve(data_.size());
        for (const auto& item : data_) {
            result.push_back(fn(item));
        }
        return result;
    }

private:
    std::vector<T> data_;
};

// Standalone function
int standaloneFunction(const std::string& param) {
    return static_cast<int>(param.length());
}

// Lambda stored in variable (as function pointer type)
inline auto arrowFunction = [](int x, int y) -> int {
    return x + y;
};

// Main function for testing
int main() {
    Calculator calc(0);
    calc.add(10);
    calc.multiply(2);

    std::cout << "Result: " << calc.getValue() << std::endl;

    math::Container<int> container;
    container.add(42);

    return 0;
}
