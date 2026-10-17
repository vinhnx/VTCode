# **Testing Guide**

This guide covers VT Code's comprehensive test suite, including unit tests, integration tests, benchmarks, and testing
best practices.

## **Test Overview**

VT Code includes a multi-layered test suite designed to ensure reliability and performance:

- **Unit Tests**: Test individual components and functions
- **Integration Tests**: Test end-to-end functionality
- **Performance Benchmarks**: Measure and track performance
- **Mock Testing**: Test with realistic mock data

## **Running Tests**

### Standalone startup benchmark

Startup measurements use the release executable directly. Build it once, then run the standalone harness with an
explicit executable path:

```bash
cargo build --release --locked --bin vtcode
VTCODE_BIN="$PWD/target/release/vtcode" \
  VTCODE_BENCH_RUNS=10 \
  cargo bench --locked --bench startup -- --noplot
```

The case matrix is intentionally limited to short-lived, provider-free commands:

```text
vtcode --version
vtcode --help
vtcode schema tools --format ndjson --name code_search
```

Each case is measured in two modes. A cold sample launches a freshly copied executable (a new copy for every sample); a
warm sample launches the same executable repeatedly after warm-up. Both modes run with isolated temporary `HOME`,
`VTCODE_CONFIG`, `VTCODE_DATA`, `VTCODE_CONFIG_PATH`, and workspace directories, so the benchmark cannot read or modify
the developer's normal configuration, credentials, data, or repository. The workspace is also explicitly selected for
each child process.

The harness writes the raw duration samples and reports a stable median and p95 for every case/mode. Keep the raw
samples with the result so regressions can be reproduced and distributions can be inspected; do not compare a lone
best-case launch. Fresh executable copies are a process/loader cold proxy, not an operating-system cache flush: the
benchmark deliberately does not flush or evict OS page caches. `VTCODE_STARTUP_TRACE=1` may be enabled for phase
diagnostics, but should remain disabled for timing samples.

For the broader local performance capture (which also includes cargo-check, first-user-I/O, and PTY first-render
measurements), use:

```bash
./scripts/perf/baseline.sh baseline
./scripts/perf/baseline.sh latest
./scripts/perf/compare.sh \
  .vtcode/perf/baseline.json .vtcode/perf/latest.json
```

### Basic Test Commands

```bash
# Recommended: run all tests quickly
cargo nextest run

# TDD mode: skip integration/e2e/slow tests, fail fast (when configured)
cargo nextest run --profile quick

# Full CI gate: retry flaky tests, no fail-fast (when configured)
cargo nextest run --profile ci

# Run only tests in crates changed since last commit (when configured)
cargo nextest run --profile changed --changed --since HEAD~1

# Run tests with detailed output
cargo nextest run -- --nocapture

# Run specific test
cargo nextest run test_name

# Run tests for specific crate
cargo nextest run -p vtcode-core

# Run tests in release mode
cargo nextest run --release
```

### Local Test Build Optimization

For local test iteration, the test build uses `CARGO_INCREMENTAL=1` (via `check-dev.sh`) to avoid full recompiles when
only a few files have changed. This overrides the `incremental = false` setting in `[profile.dev]` (which is needed for
sccache compatibility in CI).

```bash
# Fast local test builds with incremental compilation
CARGO_INCREMENTAL=1 cargo nextest run --profile quick

# Or use the dev check script (handles this automatically)
./scripts/check-dev.sh --test
```

The `quick` and `ci` profiles are optional workspace configuration. The check scripts use those profiles when available
and fall back to Nextest's `default` profile when a local checkout does not provide them; a missing profile must not
prevent the checks from running. `check-dev.sh --changed` performs its own changed-package filtering because Nextest
does not provide Cargo's `--changed --since` selection flags.

The fast gate runs Cargo checks, Clippy, changed-package metadata, and Nextest with `--locked`; verification never
updates `Cargo.lock`. Tests require `cargo-nextest` and fail if the selected suite contains no tests. A successful
`--changed` selection with no changed packages explicitly skips testing; metadata failures fail the gate instead.
Install the runner with `cargo install cargo-nextest --locked`.

