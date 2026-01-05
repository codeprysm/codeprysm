"""Main module demonstrating Python features for testing.

This module contains various Python constructs to validate AST parsing:
- Functions with type hints
- Classes with methods
- Async/await patterns
- Decorators
- Imports
"""

import asyncio
from dataclasses import dataclass


def simple_function(name: str) -> str:
    """Simple function with type hints.

    Args:
        name: The name to greet

    Returns:
        A greeting message
    """
    return f"Hello, {name}!"


def function_with_multiple_params(a: int, b: int, c: str = "default") -> dict[str, any]:
    """Function with multiple parameters and default values.

    Args:
        a: First integer parameter
        b: Second integer parameter
        c: Optional string parameter with default

    Returns:
        Dictionary containing the results
    """
    result = {"sum": a + b, "message": c, "product": a * b}
    return result


async def async_function(delay: float) -> str:
    """Async function demonstrating coroutine usage.

    Args:
        delay: Seconds to wait

    Returns:
        Completion message
    """
    await asyncio.sleep(delay)
    return f"Completed after {delay} seconds"


async def async_generator(n: int):
    """Async generator function.

    Args:
        n: Number of items to generate

    Yields:
        Integer values from 0 to n-1
    """
    for i in range(n):
        await asyncio.sleep(0.1)
        yield i


@dataclass
class Person:
    """Dataclass representing a person."""

    name: str
    age: int
    email: str | None = None

    def greet(self) -> str:
        """Generate a greeting message.

        Returns:
            Personalized greeting
        """
        return f"Hi, I'm {self.name} and I'm {self.age} years old"

    def update_email(self, new_email: str) -> None:
        """Update the person's email.

        Args:
            new_email: The new email address
        """
        self.email = new_email


class Calculator:
    """Calculator class demonstrating instance methods."""

    def __init__(self, initial_value: float = 0.0):
        """Initialize calculator with optional initial value.

        Args:
            initial_value: Starting value for calculations
        """
        self.value = initial_value
        self.history: list[str] = []

    def add(self, x: float) -> float:
        """Add a value to the current total.

        Args:
            x: Value to add

        Returns:
            New total value
        """
        self.value += x
        self.history.append(f"Added {x}")
        return self.value

    def subtract(self, x: float) -> float:
        """Subtract a value from the current total.

        Args:
            x: Value to subtract

        Returns:
            New total value
        """
        self.value -= x
        self.history.append(f"Subtracted {x}")
        return self.value

    def reset(self) -> None:
        """Reset calculator to zero."""
        self.value = 0.0
        self.history.clear()

    @property
    def current_value(self) -> float:
        """Get current calculator value.

        Returns:
            Current value
        """
        return self.value

    @staticmethod
    def square(x: float) -> float:
        """Calculate square of a number.

        Args:
            x: Number to square

        Returns:
            Square of x
        """
        return x * x

    @classmethod
    def from_string(cls, value_str: str) -> "Calculator":
        """Create calculator from string representation.

        Args:
            value_str: String representation of initial value

        Returns:
            New Calculator instance
        """
        return cls(float(value_str))


def decorator_example(func):
    """Example decorator function.

    Args:
        func: Function to decorate

    Returns:
        Decorated function
    """

    def wrapper(*args, **kwargs):
        print(f"Calling {func.__name__}")
        result = func(*args, **kwargs)
        print(f"Completed {func.__name__}")
        return result

    return wrapper


@decorator_example
def decorated_function(x: int) -> int:
    """Function with decorator applied.

    Args:
        x: Input value

    Returns:
        Processed value
    """
    return x * 2


class GenericContainer:
    """Container class demonstrating generic typing."""

    def __init__(self):
        """Initialize empty container."""
        self.items: list[any] = []

    def add_item(self, item: any) -> None:
        """Add an item to the container.

        Args:
            item: Item to add
        """
        self.items.append(item)

    def get_items(self) -> list[any]:
        """Get all items from container.

        Returns:
            List of all items
        """
        return self.items.copy()

    def filter_by_type(self, item_type: type) -> list[any]:
        """Filter items by type.

        Args:
            item_type: Type to filter by

        Returns:
            List of items matching the type
        """
        return [item for item in self.items if isinstance(item, item_type)]


if __name__ == "__main__":
    # Example usage
    print(simple_function("World"))
    calc = Calculator(10)
    print(f"Calculator value: {calc.add(5)}")
