//! Sample Rust module for integration testing.
//!
//! This module demonstrates various Rust language features for
//! graph generation validation including structs, traits, impls, and macros.

use std::collections::HashMap;
use std::sync::Mutex;

/// Module-level constant.
pub const MAX_ITEMS: usize = 100;

/// User role enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserRole {
    Admin,
    User,
    Guest,
}

/// User struct.
#[derive(Debug, Clone)]
pub struct User {
    pub id: String,
    pub name: String,
    pub email: String,
    pub role: UserRole,
}

/// Calculator trait defining calculator operations.
pub trait Calculator {
    fn add(&mut self, amount: i32) -> i32;
    fn multiply(&mut self, factor: i32) -> i32;
    fn value(&self) -> i32;
}

/// Repository trait for generic data access.
pub trait Repository<T> {
    fn find_by_id(&self, id: &str) -> Option<&T>;
    fn find_all(&self) -> Vec<&T>;
    fn save(&mut self, item: T) -> Result<(), &'static str>;
    fn delete(&mut self, id: &str) -> bool;
}

/// Simple calculator implementation.
pub struct SimpleCalculator {
    value: i32,
    history: Vec<i32>,
}

impl SimpleCalculator {
    /// Create a new calculator with an initial value.
    pub fn new(initial_value: i32) -> Self {
        Self {
            value: initial_value,
            history: Vec::new(),
        }
    }

    /// Get the operation history.
    pub fn history(&self) -> &[i32] {
        &self.history
    }

    /// Static method to square a number.
    pub fn square(x: i32) -> i32 {
        x * x
    }
}

impl Calculator for SimpleCalculator {
    fn add(&mut self, amount: i32) -> i32 {
        self.value += amount;
        self.history.push(amount);
        self.value
    }

    fn multiply(&mut self, factor: i32) -> i32 {
        self.value *= factor;
        self.value
    }

    fn value(&self) -> i32 {
        self.value
    }
}

/// Async processor for handling items.
pub struct AsyncProcessor {
    name: String,
    processed_count: Mutex<usize>,
}

impl AsyncProcessor {
    /// Create a new async processor.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            processed_count: Mutex::new(0),
        }
    }

    /// Process a single item.
    pub async fn process_item(&self, item: &str) -> String {
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        let mut count = self.processed_count.lock().unwrap();
        *count += 1;
        format!("{}:{}", self.name, item)
    }

    /// Process multiple items.
    pub async fn process_batch(&self, items: &[&str]) -> Vec<String> {
        let mut results = Vec::with_capacity(items.len());
        for item in items {
            let result = self.process_item(item).await;
            results.push(result);
        }
        results
    }

    /// Get the processed count.
    pub fn processed_count(&self) -> usize {
        *self.processed_count.lock().unwrap()
    }
}

/// Generic data processor.
pub struct DataProcessor<T> {
    data: Vec<T>,
}

impl<T> DataProcessor<T> {
    /// Create a new data processor.
    pub fn new() -> Self {
        Self { data: Vec::new() }
    }

    /// Add an item.
    pub fn add(&mut self, item: T) {
        self.data.push(item);
    }

    /// Map over all items.
    pub fn map<U, F>(&self, f: F) -> Vec<U>
    where
        F: Fn(&T) -> U,
    {
        self.data.iter().map(f).collect()
    }

    /// Filter items.
    pub fn filter<F>(&self, predicate: F) -> Vec<&T>
    where
        F: Fn(&T) -> bool,
    {
        self.data.iter().filter(|item| predicate(item)).collect()
    }
}

impl<T> Default for DataProcessor<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// User repository implementation.
pub struct UserRepository {
    users: HashMap<String, User>,
}

impl UserRepository {
    /// Create a new user repository.
    pub fn new() -> Self {
        Self {
            users: HashMap::new(),
        }
    }
}

impl Default for UserRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl Repository<User> for UserRepository {
    fn find_by_id(&self, id: &str) -> Option<&User> {
        self.users.get(id)
    }

    fn find_all(&self) -> Vec<&User> {
        self.users.values().collect()
    }

    fn save(&mut self, user: User) -> Result<(), &'static str> {
        self.users.insert(user.id.clone(), user);
        Ok(())
    }

    fn delete(&mut self, id: &str) -> bool {
        self.users.remove(id).is_some()
    }
}

/// Standalone function outside any impl block.
pub fn standalone_function(param: &str) -> usize {
    param.len()
}

/// Async standalone function.
pub async fn async_standalone(url: &str) -> HashMap<String, String> {
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    let mut result = HashMap::new();
    result.insert("url".to_string(), url.to_string());
    result
}

/// Simple macro definition.
#[macro_export]
macro_rules! create_calculator {
    ($value:expr) => {
        SimpleCalculator::new($value)
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculator() {
        let mut calc = SimpleCalculator::new(0);
        assert_eq!(calc.add(10), 10);
        assert_eq!(calc.multiply(2), 20);
    }

    #[test]
    fn test_square() {
        assert_eq!(SimpleCalculator::square(5), 25);
    }
}
