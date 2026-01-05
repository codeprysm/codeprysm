"""Sample Python module for integration testing.

This module demonstrates various Python language features for
graph generation validation.
"""

from typing import Optional, List
import asyncio


# Module-level constant
MAX_ITEMS = 100


def standalone_function(param: str) -> str:
    """A standalone function outside any class."""
    return f"processed_{param}"


async def async_standalone(url: str) -> dict:
    """An async standalone function."""
    await asyncio.sleep(0.1)
    return {"url": url}


class Calculator:
    """A simple calculator class with methods and fields."""

    class_constant = 3.14159

    def __init__(self, initial_value: int = 0):
        """Initialize the calculator with an initial value."""
        self.value = initial_value
        self.history: List[int] = []

    def add(self, amount: int) -> int:
        """Add an amount to the current value."""
        self.value += amount
        self.history.append(amount)
        return self.value

    def multiply(self, factor: int) -> int:
        """Multiply the current value by a factor."""
        self.value *= factor
        return self.value

    @staticmethod
    def square(x: int) -> int:
        """Static method to square a number."""
        return x * x

    @classmethod
    def from_string(cls, value_str: str) -> "Calculator":
        """Class method to create from string."""
        return cls(int(value_str))


class AsyncProcessor:
    """A class with async methods."""

    def __init__(self, name: str):
        self.name = name
        self.processed_count = 0

    async def process_item(self, item: str) -> str:
        """Async method to process an item."""
        await asyncio.sleep(0.01)
        self.processed_count += 1
        return f"{self.name}:{item}"

    async def process_batch(self, items: List[str]) -> List[str]:
        """Async method to process multiple items."""
        results = []
        for item in items:
            result = await self.process_item(item)
            results.append(result)
        return results


def decorator_example(func):
    """A simple decorator function."""
    def wrapper(*args, **kwargs):
        return func(*args, **kwargs)
    return wrapper


@decorator_example
def decorated_function(x: int) -> int:
    """A function with a decorator."""
    return x + 1


class InheritedClass(Calculator):
    """A class that inherits from Calculator."""

    def __init__(self, initial_value: int = 0, precision: int = 2):
        super().__init__(initial_value)
        self.precision = precision

    def divide(self, divisor: int) -> float:
        """Divide the current value."""
        if divisor == 0:
            raise ValueError("Cannot divide by zero")
        result = self.value / divisor
        self.value = int(result)
        return round(result, self.precision)
