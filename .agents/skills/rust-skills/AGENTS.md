---
name: rust-skills
description: >
  194 Rust coding rules across 14 categories. Use when writing, reviewing,
  or refactoring Rust code. Covers ownership, error handling, async patterns,
  API design, memory optimization, performance, testing, and common
  anti-patterns. Invoke with /rust-skills.
license: MIT
metadata:
  author: leonardomso
  version: "1.1.0"
  sources:
    - Rust API Guidelines
    - Rust Performance Book
    - Google Rust Style Guide
    - Microsoft Pragmatic Rust Guidelines
    - ripgrep, tokio, serde, polars codebases
---

# Rust Best Practices

Comprehensive guide for writing high-quality, idiomatic, and highly optimized Rust code. Contains 194 rules across 14 categories, prioritized by impact to guide LLMs in code generation and refactoring.

## Rule Categories by Priority

| Priority | Category | Impact | Prefix | Rules |
|----------|----------|--------|--------|-------|
| 1 | Ownership & Borrowing | CRITICAL | `own-` | 14 |
| 2 | Error Handling | CRITICAL | `err-` | 14 |
| 3 | Memory Optimization | CRITICAL | `mem-` | 15 |
| 4 | API Design | HIGH | `api-` | 17 |
| 5 | Async/Await | HIGH | `async-` | 16 |
| 6 | Compiler Optimization | HIGH | `opt-` | 12 |
| 7 | Naming Conventions | MEDIUM | `name-` | 17 |
| 8 | Type Safety | MEDIUM | `type-` | 11 |
| 9 | Testing | MEDIUM | `test-` | 15 |
| 10 | Documentation | MEDIUM | `doc-` | 11 |
| 11 | Performance Patterns | MEDIUM | `perf-` | 11 |
| 12 | Project Structure | LOW | `proj-` | 12 |
| 13 | Clippy & Linting | LOW | `lint-` | 12 |
| 14 | Anti-patterns | REFERENCE | `anti-` | 17 |

Full rule reference and detailed examples live in `rules/`; per-source digests live in
`references/` (adapted from [BeautifulRust](https://github.com/morganlinton/BeautifulRust), MIT). See the [SKILL.md](SKILL.md) for complete quick-reference tables, task-based guidance, and sources.
