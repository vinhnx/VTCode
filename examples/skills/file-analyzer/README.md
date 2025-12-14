# File Analyzer Skill

A VTCode skill that analyzes code files and provides insights about structure, complexity, and quality.

## Overview

This skill analyzes source code files and provides detailed reports including:
- File structure and organization
- Code complexity metrics
- Dependency analysis
- Quality indicators
- Suggestions for improvement

## Features

- Multi-language support (Rust, Python, JavaScript, TypeScript, Go)
- Complexity analysis (cyclomatic complexity, nesting depth)
- Dependency extraction and analysis
- Code quality metrics
- Detailed reporting in JSON format
- Streaming analysis for large files

## Installation

1. Ensure Python 3.6+ is installed on your system
2. Install required Python packages: `pip install tree-sitter tree-sitter-python tree-sitter-javascript tree-sitter-typescript tree-sitter-go tree-sitter-rust`
3. Place this directory in your VTCode skills directory
4. Make sure `analyze.py` is executable

## Configuration

Edit `tool.json` to configure:
- Analysis depth and complexity thresholds
- Output format preferences
- Language-specific settings
- Performance parameters

## Usage

Once installed, you can use this skill through VTCode's interface:

```
Analyze the file src/main.rs and provide complexity metrics
```

## Supported Languages

- Rust (.rs)
- Python (.py)
- JavaScript (.js)
- TypeScript (.ts, .tsx)
- Go (.go)

## Analysis Metrics

The skill provides:
- Lines of code (LOC)
- Cyclomatic complexity
- Function/method count
- Dependency count
- Code duplication indicators
- Style violations (basic)
- Performance suggestions