# vtcode-file-search

Fast, parallel fuzzy file search library for VT Code. Provides both a library API and a standalone CLI binary.

## Conventions

- The library (`lib.rs`) and binary (`main.rs`) share the same crate. Keep binary-specific code in `main.rs`.
- Uses `nucleo-matcher` for fuzzy matching. Do not add alternative matchers.
- Uses `ignore` for respecting `.gitignore` patterns during traversal.
- Search parallelism uses `tokio` with `num_cpus` for thread count. Do not hardcode thread counts.
- The binary accepts CLI arguments via `clap` derive macros.

## Features

- Default: all features enabled.

## Dependencies

- `nucleo-matcher` (fuzzy matching)
- `ignore` (gitignore-aware traversal)
- `tokio` (async runtime)
- `clap` (CLI parsing, binary only)
- `vtcode-commons` (shared utilities)