| Fast gate | Command | Test selection |
| --- | --- | --- |
| Checks and lints | `./scripts/check-dev.sh` | Tests excluded |
| Checks, lints, and tests | `./scripts/check-dev.sh --test` | Quick profile when configured, otherwise default |
| Changed packages | `./scripts/check-dev.sh --changed` | Changed packages in the selected scope |
| Full workspace | `./scripts/check-dev.sh --workspace --test` | All workspace packages |

Run the script regressions with `python3 -m unittest discover -s scripts/tests -p 'test_check_dev.py'`.

### Agent-Friendly Quiet Mode

`check-dev.sh --quiet` (or `-q`) suppresses progress banners and cargo noise so an agent turn or CI job sees only the
outcome: per-check success banners, errors/warnings with full diagnostics, and the final timing summary. Failure output
is never suppressed — a failing check still prints its rustfmt diff, clippy diagnostics, or test failures in full.

```bash
# Agent/CI run: minimal output, full failure detail
./scripts/check-dev.sh --quiet
./scripts/check-dev.sh --quiet --test
```

### Structural Rule Checks

VT Code bundles a generic `ast-grep` project scaffold and materializes it into the current workspace when you run
`vtcode init`.

```bash
# Install ast-grep if needed
vtcode dependencies install ast-grep

# Materialize sgconfig.yml, rules/, and rule-tests/ in the current workspace
vtcode init

# Run the VT Code check entrypoint
vtcode check ast-grep

# Or run the underlying ast-grep commands directly
ast-grep test --config sgconfig.yml

# Scan the repository with the configured rules
ast-grep scan --config sgconfig.yml

# Legacy repo wrapper around `vtcode check ast-grep`
./scripts/check.sh ast-grep
```

If `ast-grep` is not installed yet, run `vtcode dependencies install ast-grep`. If the workspace does not have
`sgconfig.yml` yet, run `vtcode init` before invoking the check command. If a user asks for ast-grep installation or
first-use help, route them to the bundled `ast-grep` skill before falling back to external package-manager instructions.

