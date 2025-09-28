# Agent Initialization Guide

## Overview

The `/init` command in vtcode generates a standardized `AGENTS.md` file that serves as a contributor guide for any repository. This command follows the specifications from the OpenAI Codex guide to create consistent, professional documentation optimized for 200-400 words.

## Key Features

### 🎯 **OpenAI Codex Compliant**

- Follows the exact specifications from OpenAI's Codex repository guidelines
- Maintains professional, instructional tone throughout
- Optimized for 200-400 word count for quick reading

### ・ **Intelligent Project Analysis**

- **Language Detection**: Automatically detects Rust, JavaScript/TypeScript, Python, Go, Java/Kotlin
- **Build System Recognition**: Identifies Cargo, npm/yarn, pip/poetry, Go modules, Maven/Gradle
- **Git History Analysis**: Analyzes commit patterns to detect conventional commits vs. standard messages
- **Project Structure Mapping**: Discovers source directories, test patterns, and configuration files

### **Adaptive Content Generation**

- **Dynamic Section Inclusion**: Only includes sections relevant to the detected project type
- **Word Count Optimization**: Prioritizes most important information within 200-400 words
- **Technology-Specific Guidelines**: Tailors coding standards to detected languages and frameworks

## Usage

1. **Navigate to any repository** (not just vtcode projects):

   ```bash
   cd /path/to/any/repository
   ```

2. **Launch vtcode**:

   ```bash
   /path/to/vtcode/run.sh
   # or if vtcode is in PATH
   vtcode
   ```

3. **Use the `/init` command**:
   - Type `/init` in the chat interface
   - Press Enter
   - The system will analyze the repository and generate `AGENTS.md`

## Generated Content Structure

### Always Included

- **Repository Guidelines** (title)
- **Project Structure & Module Organization** (if source dirs detected)
- **Agent-Specific Instructions** (for AI assistants)

### Conditionally Included (based on analysis)

- **Build, Test, and Development Commands** (if build systems detected)
- **Coding Style & Naming Conventions** (if languages detected)
- **Testing Guidelines** (if test patterns found)
- **Commit & Pull Request Guidelines** (includes detected commit patterns)

## Analysis Capabilities

### Language & Framework Detection

```
Rust → Cargo, clippy, rustfmt guidelines
JavaScript/TypeScript → npm/yarn, Prettier, ESLint
Python → pip/poetry, Black, pytest, PEP 8
Go → Go modules, gofmt, go vet
Java/Kotlin → Maven/Gradle, standard conventions
```

### Git History Analysis

- **Conventional Commits**: Detects if >50% of commits follow `feat:`, `fix:`, etc.
- **Standard Messages**: Falls back to general commit guidelines
- **No Git History**: Provides default commit recommendations

### Project Characteristics

- **Library vs Application**: Based on build files and structure
- **CI/CD Detection**: GitHub Actions, GitLab CI, Travis, Jenkins
- **Docker Support**: Dockerfile, docker-compose detection

## Example Output

For a Rust project with conventional commits:

```markdown
# Repository Guidelines

This document serves as a contributor guide for the my-rust-app repository.

## Project Structure & Module Organization

- `src/` - Source code
- `tests/` - Integration tests
- `examples/` - Usage examples

## Build, Test, and Development Commands

- `cargo build` - Build project
- `cargo test` - Run tests
- `cargo run` - Run application

## Coding Style & Naming Conventions

- **Indentation:** 4 spaces
- **Naming:** snake_case functions, PascalCase types
- **Formatting:** `cargo fmt`

## Commit & Pull Request Guidelines

- Use conventional commit format: `type(scope): description`
- Types: `feat`, `fix`, `docs`, `refactor`, `test`, `chore`
- Link issues with `Fixes #123` or `Closes #123`
- Ensure tests pass before submitting PRs

## Agent-Specific Instructions

- Follow established patterns above
- Include tests for new functionality
- Update documentation for API changes
```

## Integration with Other Projects

The enhanced init command is designed to work with **any repository**, not just vtcode itself:

### For Open Source Projects

- Run in contributor repositories to establish consistent guidelines
- Helps onboard new contributors with clear, concise documentation
- Adapts to existing project patterns and conventions

### For Team Development

- Standardizes documentation across multiple repositories
- Reduces onboarding time with technology-specific guidelines
- Maintains consistency in commit patterns and code style

### For AI-Assisted Development

- Provides clear context for AI coding assistants
- Includes specific instructions for maintaining code quality
- Adapts recommendations based on detected technologies

## Technical Implementation

- **Non-blocking execution** with real-time progress feedback
- **Error handling** for repositories without standard structure
- **Git integration** for commit pattern analysis
- **Intelligent content prioritization** based on word count limits
- **Technology detection** using file patterns and build configurations
- Integration with the project's tool registry

## Example Output

When executed, the command generates a file similar to:

```markdown
# Repository Guidelines

This document serves as a contributor guide for the [project name] repository...

## Project Structure & Module Organization
...

## Build, Test, and Development Commands
...
```

## Benefits

- **Standardization**: Consistent documentation across projects
- **Time Saving**: Automated generation reduces manual documentation effort
- **Best Practices**: Follows industry standards for contributor guides
- **Maintenance**: Easy to regenerate when project structure changes
