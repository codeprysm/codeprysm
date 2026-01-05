//! Utils crate for myapp
use myapp_core::core_function;

pub fn utils_function() -> String {
    format!("utils: {}", core_function())
}