For ast-grep rule authoring guidance, use the bundled `ast-grep` skill. It now covers the atomic / relational /
composite / utility rule cheat sheet, the YAML config cheat sheet, and CLI iteration with `scan --rule` and
`scan --inline-rules`. That skill also covers project bootstrapping with `ast-grep new` / `ast-grep new rule`, though
this repository already includes the required scaffold. Rewrite workflows such as `ast-grep run --rewrite`, YAML string
`fix`, `FixConfig`, and `expandStart` or `expandEnd` are intentionally routed through that skill, including comma and
list-item cleanup cases where the rewritten range must grow beyond the matched node. CLI-only topics such as `--stdin`,
raw `--json`, `scan -r`, `lsp`, shell completions, and GitHub Action setup are also documented there. When ast-grep rule
syntax is not expressive enough, that skill now also documents when to switch to ast-grep’s JavaScript/Python/Rust API
instead of piling more complexity into YAML. It also covers ast-grep pattern syntax itself, including `$VAR`, `$$$ARGS`,
`$_`, `$$VAR`, object-style patterns when fragments are ambiguous, rule-object features such as positive root-rule
requirements, limited `kind` ESQuery syntax, `nthChild` formulas / `reverse` / `ofRule`, `range`, relational `field`,
`stopBy`, and local/global utility rules, plus config keys and semantics such as `url`, `metadata`, `constraints`,
`severity`, `message`, `note`, `labels`, `files`, `ignores`, `transform`, `fix`, `rewriters`, `caseInsensitive` glob
objects, YAML `---` rule separators, `severity: off`, `--include-metadata`, `./` glob pitfalls, and `files` / `ignores`
precedence. It also covers FAQ-style troubleshooting such as Playground and CLI differences, using
`ast-grep run --debug-query`, incomplete fragments that need `context` plus `selector`, `kind` plus `pattern` pitfalls,
rule-order sensitivity, multi-language guidance, naming-convention matching via `constraints.regex`, and ast-grep’s
static-analysis limits. It also covers the high-level ast-grep workflow: pattern / YAML / API inputs, Tree-Sitter
parsing, Rust tree matching, search/rewrite/lint/analyze scenarios, and why ast-grep scales well across CPU cores. It
also covers pattern core concepts such as textual vs structural matching, CST vs AST, named vs unnamed nodes, `kind` vs
`field`, and significant vs trivial syntax. It also covers pattern parsing details such as invalid / incomplete /
ambiguous snippets, effective-node selection via `selector`, meta-variable detection rules, unnamed-node capture, lazy
`$$$ARGS`, and when `expandoChar` matters. It also covers the match algorithm and strictness levels used by ast-grep
commands. It also covers Find & Patch style rewrites such as `rewriters`, `transform.rewrite`, `joinBy`, and one-to-many
rewrites like splitting barrel imports. It also covers transformation-object details such as `replace`, `substring`,
`convert`, `toCase`, `separatedBy`, `CaseChange`, string-form transforms, and the experimental matching-order behavior
of `transform.rewrite`. It also covers rewriter-specific rules such as required `id` / `rule` / `fix`, rewriter-local
capture scope, rewriter-local `utils` / `transform`, and using sibling rewriters from the same list. It also covers
`sgconfig.yml` itself in more detail: `ruleDirs`, `testConfigs`, `testDir`, `snapshotDir`, `utilDirs`, `languageGlobs`
precedence, target-triple `libraryPath`, `languageSymbol`, and dynamic `injected` language selection through `$LANG`. It
also covers custom language setup, including `customLanguages`, parser compilation with `tree-sitter build`, the
`TREE_SITTER_LIBDIR` fallback, `expandoChar`, and parser inspection with `tree-sitter parse`. It also covers
multi-language documents and language injection, including built-in HTML `<script>` / `<style>` extraction,
`languageInjections`, `hostLanguage`, `injected`, `$CONTENT` captures, styled-components CSS, and GraphQL template
literals. It also covers ast-grep’s built-in language catalog, alias selection for `--lang` / YAML `language`, built-in
extension mapping, and when VT Code’s local inference subset differs from ast-grep’s full built-in list. It also covers
the programmatic API surface in more concrete terms: Node NAPI `parse` / `kind` / `pattern`, `Lang`, `SgRoot`, `SgNode`,
`NapiConfig`, Python `SgRoot` / `SgNode`, edit objects, and the deprecation of language-specific JS objects like
`js.parse(...)`. Quick-start guidance there also covers shell quoting for metavariables, Linux `ast-grep` vs `sg`, and
the optional-chaining rewrite example. Catalog-style example discovery and adaptation are also routed there, especially
when examples depend on `constraints`, `utils`, `transform`, `rewriters`, or built-in fixes.

### Integration Tests

```bash
# Run only integration tests (binary filter)
cargo nextest run -E 'binary(/integration/)'

# Run integration tests with output
cargo nextest run -E 'binary(/integration/)' -- --nocapture
```

### Benchmarks

```bash
# Run all benchmarks (sequentially; never in parallel)
cargo bench

# Run specific Criterion benches used in this workspace
cargo bench --bench allocator_throughput
cargo bench -p vtcode-core --bench tool_pipeline
cargo bench -p vtcode-core --bench agent_harness
cargo bench -p vtcode-ui --bench markdown_render
cargo bench -p vtcode-ui --bench transcript
```

See `docs/development/performance.md` Benchmark discipline for the baseline, independence, and no-gaming guardrails. The
standalone `startup` bench needs a release binary; see the Testing Guide startup section.

### Fuzz Testing (cargo-fuzz)

```bash
# List fuzz targets
cargo +nightly fuzz list

# Build and run a target for 60 seconds
cargo +nightly fuzz build shell_parser
cargo +nightly fuzz run shell_parser -- -max_total_time=60
```

