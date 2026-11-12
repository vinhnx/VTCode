# Tree-sitter Integration

## Overview

VT Code uses [tree-sitter](https://tree-sitter.github.io/tree-sitter/) for one thing only:
robust, syntax-aware parsing of
shell commands. Shell commands are a security boundary, so they get a real grammar rather than heuristic text matching.

Everything else — code search, declaration lookup, usage classification — runs on VT Code's own native search engine.
There are no other tree-sitter grammars in the binary.

## Shell Safety Parsing

The primary use of tree-sitter in VT Code is within the `command_safety` module. When an agent attempts to run a shell
command (especially complex ones involving pipes, redirects, or logical operators), tree-sitter-bash is used to parse
the command into an Abstract Syntax Tree (AST).

### Key Benefits

- **Accurate Decomposition**: Correctly identifies individual sub-commands in complex pipelines like
  `cat file.txt | grep "pattern" && rm -rf /`.
- **Security Validation**: Each sub-command is independently validated against safety policies.
- **Robustness**: Handles shell-specific syntax more reliably than simple regex or string splitting.

## Architecture

The integration is centered around the following components:

- **`tree-sitter` core**: The underlying incremental parsing library.
- **`tree-sitter-bash`**: The grammar used for parsing shell commands.
- **`command_safety/shell_parser`**: The implementation that walks the bash AST to extract command vectors for safety
  checking.

The grammar lives in the `vtcode-safety` crate and is the only place tree-sitter is used. Callers outside that crate
receive parsed results through the `BashTree` / `BashNode` wrappers rather than raw tree-sitter types, so the grammar
dependency cannot leak into the rest of the workspace.

## Technical Details

VT Code has deliberately moved away from full-scale AST parsing for general programming languages (Rust, Python, etc.)
to minimize binary bloat and dependency complexity. Modern LLMs are highly proficient at understanding code from raw
text, making AST-level parsing for general coding tasks largely redundant for the agent's primary workflows.

By focusing tree-sitter usage exclusively on shell safety, we maintain high security standards while keeping the
application lightweight and efficient.

Code search and declaration lookup are served by the native content-search engine in `vtcode-commons::content_search`,
which walks the tree with the `ignore` crate and matches lines with the `regex` crate.

## Configuration

Shell safety parsing is a core security feature and is enabled by default. It does not require manual configuration.

---

_Note: Previous versions of VT Code included broader language support (Rust, Python, JavaScript, etc.) via tree-sitter.
This was removed in favor of simpler, more efficient text-based analysis which proved equally effective for LLM-assisted
coding while significantly reducing the binary size._
