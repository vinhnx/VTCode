# Tree-sitter Integration Guide

## Overview

The tree-sitter integration provides Research-preview code parsing and analysis capabilities to enhance vtcode's understanding of codebases. This implementation follows Anthropic's breakthrough engineering approach for SWE-bench optimization.

## Features

### **Research-preview Code Parsing**

- **Syntax-aware parsing** for multiple programming languages
- **Abstract Syntax Tree (AST)** generation and traversal
- **Incremental parsing** for efficient updates
- **Language-agnostic architecture** for extensibility

### **Code Analysis**

- **Comprehensive metrics** calculation (complexity, maintainability, etc.)
- **Symbol extraction** (functions, classes, variables, imports)
- **Dependency analysis** and relationship mapping
- **Code quality assessment** with actionable insights

### **Code Navigation**

- **Go-to-definition** functionality
- **Symbol search** and discovery
- **Scope analysis** and hierarchy understanding
- **Reference finding** and cross-referencing

### **Refactoring Support**

- **Intelligent refactoring suggestions**
- **Safe code transformation** with conflict detection
- **Preview capabilities** before applying changes
- **Multi-step refactoring** workflows

## Supported Languages

| Language | Status | Parser |
|----------|--------|---------|
| Rust |  Full Support | `tree-sitter-rust` |
| Python |  Full Support | `tree-sitter-python` |
| JavaScript |  Full Support | `tree-sitter-javascript` |
| TypeScript |  Full Support | `tree-sitter-typescript` |
| Go |  Full Support | `tree-sitter-go` |
| Java |  Full Support | `tree-sitter-java` |
| Swift |  Full Support (feature: `tree-sitter-swift`) | `tree-sitter-swift` |

