# Language Support in VT Code

VT Code supports a wide range of programming languages through a combination of LLM-native understanding and robust shell integration.

## Semantic Understanding

VT Code relies on the inherent ability of Large Language Models (LLMs) to understand and analyze code from raw text. This approach covers almost all modern programming languages including:

- **Rust**, **Python**, **JavaScript**, **TypeScript**, **Go**, **Java**, **C/C++**, **Swift**, **Ruby**, **PHP**, and many others.

The agent uses shell inspection through `exec_command.cmd`, patch edits through
`apply_patch`, and advanced `code_search` when focused source search is useful.
`code_search` reports recognised declarations as definitions. It classifies an
exact identifier occurrence outside a known declaration name as a syntactic
usage. A usage can belong to a different symbol with the same spelling, so it
is not a semantically resolved reference.

VT Code bundles local parsers for Rust, Python, JavaScript, TypeScript, TSX,
Go, Java, C, C++, and Bash. `code_search` uses the supported programming
language parsers for syntax-aware usage classification. Other languages can
still produce text, path, and available definition results, while the model can
reason about their source text directly.

## Tree-sitter Security Parsing (Bash)

While general code analysis is text-based, **Bash/Shell scripts** are explicitly parsed using [tree-sitter](https://tree-sitter.github.io/tree-sitter/) to ensure security.

| Language | Extensions | Analysis Method | Safety Level |
|----------|------------|-----------------|--------------|
| Bash/Shell | `.sh`, `.bash` | Tree-sitter AST | **High** (Parsed for command validation) |
| Other source languages | Any | Focused search, bundled parsers where supported, and LLM reasoning | **Standard** |

### Why use Tree-sitter for Bash?

Shell commands can be notoriously difficult to validate with simple text matching due to pipes (`|`), logical operators (`&&`, `||`), and redirections. VT Code uses `tree-sitter-bash` to accurately decompose these commands into their constituent parts, ensuring that every subcommand is checked against the configured security policies before execution.

## Syntax Highlighting

Syntax highlighting in the terminal UI and previews is handled by the `syntect` crate, which uses Sublime Text-compatible grammars for a wide variety of languages. This provides a rich visual experience without the overhead of heavy tree-sitter grammars for every language.
