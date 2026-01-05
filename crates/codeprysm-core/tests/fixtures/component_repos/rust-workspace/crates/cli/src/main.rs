//! CLI for myapp
use myapp_core::core_function;
use myapp_utils::utils_function;

fn main() {
    println!("{}", core_function());
    println!("{}", utils_function());
}