See [Fuzzing Guide](./fuzzing.md) for target details, corpus layout, and crash reproduction.

## **Test Structure**

```text
tests/
 mod.rs                 # Test module declarations
 common.rs              # Shared test utilities
 mock_data.rs           # Mock data and responses
 integration_tests.rs   # End-to-end integration tests

benches/
 tool_pipeline.rs       # vtcode-core tool pipeline benchmarks
 agent_harness.rs       # interactive prompt/indexer workload benchmarks

src/
 lib.rs                 # Unit tests for library exports
 tools.rs               # Unit tests for tool registry
 tree_sitter/
     analyzer.rs        # Unit tests for tree-sitter analyzer
```

## **Test Profiles & Groups**

Tests are organized into nextest profiles and test groups for selective execution:

| Profile        | Use Case                 | Key Settings                                  |
| -------------- | ------------------------ | --------------------------------------------- |
| `default`      | Local dev – everything   | `fail-fast`, 30s timeout, 3 retry periods     |
| `quick`        | TDD iteration (optional) | Skips integration/e2e/slow tests, 10s timeout |
| `changed`      | Changed-crate testing    | `--changed --since HEAD~1` support            |
| `ci`           | Full CI gate (optional)  | `fail-fast=false`, 2 retries, 60s timeout     |
| `ci-partition` | Parallel CI shards       | Hash-based partition, 1 retry                 |

### Test Groups

Tests assigned to groups inherit resource and timeout constraints:

| Group         | Max Threads | Timeout | Assigned Tests                                          |
| ------------- | ----------- | ------- | ------------------------------------------------------- |
| `slow`        | 2           | 120s    | Tests tagged with `/slow_\|bench_\|heavy/`              |
| `integration` | 4           | 60s     | Integration binaries, tests tagged `/e2e\|integration/` |

## **Test Categories**

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

### Specification Compliance Tests

Located in standalone files in `tests/`:

