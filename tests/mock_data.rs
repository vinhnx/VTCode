use serde_json::{Value, json};

/// Mock Gemini API responses for testing
pub struct MockGeminiResponses;

impl MockGeminiResponses {
    /// Mock response for a simple function call
    pub fn simple_function_call() -> Value {
        json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "list_files",
                            "args": {
                                "path": "."
                            }
                        }
                    }]
                }
            }]
        })
    }

    /// Mock response with text content
    pub fn text_response(content: &str) -> Value {
        json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "text": content
                    }]
                }
            }]
        })
    }

    /// Mock response for file reading
    pub fn read_file_response(content: &str) -> Value {
        json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "read_file",
                            "args": {
                                "path": "test.txt",
                                "max_bytes": 1024
                            }
                        }
                    }],
                    "text": content
                }
            }]
        })
    }

    /// Mock response for search operation
    pub fn search_response() -> Value {
        json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "functionCall": {
                            "name": "grep_file",
                            "args": {
                                "pattern": "fn main",
                                "path": ".",
                                "type": "regex",
                                "case_sensitive": false,
                                "max_results": 10
                            }
                        }
                    }]
                }
            }]
        })
    }

    /// Mock error response
    pub fn error_response(message: &str) -> Value {
        json!({
            "error": {
                "message": message,
                "code": 400
            }
        })
    }

    /// Mock streaming response
    pub fn streaming_response(chunks: Vec<&str>) -> Vec<Value> {
        chunks.iter().enumerate().map(|(i, chunk)| {
            json!({
                "candidates": [{
                    "content": {
                        "parts": [{
                            "text": chunk
                        }]
                    },
                    "finishReason": if i == chunks.len() - 1 { serde_json::Value::String("STOP".to_string()) } else { serde_json::Value::Null }
                }]
            })
        }).collect()
    }
}

/// Mock file system data for testing
pub struct MockFileSystem;

impl MockFileSystem {
    pub fn rust_source_file() -> &'static str {
        r#"use std::collections::HashMap;

fn main() {
    println!("Hello, world!");
    let mut map = HashMap::new();
    map.insert("key", "value");
    println!("{:?}", map);
}

fn calculate_sum(numbers: &[i32]) -> i32 {
    numbers.iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_calculate_sum() {
        assert_eq!(calculate_sum(&[1, 2, 3]), 6);
        assert_eq!(calculate_sum(&[]), 0);
    }
}
"#
    }

    pub fn python_source_file() -> &'static str {
        r#"import json
from typing import List, Dict

def main():
    """Main function"""
    print("Hello, Python!")
    data = {"key": "value", "numbers": [1, 2, 3]}
    print(json.dumps(data, indent=2))

def calculate_average(numbers: List[float]) -> float:
    """Calculate average of numbers"""
    if not numbers:
        return 0.0
    return sum(numbers) / len(numbers)

class Calculator:
    def __init__(self):
        self.history = []

    def add(self, a: float, b: float) -> float:
        result = a + b
        self.history.append(f"{a} + {b} = {result}")
        return result

if __name__ == "__main__":
    main()
"#
    }

    pub fn javascript_source_file() -> &'static str {
        r#"const fs = require('fs');
const path = require('path');

function readFileAsync(filePath) {
    return new Promise((resolve, reject) => {
        fs.readFile(filePath, 'utf8', (err, data) => {
            if (err) {
                reject(err);
            } else {
                resolve(data);
            }
        });
    });
}

class FileProcessor {
    constructor() {
        this.processedFiles = [];
    }

    async processDirectory(dirPath) {
        const files = await fs.promises.readdir(dirPath);
        for (const file of files) {
            const filePath = path.join(dirPath, file);
            const stat = await fs.promises.stat(filePath);

            if (stat.isDirectory()) {
                await this.processDirectory(filePath);
            } else if (file.endsWith('.js')) {
                const content = await readFileAsync(filePath);
                this.processedFiles.push({
                    path: filePath,
                    content: content,
                    size: stat.size
                });
            }
        }
    }
}

module.exports = { FileProcessor, readFileAsync };
"#
    }

    pub fn markdown_file() -> &'static str {
        r#"# Project Documentation

## Overview

This is a sample project that demonstrates various features and capabilities.

## Features

- **File Processing**: Handle different file types
- **Async Operations**: Support for asynchronous programming
- **Error Handling**: Robust error handling mechanisms
- **Testing**: Comprehensive test coverage

## Usage

```rust
use my_project::process_files;

fn main() {
    process_files("./data").expect("Failed to process files");
}
```

## API Reference

### Functions

#### `process_files(path: &str) -> Result<(), Error>`

Processes all files in the given directory path.

**Parameters:**
- `path`: Directory path to process

**Returns:**
- `Result<(), Error>`: Success or error

### Classes

#### `FileProcessor`

A class for processing files with Research-preview features.

```rust
let processor = FileProcessor::new();
processor.process("./data")?;
```

## Contributing

1. Fork the repository
2. Create a feature branch
3. Make your changes
4. Add tests
5. Submit a pull request

## License

This project is licensed under the MIT License - see the LICENSE file for details.
"#
    }

    pub fn json_config_file() -> &'static str {
        r#"{
  "project": {
    "name": "vtcode-test",
    "version": "0.1.0",
    "description": "Test configuration for vtcode"
  },
  "settings": {
    "debug": true,
    "max_connections": 100,
    "timeout": 30,
    "features": ["logging", "metrics", "tracing"]
  },
  "database": {
    "host": "localhost",
    "port": 5432,
    "name": "test_db",
    "credentials": {
      "username": "test_user",
      "password": "test_pass"
    }
  },
  "logging": {
    "level": "info",
    "format": "json",
    "outputs": ["console", "file"],
    "file": {
      "path": "/var/log/vtcode.log",
      "max_size": "10MB",
      "retention": "30d"
    }
  }
}"#
    }
}

/// Mock command line arguments for testing
pub struct MockCliArgs;

impl MockCliArgs {
    pub fn ask_command(query: &str) -> Vec<String> {
        vec!["vtcode".to_string(), "ask".to_string(), query.to_string()]
    }

    pub fn analyze_command(path: &str) -> Vec<String> {
        vec![
            "vtcode".to_string(),
            "analyze".to_string(),
            path.to_string(),
        ]
    }

    pub fn validate_command() -> Vec<String> {
        vec!["vtcode".to_string(), "validate".to_string()]
    }

    pub fn chat_command() -> Vec<String> {
        vec!["vtcode".to_string(), "chat".to_string()]
    }
}

/// Test data generators
pub struct TestDataGenerator;

impl TestDataGenerator {
    pub fn random_string(length: usize) -> String {
        use rand::{Rng, distr::Alphanumeric};

        let mut rng = rand::rng();
        (&mut rng)
            .sample_iter(&Alphanumeric)
            .take(length)
            .map(char::from)
            .collect()
    }

    pub fn random_email() -> String {
        format!("{}@{}.com", Self::random_string(8), Self::random_string(5))
    }

    pub fn random_file_path(extension: &str) -> String {
        format!(
            "/tmp/{}.{}",
            Self::random_string(12),
            extension.trim_start_matches('.')
        )
    }

    pub fn random_port() -> u16 {
        use rand::Rng;

        rand::rng().random_range(1024..65535)
    }
}
