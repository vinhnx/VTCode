#  **Testing Guide**

This guide covers vtcode's comprehensive test suite, including unit tests, integration tests, benchmarks, and testing best practices.

##  **Test Overview**

vtcode includes a multi-layered test suite designed to ensure reliability and performance:

- **Unit Tests**: Test individual components and functions
- **Integration Tests**: Test end-to-end functionality
- **Performance Benchmarks**: Measure and track performance
- **Mock Testing**: Test with realistic mock data

##  **Running Tests**

### Basic Test Commands

```bash
# Run all tests
cargo test

# Run tests with detailed output
cargo test -- --nocapture

# Run specific test
cargo test test_tool_registry

# Run tests for specific module
cargo test tools::

# Run tests in release mode
cargo test --release
```

### Integration Tests

```bash
# Run only integration tests
cargo test --test integration_tests

# Run integration tests with output
cargo test --test integration_tests -- --nocapture
```

### Benchmarks

```bash
# Run all benchmarks
cargo bench

# Run specific benchmark
cargo bench -- search_benchmark

# Run benchmarks with custom options
cargo bench --features criterion/html_reports
```

##  **Test Structure**

```
tests/
 mod.rs                 # Test module declarations
 common.rs              # Shared test utilities
 mock_data.rs           # Mock data and responses
 integration_tests.rs   # End-to-end integration tests

benches/
 search_benchmark.rs    # Search performance benchmarks
 tree_sitter_benchmark.rs # Tree-sitter performance benchmarks

src/
 lib.rs                 # Unit tests for library exports
 tools.rs               # Unit tests for tool registry
 tree_sitter/
     analyzer.rs        # Unit tests for tree-sitter analyzer
```

##  **Test Categories**

### Unit Tests

Located in the source files alongside the code they test:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_specific_functionality() {
        // Test code here
    }
}
```

### Integration Tests

Located in `tests/integration_tests.rs`:

```rust
#[cfg(test)]
mod integration_tests {
    use vtcode::tools::ToolRegistry;
    use serde_json::json;

    #[tokio::test]
    async fn test_tool_integration() {
        // Integration test code here
    }
}
```

### Benchmarks

Located in `benches/` directory:

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_function(c: &mut Criterion) {
    // Benchmark setup and execution
}

criterion_group!(benches, benchmark_function);
criterion_main!(benches);
```

##  **Testing Tools and Components**

### Tool Registry Testing

Test the file system tools:

```rust
#[tokio::test]
async fn test_list_files_tool() {
    let env = create_test_project();
    let mut registry = ToolRegistry::new();

    let args = json!({
        "path": "."
    });

    let result = registry.execute("list_files", args).await;
    assert!(result.is_ok());
}
```

### Tree-sitter Testing

Test code analysis capabilities:

```rust
#[test]
fn test_parse_rust_code() {
    let analyzer = create_test_analyzer();

    let rust_code = r#"fn main() { println!("Hello"); }"#;
    let result = analyzer.parse(rust_code, LanguageSupport::Rust);
    assert!(result.is_ok());
}
```

### Search Functionality Testing

Test regex-based search:

```rust
#[tokio::test]
async fn test_grep_file_tool() {
    let env = TestEnv::new();
    let content = "fn main() { println!(\"test\"); }";
    env.create_test_file("test.rs", content);

    let mut registry = ToolRegistry::new();

    let args = json!({
        "pattern": "fn main",
        "path": "."
    });

    let result = registry.execute("grep_file", args).await;
    assert!(result.is_ok());
}
```

##  **Mock Data and Testing Utilities**

### Common Test Setup

```rust
use tests::common::{TestEnv, create_test_project};

#[test]
fn test_with_test_environment() {
    let env = TestEnv::new();
    env.create_test_file("test.txt", "content");

    // Test code here
}
```

### Mock Gemini Responses

```rust
use tests::mock_data::MockGeminiResponses;

#[test]
fn test_with_mock_response() {
    let response = MockGeminiResponses::simple_function_call();
    assert!(response["candidates"].is_array());
}
```

### Test File Creation

