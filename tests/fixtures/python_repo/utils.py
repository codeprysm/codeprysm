"""Utility module with helper functions and classes.

This module demonstrates:
- Nested functions
- Lambda expressions
- List comprehensions
- Context managers
"""

from collections.abc import Callable, Iterator
from contextlib import contextmanager


def outer_function(x: int) -> Callable:
    """Function that returns another function (closure).

    Args:
        x: Value to capture in closure

    Returns:
        Inner function that uses captured value
    """

    def inner_function(y: int) -> int:
        """Inner function with access to outer scope.

        Args:
            y: Value to combine with captured value

        Returns:
            Combined result
        """
        return x + y

    return inner_function


def higher_order_function(func: Callable[[int], int], value: int) -> int:
    """Function that takes another function as parameter.

    Args:
        func: Function to apply
        value: Value to pass to function

    Returns:
        Result of applying function to value
    """
    return func(value)


def list_comprehension_example(numbers: list[int]) -> list[int]:
    """Demonstrate list comprehension.

    Args:
        numbers: List of integers

    Returns:
        Filtered and transformed list
    """
    # Filter even numbers and square them
    squared_evens = [n * n for n in numbers if n % 2 == 0]
    return squared_evens


def lambda_example() -> None:
    """Demonstrate lambda expressions."""

    # Define function instead of lambda
    def double(x):
        return x * 2

    # Lambda for filtering
    numbers = [1, 2, 3, 4, 5]
    _ = list(filter(lambda x: x % 2 == 0, numbers))

    # List comprehension for mapping
    _ = [x * 2 for x in numbers]


@contextmanager
def file_manager(filename: str) -> Iterator[any]:
    """Context manager for file operations.

    Args:
        filename: Path to file

    Yields:
        File handle
    """
    print(f"Opening {filename}")
    with open(filename) as file:
        yield file
        print(f"Closing {filename}")


class BaseClass:
    """Base class for inheritance demonstration."""

    def __init__(self, name: str):
        """Initialize base class.

        Args:
            name: Object name
        """
        self.name = name

    def base_method(self) -> str:
        """Method in base class.

        Returns:
            Base message
        """
        return f"Base class: {self.name}"

    def overrideable_method(self) -> str:
        """Method that can be overridden.

        Returns:
            Base implementation message
        """
        return "Base implementation"


class DerivedClass(BaseClass):
    """Derived class demonstrating inheritance."""

    def __init__(self, name: str, value: int):
        """Initialize derived class.

        Args:
            name: Object name
            value: Additional value
        """
        super().__init__(name)
        self.value = value

    def overrideable_method(self) -> str:
        """Override base class method.

        Returns:
            Derived implementation message
        """
        return f"Derived implementation with value {self.value}"

    def derived_method(self) -> int:
        """Method specific to derived class.

        Returns:
            The value
        """
        return self.value


def exception_handling_example(x: int) -> str:
    """Demonstrate exception handling.

    Args:
        x: Value to process

    Returns:
        Success message

    Raises:
        ValueError: If x is negative
    """
    try:
        if x < 0:
            raise ValueError("Value must be non-negative")
        result = 100 / x
        return f"Result: {result}"
    except ZeroDivisionError:
        return "Cannot divide by zero"
    except ValueError as e:
        return f"Error: {e}"
    finally:
        print("Cleanup operations")
