---
name: code-review-skill
description: Performs comprehensive code reviews focusing on security, performance, and maintainability
version: 1.0.0
author: VTCode Team
license: MIT
model: inherit
mode: false
allowed-tools:
    - Read
    - Grep
disable-model-invocation: false
when-to-use: "Deep Rust code reviews for security/performance"
requires-container: false
disallow-container: true
---

# Code Review Skill

## Instructions

-   Tools: use **Read** for context and **Grep** for pattern searches. Do not write or execute commands.

1. Analyze the provided code for:

    - Security vulnerabilities
    - Performance bottlenecks
    - Code clarity and readability
    - Test coverage gaps
    - Architecture compliance
    - Error handling completeness

2. Provide specific, actionable feedback with:

    - Line-by-line analysis of critical issues
    - Suggestions for improvement
    - Best practice recommendations
    - Performance optimization opportunities

3. Focus on:
    - Rust-specific patterns and idioms
    - Memory safety and lifetime management
    - Concurrency and thread safety
    - Error handling patterns
    - Code organization and modularity

## Examples

-   "Review this function for security vulnerabilities:"
-   "Analyze this module for performance improvements:"
-   "Check this code for Rust best practices:"

## Output Format

Provide a structured review with:

1. **Summary**: High-level overview of findings
2. **Critical Issues**: Security vulnerabilities or major bugs
3. **Improvements**: Performance and readability suggestions
4. **Best Practices**: Compliance with Rust conventions
5. **Next Steps**: Actionable recommendations
