/// A simple counter struct.
pub struct Counter {
    value: i32,
}

impl Counter {
    /// Create a new counter with initial value.
    pub fn new(initial: i32) -> Self {
        Counter { value: initial }
    }

    /// Increment the counter by amount.
    pub fn increment(&mut self, amount: i32) -> i32 {
        self.value += amount;
        self.value
    }

    /// Get the current value.
    pub fn get(&self) -> i32 {
        self.value
    }
}

/// Helper function to create a zeroed counter.
pub fn create_counter() -> Counter {
    Counter::new(0)
}

/// Async function example.
pub async fn fetch_config(path: &str) -> Result<String, std::io::Error> {
    std::fs::read_to_string(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_counter() {
        let mut c = Counter::new(5);
        assert_eq!(c.increment(3), 8);
    }
}
