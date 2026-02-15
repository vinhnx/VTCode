# Tree-sitter Language Support Matrix

`vtcode` uses [tree-sitter](https://tree-sitter.github.io/tree-sitter/) for advanced code analysis, symbol extraction, and syntax highlighting. 

## Supported Languages

The following table lists the languages currently supported by `vtcode` and the features available for each.

| Language | Extensions | Symbol Extraction | Highlighting | Dependency Analysis | Notes |
|----------|------------|-------------------|--------------|---------------------|-------|
| Rust | `.rs` | Yes | Yes | Yes | |
| Python | `.py` | Yes | Yes | Yes | |
| JavaScript | `.js`, `.jsx` | Yes | Yes | Yes | |
| TypeScript | `.ts`, `.tsx` | Yes | Yes | Yes | |
| Go | `.go` | Yes | Yes | Basic | |
| Java | `.java` | Yes | Yes | Basic | |
| Bash | `.sh`, `.bash` | Yes | Yes | Yes | |
| Swift | `.swift` | Yes | Yes | Basic | Requires `swift` feature |

## Feature Implementation Status

- **Symbol Extraction**: Extraction of functions, classes, structs, interfaces, traits, and variables using language-specific tree-sitter queries.
- **Highlighting**: Syntax highlighting using tree-sitter grammars (currently used for terminal UI and preview).
- **Dependency Analysis**: Identification of imports/dependencies within the source file.

## Configuration

Language support is automatically detected based on file extensions. For more information on how to configure tree-sitter or add new languages, see the [Developer Documentation](development/README.md).