Swift parsing leverages the official [tree-sitter/swift-tree-sitter](https://github.com/tree-sitter/swift-tree-sitter)
grammar through the `tree-sitter-swift` crate, and is available when the `tree-sitter-swift` feature is enabled.

## Architecture

```
src/tree_sitter/
 mod.rs                 # Main module definition
 analyzer.rs           # Core parsing and analysis engine
 languages.rs          # Language-specific queries and patterns
 analysis.rs           # Code analysis and metrics calculation
 navigation.rs         # Code navigation and symbol lookup
 refactoring.rs        # Refactoring operations and suggestions
```

## Usage Examples

### Basic Code Analysis

```rust
use vtcode::tree_sitter::{TreeSitterAnalyzer, CodeAnalyzer, LanguageSupport};

// Initialize analyzers
let mut analyzer = TreeSitterAnalyzer::new()?;
let code_analyzer = CodeAnalyzer::new(&LanguageSupport::Rust);

// Parse source code
let source_code = r#"
fn main() {
    println!("Hello, world!");
}
"#;

let tree = analyzer.parse(source_code, LanguageSupport::Rust)?;
let syntax_tree = analyzer.parse_syntax_tree(tree, source_code, LanguageSupport::Rust);

// Perform comprehensive analysis
let analysis = code_analyzer.analyze(&syntax_tree, "example.rs");

// Access results
println!("Lines of code: {}", analysis.metrics.lines_of_code);
println!("Functions: {}", analysis.metrics.functions_count);
println!("Complexity: {}", analysis.complexity.cyclomatic_complexity);
```

### Symbol Extraction

```rust
use vtcode::tree_sitter::languages::{LanguageAnalyzer, SymbolInfo};

// Extract symbols from parsed code
let language_analyzer = LanguageAnalyzer::new(&LanguageSupport::Rust);
let symbols = language_analyzer.extract_symbols(&syntax_tree);

// Work with extracted symbols
for symbol in symbols {
    println!("Found {}: {} at line {}",
             symbol.kind, symbol.name, symbol.position.row + 1);

    if let Some(signature) = &symbol.signature {
        println!("  Signature: {}", signature);
    }
}
```

### Code Navigation

```rust
use vtcode::tree_sitter::navigation::CodeNavigator;

// Build navigation index
let mut navigator = CodeNavigator::new();
navigator.build_index(&symbols);

// Navigate to definitions
if let Some(location) = navigator.goto_definition("main") {
    println!("Found 'main' at line {}", location.target.get_position().row + 1);
}

// Search for symbols
let search_results = navigator.search_symbols("print", None);
println!("Found {} symbols matching 'print'", search_results.len());
```

### Refactoring Analysis

```rust
use vtcode::tree_sitter::refactoring::RefactoringEngine;

// Analyze refactoring opportunities
let mut engine = RefactoringEngine::new();
let operations = engine.analyze_refactoring_options(&symbol, &syntax_tree);

// Display available refactorings
for operation in &operations {
    println!("Available: {}", operation.description);
    println!("Preview:\n{}", engine.generate_preview(operation));
}
```

## Integration with Existing Tools

### Enhanced Analyze Command

The tree-sitter integration enhances the existing `analyze` command:

```bash
# Basic analysis
cargo run -- analyze

# Deep analysis with tree-sitter (includes code metrics, symbol extraction, complexity analysis)
cargo run -- analyze --depth deep

# JSON output for programmatic access
cargo run -- analyze --format json
```

### Integration Points

1. **Tool Registry Enhancement**
   - Tree-sitter analysis integrated into existing tool framework
   - Results available to LLM for enhanced decision making

2. **Agent Decision Making**
   - Code analysis results inform agent decisions
   - Complexity metrics guide tool selection
   - Symbol information enhances context understanding

3. **Error Recovery**
   - Syntax analysis helps identify error patterns
   - Code structure understanding improves recovery strategies

## Performance Considerations

### Memory Usage

- Tree-sitter parsers are memory-efficient
- Syntax trees cached for repeated analysis
- Incremental parsing for large codebases

### Analysis Speed

- Initial parsing: ~1-5ms per 1000 lines of code
- Symbol extraction: ~0.5-2ms per file
- Complexity analysis: ~0.1-0.5ms per file

### Caching Strategy

- Parsed trees cached by file path and modification time
- Symbol information cached for navigation
- Analysis results cached for repeated queries

## Configuration

### Language-Specific Settings

```toml
[tree_sitter]
# Enable/disable specific languages
rust_enabled = true
python_enabled = true
javascript_enabled = true

# Analysis depth settings
max_file_size_kb = 1024
analysis_timeout_ms = 5000

# Caching configuration
cache_enabled = true
cache_max_size_mb = 100
cache_ttl_seconds = 3600
```

### Performance Tuning

```toml
[tree_sitter.performance]
# Parser pool size
parser_pool_size = 4

# Analysis concurrency
max_concurrent_analysis = 8

# Memory limits
max_tree_size_mb = 50
max_cache_size_mb = 200
```

## Extending Language Support

### Adding a New Language

1. **Add dependency** in `Cargo.toml`:

```toml
tree-sitter-your-language = "0.21"
```

2. **Extend LanguageSupport enum**:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, Eq, Hash, PartialEq)]
pub enum LanguageSupport {
    // ... existing languages
    YourLanguage,
}
```

3. **Add language queries** in `languages.rs`:

```rust
impl LanguageQueries {
    fn your_language_queries() -> Self {
        Self {
            functions_query: r#"
                (function_definition
                    name: (identifier) @function.name
                    parameters: (parameter_list) @function.parameters
                    body: (block) @function.body) @function.def
            "#.to_string(),
            // ... other queries
        }
    }
}
```

4. **Update language detection** in `analyzer.rs`:

```rust
pub fn detect_language_from_path(&self, path: P) -> Result<LanguageSupport> {
    let extension = path.extension()?.to_str()?;
    match extension {
        "yl" => Ok(LanguageSupport::YourLanguage),
        // ... other extensions
    }
}
```

## Best Practices

### Analysis Workflow

1. **Parse once, analyze many times** - Cache parsed trees
2. **Use appropriate analysis depth** - Basic vs. deep analysis
3. **Filter by language** - Only analyze relevant files
4. **Handle errors gracefully** - Not all files will parse successfully

### Memory Management

1. **Limit file sizes** - Skip very large files
2. **Clear caches periodically** - Prevent memory leaks
3. **Use streaming for large codebases** - Process files incrementally

### Performance Optimization

1. **Parallel analysis** - Use multiple threads for large codebases
2. **Incremental updates** - Only re-analyze changed files
3. **Selective analysis** - Focus on relevant parts of the codebase

## Troubleshooting

### Common Issues

#### Parser Not Found

```rust
// Error: "Unsupported language: CustomLang"
```

**Solution**: Ensure the language parser is properly integrated and the enum is updated.

#### Memory Issues

```rust
// Error: "Out of memory during parsing"
```

**Solution**: Implement file size limits and memory management.

#### Slow Analysis

```rust
// Analysis taking too long
```

**Solution**: Use basic analysis mode, implement timeouts, or optimize queries.

### Debugging

Enable debug logging to see tree-sitter operations:

```bash
RUST_LOG=tree_sitter=debug cargo run -- analyze --depth deep
```

## Future Enhancements

### Planned Features

- **Incremental parsing** for real-time analysis
- **Language server protocol** integration
- **Research-preview refactoring** with AI assistance
- **Code generation** from analysis results
- **Cross-language analysis** for polyglot projects

### Research Areas

- **Machine learning integration** for better analysis
- **Graph-based code representation** for complex relationships
- **Semantic analysis** beyond syntax
- **Performance benchmarking** against other tools

## Contributing

### Adding Language Support

1. Follow the extension guide above
2. Add comprehensive tests
3. Update documentation
4. Ensure performance benchmarks pass

### Improving Analysis

1. Profile current performance
2. Identify bottlenecks
3. Implement optimizations
4. Add comprehensive tests

### Testing

```bash
# Run tree-sitter specific tests
cargo test tree_sitter

# Run integration tests
cargo test tree_sitter_integration

# Run performance benchmarks
cargo bench tree_sitter
```

---

## **SWE-bench Impact**

This tree-sitter integration directly addresses key requirements for SWE-bench optimization:

- **Research-preview Code Understanding**: Syntax-aware parsing enables better code comprehension
- **Precise Navigation**: Accurate symbol location and relationship analysis
- **Intelligent Analysis**: Complexity and quality metrics guide decision making
- **Multi-language Support**: Essential for diverse software ecosystems

By providing deep code understanding capabilities, tree-sitter integration positions vtcode to achieve competitive performance on SWE-bench Verified, following Anthropic's breakthrough engineering approach!
