use assert_fs::TempDir;
use assert_fs::prelude::*;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

/// Test environment setup and teardown
pub struct TestEnv {
    pub temp_dir: TempDir,
    pub original_cwd: PathBuf,
}

impl TestEnv {
    pub fn new() -> Self {
        let temp_dir = TempDir::new().expect("Failed to create temp directory");
        let original_cwd = env::current_dir().expect("Failed to get current directory");

        // Change to temp directory for tests
        env::set_current_dir(temp_dir.path()).expect("Failed to change directory");

        Self {
            temp_dir,
            original_cwd,
        }
    }

    pub fn path(&self) -> &Path {
        self.temp_dir.path()
    }

    pub fn create_test_file(&self, name: &str, content: &str) -> PathBuf {
        let file = self.temp_dir.child(name);
        file.write_str(content).expect("Failed to create test file");
        file.path().to_path_buf()
    }

    pub fn create_test_dir(&self, name: &str) -> PathBuf {
        let dir = self.temp_dir.child(name);
        dir.create_dir_all()
            .expect("Failed to create test directory");
        dir.path().to_path_buf()
    }
}

impl Default for TestEnv {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestEnv {
    fn drop(&mut self) {
        // Restore original working directory
        let _ = env::set_current_dir(&self.original_cwd);
    }
}

/// Create a temporary file with given content
pub fn create_temp_file_with_content(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().expect("Failed to create temp file");
    write!(file, "{}", content).expect("Failed to write to temp file");
    file
}

/// Create a test project structure
pub fn create_test_project() -> TestEnv {
    let env = TestEnv::new();

    // Create main source files
    env.create_test_file(
        "Cargo.toml",
        r#"
[package]
name = "test-project"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = "1.0"
tokio = { version = "1.0", features = ["macros"] }
"#,
    );

    env.create_test_file(
        "src/main.rs",
        r#"
use std::collections::HashMap;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
struct Config {
    database_url: String,
    max_connections: u32,
}

fn main() {
    println!("Hello, world!");
    let config = Config {
        database_url: "postgres://localhost:5432/mydb".to_string(),
        max_connections: 10,
    };
    println!("Config: {:?}", config);
}

fn calculate_fibonacci(n: usize) -> usize {
    if n <= 1 {
        return n;
    }
    calculate_fibonacci(n - 1) + calculate_fibonacci(n - 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fibonacci() {
        assert_eq!(calculate_fibonacci(0), 0);
        assert_eq!(calculate_fibonacci(1), 1);
        assert_eq!(calculate_fibonacci(5), 5);
    }
}
"#,
    );

    env.create_test_file(
        "src/lib.rs",
        r#"
pub mod utils;
pub mod models;

pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add() {
        let result = add(2, 2);
        assert_eq!(result, 4);
    }
}
"#,
    );

    // Create submodules
    env.create_test_dir("src");
    env.create_test_file(
        "src/utils.rs",
        r#"
use regex::Regex;

pub fn validate_email(email: &str) -> bool {
    let email_regex = Regex::new(r"^[^@]+@[^@]+\.[^@]+$").unwrap();
    email_regex.is_match(email)
}

pub fn format_name(first: &str, last: &str) -> String {
    format!("{} {}", first.trim(), last.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_email() {
        assert!(validate_email("test@example.com"));
        assert!(!validate_email("invalid-email"));
    }

    #[test]
    fn test_format_name() {
        assert_eq!(format_name("John", "Doe"), "John Doe");
    }
}
"#,
    );

    env.create_test_file(
        "src/models.rs",
        r#"
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct User {
    pub id: u64,
    pub username: String,
    pub email: String,
    pub active: bool,
}

impl User {
    pub fn new(id: u64, username: &str, email: &str) -> Self {
        Self {
            id,
            username: username.to_string(),
            email: email.to_string(),
            active: true,
        }
    }

    pub fn deactivate(&mut self) {
        self.active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_user_creation() {
        let user = User::new(1, "john_doe", "john@example.com");
        assert_eq!(user.id, 1);
        assert_eq!(user.username, "john_doe");
        assert!(user.active);
    }

    #[test]
    fn test_user_deactivation() {
        let mut user = User::new(1, "john_doe", "john@example.com");
        user.deactivate();
        assert!(!user.active);
    }
}
"#,
    );

    // Create some documentation
    env.create_test_file(
        "README.md",
        r#"
# Test Project

This is a test project for demonstrating vtcode capabilities.

## Features

- User management
- Email validation
- Fibonacci calculation
- JSON serialization

## Usage

```rust
use test_project::add;

fn main() {
    println!("2 + 2 = {}", add(2, 2));
}
```

## License

MIT
"#,
    );

    env
}

/// Helper to assert that a result contains an error with specific message
pub fn assert_error_contains<T>(result: Result<T, Box<dyn std::error::Error>>, expected_msg: &str) {
    match result {
        Ok(_) => panic!("Expected error but got success"),
        Err(e) => {
            let error_msg = e.to_string();
            if !error_msg.contains(expected_msg) {
                panic!(
                    "Expected error message '{}' not found in '{}'",
                    expected_msg, error_msg
                );
            }
        }
    }
}

/// Helper to assert that a result is successful
pub fn assert_success<T>(result: Result<T, Box<dyn std::error::Error>>) -> T {
    match result {
        Ok(value) => value,
        Err(e) => panic!("Expected success but got error: {}", e),
    }
}
