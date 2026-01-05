"""
Test fixture for Python constants.

This file contains various module-level constants to test Data:constant support.
"""

# Simple constants
MAX_RETRIES = 3
TIMEOUT_SECONDS = 30
DEFAULT_PORT = 8080

# String constants
API_VERSION = "v1"
BASE_URL = "https://api.example.com"
DEFAULT_ENCODING = "utf-8"

# Boolean constants
DEBUG_MODE = False
ENABLE_CACHING = True

# Collection constants
ALLOWED_METHODS = ["GET", "POST", "PUT", "DELETE"]
DEFAULT_HEADERS = {"Content-Type": "application/json"}

# Tuple constant (immutable)
SUPPORTED_VERSIONS = (1, 2, 3)

# None constant
UNSET_VALUE = None

# Computed constant
TOTAL_TIMEOUT = TIMEOUT_SECONDS * MAX_RETRIES


def use_constants():
    """Function that uses the constants."""
    return {
        "retries": MAX_RETRIES,
        "timeout": TIMEOUT_SECONDS,
        "url": f"{BASE_URL}/{API_VERSION}",
    }


class ConfigManager:
    """Class that references module constants."""

    def __init__(self):
        self.port = DEFAULT_PORT
        self.debug = DEBUG_MODE

    def get_url(self):
        return BASE_URL
