# Language Support in VT Code

VT Code supports a wide range of programming languages through a combination of LLM-native understanding and robust shell integration.

## Semantic Understanding

VT Code relies on the inherent ability of Large Language Models (LLMs) to understand and analyze code from raw text. This approach covers almost all modern programming languages including:

- **Rust**, **Python**, **JavaScript**, **TypeScript**, **Go**, **Java**, **C/C++**, **Swift**, **Ruby**, **PHP**, and many others.

The agent uses tools like `grep_file` and `read_file` to explore these codebases, and its internal reasoning provides "LSP-like" capabilities (goto-definition, find-references) without the need for local AST-level parsing or grammar libraries for each language.

## Tree-sitter Security Parsing (Bash)

While general code analysis is text-based, **Bash/Shell scripts** are explicitly parsed using [tree-sitter](https://tree-sitter.github.io/tree-sitter/) to ensure security.

| Language | Extensions | Analysis Method | Safety Level |
|----------|------------|-----------------|--------------|
| Bash/Shell | `.sh`, `.bash` | Tree-sitter AST | **High** (Parsed for command validation) |
| All Others | Any | LLM Semantic | **Standard** |

### Why use Tree-sitter for Bash?

Shell commands can be notoriously difficult to validate with simple text matching due to pipes (`|`), logical operators (`&&`, `||`), and redirections. VT Code uses `tree-sitter-bash` to accurately decompose these commands into their constituent parts, ensuring that every subcommand is checked against the configured security policies before execution.

## Syntax Highlighting

Syntax highlighting in the terminal UI and previews is handled by the `syntect` crate, which uses Sublime Text-compatible grammars for a wide variety of languages. This provides a rich visual experience without the overhead of heavy tree-sitter grammars for every language.

---

*Note: Previous versions of VT Code used tree-sitter for multiple programming languages. These were removed to reduce binary size and complexity, as LLM-native analysis proved to be more flexible and equally accurate for coding tasks.*