- `tests/open_responses_compliance.rs`: Validates strict adherence to the
  [Open Responses](https://www.openresponses.org/) specification.

```bash
# Run Open Responses compliance tests
cargo nextest run -E 'binary(/open_responses_compliance/)'
```

### Benchmarks

Located in `benches/` directories (`benches/`, `crates/codegen/vtcode-core/benches/`,
`crates/codegen/vtcode-ui/benches/`). Follow the Benchmark discipline in `docs/development/performance.md` (sequential
runs, `black_box` outputs, `iter_batched` independence except explicit `cache_hit` benches, no edits to timed bench code
to hit a goal).

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_function(c: &mut Criterion) {
    // Benchmark setup and execution
}

criterion_group!(benches, benchmark_function);
criterion_main!(benches);
```

## **Testing Tools and Components**

### Tool Registry Testing

Test the default shell tool for file listing:

```rust
#[tokio::test]
async fn test_exec_command_lists_files() {
    let env = create_test_project();
    let mut registry = ToolRegistry::new();

    let args = json!({
        "cmd": "find . -maxdepth 1 -type f",
        "workdir": env.root()
    });

    let result = registry.execute("exec_command", args).await;
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

Test regex-based text search through the shell:

```rust
#[tokio::test]
async fn test_exec_command_rg() {
    let env = TestEnv::new();
    let content = "fn main() { println!(\"test\"); }";
    env.create_test_file("test.rs", content);

    let mut registry = ToolRegistry::new();

    let args = json!({
        "cmd": "rg 'fn main' .",
        "workdir": env.root()
    });

    let result = registry.execute("exec_command", args).await;
    assert!(result.is_ok());
}
```

Use advanced `code_search` for focused literal queries across definitions, syntactic usages, text, and paths. Use
`exec_command` or the ast-grep skill for arbitrary structural patterns.

## **Mock Data and Testing Utilities**

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

## **Performance Benchmarks**

### Core Tool Pipeline Performance

```bash
cargo bench -p vtcode-core --bench tool_pipeline
```

Measures:

- Rate limiter throughput and latency
- Tool pipeline outcome construction overhead

### Interactive Prompt and Indexed Search Performance

```bash
cargo bench -p vtcode-core --bench agent_harness
cargo nextest run -p vtcode-indexer
```

Measures warm prompt-resource cache hits, few-shot selection, tool-definition sorting, and indexed file-search scoring.
The benchmark is intentionally local and comparative rather than a CI threshold.

### Tool Cache Performance

Measures:

- Owned vs `Arc` retrieval overhead

## **Testing Best Practices**

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

## **Continuous Integration**

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
            - run: cargo nextest run
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

## **Debugging Tests**

### Running Failed Tests

```bash
# Rerun failed tests from previous run
cargo nextest run --rerun '(<run-id>)'

# Run with backtrace
RUST_BACKTRACE=1 cargo nextest run
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

## **Performance Monitoring**

### Benchmark Baselines

```bash
# Capture baseline and latest local metrics
./scripts/perf/baseline.sh baseline
./scripts/perf/baseline.sh latest
```

### Performance Regression Detection

```bash
# Compare baseline vs latest
./scripts/perf/compare.sh
```

## **Agentic Testing (risk-first)**

Based on Dan Luu `agentic-testing`: naming a technique alone does not help. Prefer small behavior nudges over large
tutorial skills.

| Quick reference | Rule                                                                                                                                                                   |
| --------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Risk-first      | List risky areas + likely mistakes before implementing; check asymmetric/boundary both sides (`[a,b]` vs `[b,a]`, never `[a,a]` vs `[a,a]`)                            |
| Fresh context   | Re-derive high-risk results without reusing production helpers; plan steps accept `verify: independent-rederive <target> (fresh context, no helper reuse)`             |
| Structured PBT  | Use `prop_oneof` / `prop_compose` / `Just` toward interesting paths with shrinking; see `crates/codegen/vtcode-core/tests/core_fuzz_props.rs` order-sensitive examples |
| Oracle required | Every test needs `assert*` / `prop_assert*` / `insta::*`; panic-only tests are a smell                                                                                 |
| Avoid           | Red-green TDD encoding current output, fully random bytes for structured parsers, identical/palindromic fixtures                                                       |

Triage with `vtcode_eval::analyze_test_source` (`TestQualitySummary::meets_agentic_testing_bar`: panic-only <=50%,
asymmetric >=20%). Lazy skill: `.agents/skills/agentic-testing/SKILL.md`.

## **Testing Checklist**

- [ ] Unit tests for all public functions
- [ ] Integration tests for component interactions
- [ ] Error handling tests
- [ ] Edge case testing
- [ ] Performance benchmarks
- [ ] Documentation examples tested
- [ ] Cross-platform compatibility
- [ ] Memory leak testing (if applicable)

## **Additional Resources**

### Testing Frameworks

- **[Rust Testing Book](https://doc.rust-lang.org/book/ch11-00-testing.html)**
- **[Criterion.rs Documentation](https://bheisler.github.io/criterion.rs/book/)**
- **[Mockito Documentation](https://docs.rs/mockito/latest/mockito/)**

### Best Practices

- **[Rust Testing Guidelines](https://rust-lang.github.io/rfcs/2909-destructuring-assignment.html)**
- **[Effective Rust Testing](https://www.lurklurk.org/effective-rust/testing.html)**

## **Getting Help**

### Common Issues

#### Test fails intermittently

- Check for race conditions in async tests
- Ensure proper test isolation
- Use unique test data for each test

#### Benchmark results vary

- Run benchmarks multiple times
- Use statistical significance testing
- Consider environmental factors

#### Mock setup is complex

- Simplify test scenarios
- Use builder patterns for complex objects
- Consider integration tests instead of complex mocks

---

## **Navigation**

- **[Back to Documentation Index](./../README.md)**
- **[User Guide](../user-guide/)**
- **[Contributing Guide](../CONTRIBUTING.md)**

---

**Happy Testing!**
