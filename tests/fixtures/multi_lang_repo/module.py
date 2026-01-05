"""Python file in multi-language repository."""


def python_function(x: int) -> int:
    """Simple Python function.

    Args:
        x: Input value

    Returns:
        Doubled value
    """
    return x * 2


class PythonClass:
    """Simple Python class."""

    def __init__(self, value: int):
        """Initialize with value."""
        self.value = value

    def get_value(self) -> int:
        """Get the value."""
        return self.value
