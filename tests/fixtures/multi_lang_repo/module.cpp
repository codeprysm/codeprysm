// C++ file in multi-language repository

#include <string>

namespace Multi {

// Simple function
int cppFunction(int x) {
    return x * 2;
}

// C++ class
class CPPClass {
private:
    int value;

public:
    CPPClass(int v) : value(v) {}
    
    int getValue() const {
        return value;
    }
};

} // namespace Multi