```rust
use tests::common::TestEnv;

#[test]
fn test_file_operations() {
    let env = TestEnv::new();

    // Create test files
    env.create_test_file("main.rs", "fn main() {}");
    env.create_test_dir("src");

    // Test operations
}
```

##  **Performance Benchmarks**

### Search Performance

```bash
cargo bench -- search_benchmark
```

Measures:

- Simple pattern search performance
- Word boundary search performance
- Case-insensitive search performance
- Search with context lines performance
- Glob pattern filtering performance

### Tree-sitter Performance

```bash
cargo bench -- tree_sitter_benchmark
```

Measures:

- Parsing performance for different languages
- Symbol extraction performance
- Code analysis performance
- File analysis performance

##  **Testing Best Practices**

### Test Organization

1. **Unit Tests**: Test individual functions and methods
2. **Integration Tests**: Test component interactions
3. **End-to-End Tests**: Test complete workflows
4. **Performance Tests**: Benchmark critical paths

### Test Naming Conventions

```rust
#[test]
fn test_descriptive_name() {
    // Test implementation
}

#[tokio::test]
async fn test_async_functionality() {
    // Async test implementation
}
```

### Assertions

```rust
// Prefer specific assertions
assert_eq!(result, expected_value);
assert!(condition, "Descriptive message");

// Use appropriate matchers
assert!(result.is_ok());
assert!(error_msg.contains("expected text"));
```

### Test Isolation

```rust
#[test]
fn test_independent_functionality() {
    let env = TestEnv::new(); // Fresh environment for each test
    // Test implementation
}
```

##  **Continuous Integration**

### GitHub Actions Setup

```yaml
name: Tests
on: [push, pull_request]

jobs:
  test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      - uses: dtolnay/rust-toolchain@stable
      - run: cargo test
      - run: cargo bench
```

### Test Coverage

```bash
# Install cargo-tarpaulin
cargo install cargo-tarpaulin

# Generate coverage report
cargo tarpaulin --out Html

# Open coverage report
open tarpaulin-report.html
```

##  **Debugging Tests**

### Running Failed Tests

```bash
# Run only failed tests
cargo test -- --failed

# Run with backtrace
RUST_BACKTRACE=1 cargo test
```

### Debugging Output

```rust
#[test]
fn test_with_debug_output() {
    let result = some_function();
    println!("Debug: {:?}", result); // Will show in --nocapture mode
    assert!(result.is_ok());
}
```

##  **Performance Monitoring**

### Benchmark Baselines

```rust
// Establish performance baselines
#[bench]
fn bench_baseline_search(b: &mut Bencher) {
    // Baseline implementation
}
```

### Performance Regression Detection

```bash
# Compare against baseline
cargo bench --baseline baseline
```

##  **Testing Checklist**

- [ ] Unit tests for all public functions
- [ ] Integration tests for component interactions
- [ ] Error handling tests
- [ ] Edge case testing
- [ ] Performance benchmarks
- [ ] Documentation examples tested
- [ ] Cross-platform compatibility
- [ ] Memory leak testing (if applicable)

##  **Additional Resources**

### Testing Frameworks

- **[Rust Testing Book](https://doc.rust-lang.org/book/ch11-00-testing.html)**
- **[Criterion.rs Documentation](https://bheisler.github.io/criterion.rs/book/)**
- **[Mockito Documentation](https://docs.rs/mockito/latest/mockito/)**

### Best Practices

- **[Rust Testing Guidelines](https://rust-lang.github.io/rfcs/2909-destructuring-assignment.html)**
- **[Effective Rust Testing](https://www.lurklurk.org/effective-rust/testing.html)**

##  **Getting Help**

### Common Issues

**Test fails intermittently**

- Check for race conditions in async tests
- Ensure proper test isolation
- Use unique test data for each test

**Benchmark results vary**

- Run benchmarks multiple times
- Use statistical significance testing
- Consider environmental factors

**Mock setup is complex**

- Simplify test scenarios
- Use builder patterns for complex objects
- Consider integration tests instead of complex mocks

---

##  **Navigation**

- **[Back to Documentation Index](./../README.md)**
- **[User Guide](../user-guide/)**
- **[API Reference](../api/)**
- **[Contributing Guide](./contributing.md)**

---

**Happy Testing! **
