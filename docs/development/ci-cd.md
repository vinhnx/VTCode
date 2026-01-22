# CI/CD and Code Quality

This document describes the CI/CD pipeline and code quality tools used in the vtcode project.

## GitHub Actions Workflows

The project uses several GitHub Actions workflows to ensure code quality and automate testing:

### 1. CI Workflow (`ci.yml`)

**Triggers:**

- Push to `main`/`master` branches
- Pull requests to `main`/`master` branches

**Jobs:**

- **Format Check (rustfmt)**: Ensures code is properly formatted
- **Lint Check (clippy)**: Runs comprehensive linting
- **Test**: Runs tests on Ubuntu, macOS, and Windows
- **Benchmarks**: Performance regression testing
- **Security Audit**: Checks for vulnerable dependencies
- **Documentation**: Builds and tests documentation

### 2. Code Quality Workflow (`code-quality.yml`)

**Triggers:**

- Push to `main`/`master` branches
- Pull requests to `main`/`master` branches

**Jobs:**

- **Format Check**: Comprehensive rustfmt checking
- **Lint Check**: Research-preview clippy linting with all targets and features
- **Unused Dependencies**: Checks for unused dependencies
- **Outdated Dependencies**: Identifies outdated dependencies
- **MSRV**: Minimum Supported Rust Version verification
- **License Check**: Dependency license compliance

### 3. Development Workflow (`development.yml`)

**Triggers:**

- Push to `develop`/`dev` branches and feature branches
- Pull requests to `develop`/`dev` branches

**Jobs:**

- **Development Check**: Full development workflow
- **Performance Check**: Benchmark comparisons for PRs
- **Code Coverage**: Test coverage reporting

### 4. Nightly Build Workflow (`nightly.yml`)

**Triggers:**

- Scheduled nightly at 3 AM UTC
- Manual trigger with reason

**Jobs:**

- **Nightly Test**: Tests against latest Rust nightly
- **MSRV Test**: Minimum supported version testing
- **Feature Matrix**: Tests different feature combinations

## Code Quality Tools

### rustfmt

**Installation:**

```bash
rustup component add rustfmt
```

**Usage:**

```bash
# Check formatting
cargo fmt --all -- --check

# Auto-format code
cargo fmt --all

# Print current configuration
cargo fmt --print-config default rustfmt.toml
```

**Configuration:**
Create a `rustfmt.toml` or `.rustfmt.toml` file in your project root:

```toml
edition = "2021"
max_width = 100
tab_spaces = 4
```

### clippy

**Installation:**

```bash
rustup component add clippy
```

**Usage:**

```bash
# Run clippy with warnings as errors
cargo clippy -- -D warnings

# Run on specific target
cargo clippy --lib

# Fix clippy suggestions automatically
cargo clippy --fix
```

**Common clippy lints:**

- `clippy::all`: Enable all lints
- `clippy::pedantic`: More strict lints
- `clippy::nursery`: Experimental lints
- `clippy::cargo`: Cargo.toml specific lints

## Local Development

### Development Check Script

Use the provided development check script to run the same checks locally:

```bash
# Run all checks
./scripts/check.sh

# Run specific checks
./scripts/check.sh fmt      # Format check
./scripts/check.sh clippy   # Clippy check
./scripts/check.sh test     # Run tests
./scripts/check.sh build    # Build project
./scripts/check.sh docs     # Generate docs
```

### Manual Setup

To set up the development environment manually:

```bash
# Install required components
rustup component add rustfmt clippy

# Install additional tools
cargo install cargo-audit      # Security auditing
cargo install cargo-outdated   # Dependency checking
cargo install cargo-udeps      # Unused dependencies
cargo install cargo-msrv       # MSRV checking
cargo install cargo-license    # License checking
cargo install cargo-tarpaulin  # Code coverage
```

## Best Practices

### 1. Pre-commit Hooks

Set up pre-commit hooks to run checks before committing:

```bash
# Install pre-commit (if using)
pre-commit install

# Or create .git/hooks/pre-commit manually:
#!/bin/bash
./scripts/check.sh
```

### 2. Editor Integration

#### VS Code

Add to `.vscode/settings.json`:

```json
{
  "rust-analyzer.checkOnSave.command": "clippy",
  "editor.formatOnSave": true,
  "rust-analyzer.rustfmt.enableRangeFormatting": true
}
```

#### Vim/Neovim

```vim
autocmd BufWritePre *.rs :silent! !cargo fmt -- %:p
```

### 3. IDE Integration

Most Rust IDEs support rustfmt and clippy:

- **IntelliJ/CLion**: Built-in Rust plugin
- **VS Code**: rust-analyzer extension
- **Vim**: rust.vim plugin
- **Emacs**: rustic-mode

## CI/CD Configuration

### Branch Protection

Configure branch protection rules in GitHub:

1. Go to repository Settings → Branches
2. Add rule for `main`/`master` branch
3. Require status checks to pass:
   - `fmt`
   - `clippy`
   - `test`
   - `security-audit`

### Status Badges

Add these badges to your README:

```markdown
[![CI](https://github.com/yourusername/vtcode/actions/workflows/ci.yml/badge.svg)](https://github.com/yourusername/vtcode/actions/workflows/ci.yml)
[![Code Quality](https://github.com/yourusername/vtcode/actions/workflows/code-quality.yml/badge.svg)](https://github.com/yourusername/vtcode/actions/workflows/code-quality.yml)
```

## Troubleshooting

### Common Issues

#### rustfmt not found

```bash
rustup component add rustfmt
rustup update
```

#### clippy warnings not showing

```bash
cargo clippy -- -W clippy::all
```

#### MSRV issues

```bash
cargo msrv --workspace
cargo msrv --workspace set 1.93.0  # Set specific version
```

#### Dependency issues

```bash
cargo update
cargo outdated
cargo udeps
```

### Performance Optimization

#### Faster CI builds

```yaml
# In workflow
- uses: actions/cache@v3
  with:
    path: |
      ~/.cargo/registry
      ~/.cargo/git
      target
    key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
```

#### Parallel jobs

```yaml
strategy:
  matrix:
    os: [ubuntu-latest, macos-latest, windows-latest]
```

## Security

### Dependency Auditing

```bash
# Install cargo-audit
cargo install cargo-audit

# Run audit
cargo audit

# Fix vulnerabilities
cargo audit fix
```

### License Compliance

```bash
# Check licenses
cargo install cargo-license
cargo license --workspace
```

## References

- [rustfmt Documentation](https://rust-lang.github.io/rustfmt/)
- [clippy Documentation](https://rust-lang.github.io/rust-clippy/)
- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [Cargo Documentation](https://doc.rust-lang.org/cargo/)
