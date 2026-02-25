# Tree-sitter Integration

## Overview

VT Code uses [tree-sitter](https://tree-sitter.github.io/tree-sitter/) for robust, syntax-aware parsing of shell commands. This is a critical security feature that ensures shell commands are correctly decomposed and validated before execution.

## Shell Safety Parsing

The primary use of tree-sitter in VT Code is within the `command_safety` module. When an agent attempts to run a shell command (especially complex ones involving pipes, redirects, or logical operators), tree-sitter-bash is used to parse the command into an Abstract Syntax Tree (AST).

### Key Benefits

- **Accurate Decomposition**: Correctly identifies individual sub-commands in complex pipelines like `cat file.txt | grep "pattern" && rm -rf /`.
- **Security Validation**: Each sub-command is independently validated against safety policies.
- **Robustness**: Handles shell-specific syntax more reliably than simple regex or string splitting.

## Architecture

The integration is centered around the following components:

- **`tree-sitter` core**: The underlying incremental parsing library.
- **`tree-sitter-bash`**: The grammar used for parsing shell commands.
- **`shell_parser.rs`**: The implementation that walks the bash AST to extract command vectors for safety checking.

## Technical Details

VT Code has deliberately moved away from full-scale AST parsing for general programming languages (Rust, Python, etc.) to minimize binary bloat and dependency complexity. Modern LLMs are highly proficient at understanding code from raw text, making AST-level parsing for general coding tasks largely redundant for the agent's primary workflows.

By focusing tree-sitter usage exclusively on shell safety, we maintain high security standards while keeping the application lightweight and efficient.

## Configuration

Shell safety parsing is a core security feature and is enabled by default. It does not require manual configuration.

---

*Note: Previous versions of VT Code included broader language support (Rust, Python, JavaScript, etc.) via tree-sitter. This was removed in favor of simpler, more efficient text-based analysis which proved equally effective for LLM-assisted coding while significantly reducing the binary size.*
