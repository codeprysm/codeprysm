"""Main module for sample repo."""


class Calculator:
    """A simple calculator class."""

    def __init__(self, initial_value: int = 0):
        self.value = initial_value

    def add(self, x: int) -> int:
        """Add x to the current value."""
        self.value += x
        return self.value

    def multiply(self, x: int) -> int:
        """Multiply current value by x."""
        self.value *= x
        return self.value


def create_calculator(initial: int = 0) -> Calculator:
    """Factory function to create a calculator."""
    return Calculator(initial)


async def fetch_data(url: str) -> dict:
    """Async function to fetch data."""
    return {"url": url, "data": None}
