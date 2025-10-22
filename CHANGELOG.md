# Changelog - vtcode

All notable changes to vtcode will be documented in this file.

## [Unreleased] - Latest Improvements

### Extracted crates release preparation

- **vtcode-commons 0.1.0** – marks the shared workspace path/telemetry traits crate ready for publishing with repository and
  documentation metadata in `Cargo.toml`.
- **vtcode-markdown-store 0.1.0** – aligns the markdown-backed storage crate with the initial release version and links to the
  public documentation.
- **vtcode-indexer 0.1.0** – retags the workspace-friendly indexer for its first standalone release and records the docs.rs URL
  for consumers.
- **vtcode-bash-runner 0.1.0** – updates the shell execution helper crate to the shared release version, adds licensing
  metadata, and points to hosted documentation.
- **vtcode-exec-events 0.1.0** – finalizes the telemetry schema crate for release with docs.rs metadata alongside the version
  alignment.

### **Major Enhancements - Context Engineering & Attention Management** (Phase 1 & 2)

#### Phase 1: Enhanced System Prompts

- **Explicit Response Framework**: All system prompts now include a clear 5-step framework
  1. Assess the situation - Understand what the user needs
  2. Gather context efficiently - Use search tools before reading files
  3. Make precise changes - Prefer targeted edits over rewrites
  4. Verify outcomes - Test changes appropriately  
  5. Confirm completion - Summarize and verify satisfaction
- **Enhanced Guidelines**: More specific guidance on tool selection, code style preservation, and handling destructive operations
- **Multi-Turn Coherence**: Explicit guidance on building context across conversation turns
- **Token Efficiency**: Maintained concise prompts (~280 tokens) while adding structure

**System Prompt Improvements:**
- Default prompt: Added explicit framework, guidelines, and context management strategies
- Lightweight prompt: Added minimal 4-step approach for quick tasks
- Specialized prompt: Added tool selection strategy by phase, advanced guidelines, and multi-turn coherence

#### Phase 2: Dynamic Context Curation

- **New Module**: `context_curator.rs` - Implements iterative per-turn context selection based on Anthropic's principles
- **Conversation Phase Detection**: Automatically detects phase (Exploration, Implementation, Validation, Debugging, Unknown)
- **Phase-Aware Tool Selection**: Dynamically selects relevant tools based on current conversation phase
- **Priority-Based Context Selection**:
  1. Recent messages (always included, configurable)
  2. Active work context (files being modified)
  3. Decision ledger summary (compact)
  4. Recent errors and resolutions
  5. Relevant tools (phase-aware)
- **Automatic Compression**: Compresses context when budget is exceeded while preserving priority items
- **Configurable Curation**: Full control via `[context.curation]` configuration

**Key Features:**
- Tracks active files and file summaries
- Maintains recent error context for debugging
- Selects optimal tools based on conversation phase
- Respects token budget constraints
- Integrates with TokenBudgetManager and DecisionTracker

**API:**
```rust
let curator = ContextCurator::new(config, token_budget, decision_ledger);
curator.mark_file_active("src/main.rs".to_string());
curator.add_error(ErrorContext { ... });
let curated = curator.curate_context(&messages, &tools).await?;
```

**Configuration:**
```toml
[context.curation]
enabled = true
max_tokens_per_turn = 100000
preserve_recent_messages = 5
max_tool_descriptions = 10
include_ledger = true
ledger_max_entries = 12
include_recent_errors = true
max_recent_errors = 3
```

#### Token Budget Tracking & Attention Management

- **New Module**: `token_budget.rs` - Real-time token budget tracking using Hugging Face `tokenizers`
- **Component-Level Tracking**: Monitor token usage by category (system prompt, messages, tool results, decision ledger)
- **Configurable Thresholds**: Warning at 75% and compaction trigger at 85% (customizable via `vtcode.toml`)
- **Model-Specific Tokenizers**: Support for GPT, Claude, and other models for accurate counting
- **Automatic Deduction**: Track token removal during context cleanup and compaction
- **Budget Reports**: Generate detailed token usage reports by component
- **Performance Optimized**: ~10μs per message using Rust-native Hugging Face `tokenizers`
- **New Method**: `remaining_tokens()` - Get remaining tokens in budget for context curation decisions

**Configuration:**
```toml
[context.token_budget]
enabled = true
model = "gpt-4o-mini"
warning_threshold = 0.75
compaction_threshold = 0.85
detailed_tracking = false
```

#### Optimized System Prompts & Tool Descriptions

- **67-82% Token Reduction**: System prompts streamlined from ~600 tokens to ~200 tokens
- **80% Tool Description Efficiency**: Average tool description reduced from ~400 to ~80 tokens
- **"Right Altitude" Principles**: Concise, actionable guidance over verbose instructions
- **Progressive Disclosure**: Emphasize search-first approach with `grep_file` and `ast_grep_search`
- **Clear Tool Purposes**: Eliminated capability overlap in tool descriptions
- **Token Management Guidance**: Built-in advice for efficient context usage (e.g., `max_results` parameters)

**System Prompt Improvements:**
- Removed verbose explanations and redundant information
- Focused on core principles and actionable strategies
- Added explicit context strategy guidance
- Emphasized metadata-first, content-second approach

**Tool Description Improvements:**
- Clear, unambiguous purposes with minimal overlap
- Token efficiency guidance (e.g., `max_results` limits)
- Auto-chunking behavior documented
- Metadata-first approach emphasized

#### Context Engineering Documentation

- **New Documentation**: `docs/context_engineering.md` - Comprehensive guide to context management
- **Implementation Summary**: `docs/context_engineering_implementation.md` - Technical details
- **Best Practices**: User and developer guidelines for efficient context usage
- **Configuration Examples**: Complete examples for token budget and context management
- **Performance Metrics**: Token efficiency improvements documented
- **References**: Links to Anthropic research and related resources

#### Bug Fixes

- **Fixed MCP Server Initialization**: Removed premature `cleanup_dead_providers()` call that caused `BrokenPipeError` during initialization
- **MCP Process Management**: Improved connection lifecycle management to prevent pipe closure issues

#### Dependencies

- **Added**: `tokenizers = "0.15"` for accurate token counting
- **Updated**: Cargo.lock with new dependencies

#### Release Automation

- **Cargo Release Integration**: Adopted `cargo release` with a shared workspace configuration (`release.toml`) and updated `scripts/release.sh` to drive changelog-powered GitHub releases, coordinated crates.io publishing, and npm version synchronization.

### **Major Enhancements - Anthropic-Inspired Architecture**

#### Decision Transparency System

- **New Module**: `decision_tracker.rs` - Complete audit trail of all agent decisions
- **Real-time Tracking**: Every action logged with reasoning and confidence scores
- **Transparency Reports**: Live decision summaries and session statistics
- **Confidence Scoring**: Quality assessment for all agent actions
- **Context Preservation**: Full conversation context maintained across decisions

#### Error Recovery & Resilience

- **New Module**: `error_recovery.rs` - Intelligent error handling system
- **Pattern Detection**: Automatic identification of recurring errors
- **Context Preservation**: Never lose important information during failures
- **Recovery Strategies**: Multiple approaches for handling errors gracefully
- **Error Statistics**: Comprehensive analysis of error patterns and recovery rates

#### Conversation Summarization

- **New Module**: `conversation_summarizer.rs` - Automatic conversation compression
- **Intelligent Summaries**: Key decisions, completed tasks, and error patterns
- **Long Session Support**: Automatic triggers when conversations exceed thresholds
- **Confidence Scoring**: Quality assessment for summary reliability
- **Context Efficiency**: Maintain useful context without hitting limits

### **Tool Design Improvements**

#### Enhanced Tool Documentation

- **Comprehensive Specifications**: Extensive tool descriptions with examples and error cases
- **Error-Proofing**: Anticipate and prevent common model misunderstandings
- **Clear Usage Guidelines**: Detailed instructions for each tool parameter
- **Debugging Support**: Specific guidance for troubleshooting tool failures

#### Improved System Instruction

- **Model-Driven Control**: Give maximum autonomy to the language model
- **Thorough Reasoning**: Encourage deep thinking for complex problems
- **Flexible Methodology**: Adaptable problem-solving approaches
- **Quality First**: Emphasize correctness over speed

### **Release Automation**

- **Coordinated Version Bumps**: `scripts/release.sh` now prompts maintainers to bump the `vtagent-core` crate alongside the main binary, keeping release metadata synchronized.

### **Transparency & Observability**

#### Verbose Mode Enhancements

- **Real-time Decision Tracking**: See exactly why each action is taken
- **Error Recovery Monitoring**: Observe intelligent error handling
- **Conversation Summarization Alerts**: Automatic notifications for long sessions
- **Session Statistics**: Comprehensive metrics and pattern analysis
- **Pattern Detection**: Automatic identification of recurring issues

#### Session Reporting

- **Final Transparency Reports**: Complete session summaries with success metrics
- **Error Recovery Statistics**: Analysis of error patterns and recovery rates
- **Decision Quality Metrics**: Confidence scores and decision success rates
- **Context Usage Monitoring**: Automatic warnings for approaching limits

### **Configuration System Improvements**

#### Two-Way Configuration Synchronization

- **Smart Config Generation**: `vtcode config` now reads existing `vtcode.toml` and preserves customizations
- **Complete Template Generation**: Ensures all configuration sections are present, even missing ones
- **Bidirectional Sync**: Generated configs always match your actual configuration state
- **Fallback Safety**: Uses system defaults when no configuration file exists
- **TOML Serialization**: Replaced hardcoded templates with proper TOML generation

## [Previous Versions]

### v0.1.0 - Initial Release

- Basic agent architecture with Gemini integration
- Core file system tools (list_files, read_file, write_file, edit_file)
- Interactive chat and specialized workflows
- Workspace safety and path validation
- Comprehensive logging and debugging support

## **Performance & Reliability**

### SWE-bench Inspired Improvements

- **49% Target Achievement**: Architecture designed following Anthropic's breakthrough approach
- **Error-Proofed Tools**: Extensive validation and error handling
- **Context Engineering**: Research-preview conversation management techniques
- **Model Empowerment**: Maximum control given to language models

### Reliability Enhancements

- **Context Preservation**: Never lose important information during failures
- **Recovery Strategies**: Multiple approaches for error handling
- **Pattern Detection**: Automatic identification of recurring issues
- **Comprehensive Logging**: Full audit trail of all agent actions

## **Technical Improvements**

### Architecture Refactoring

- **Modular Design**: Separate modules for transparency, error recovery, and summarization
- **Clean Interfaces**: Well-defined APIs between components
- **Performance Optimization**: Efficient data structures and algorithms
- **Error Handling**: Comprehensive error management throughout

### Code Quality

- **Documentation**: Extensive inline documentation and examples
- **Type Safety**: Strong typing with comprehensive error handling
- **Testing**: Unit tests for core functionality
- **Linting**: Clean, well-formatted code following Rust best practices

## **Key Features Summary**

### New Capabilities

1. **Complete Decision Transparency** - Every action tracked and explained
2. **Intelligent Error Recovery** - Learn from mistakes and adapt strategies
3. **Automatic Conversation Summarization** - Handle long sessions efficiently
4. **Confidence Scoring** - Quality assessment for all agent actions
5. **Pattern Detection** - Identify and address recurring issues

### Enhanced User Experience

1. **Verbose Mode Overhaul** - Rich transparency and debugging information
2. **Better Error Messages** - Clear, actionable feedback for all failures
3. **Session Insights** - Comprehensive statistics and recommendations
4. **Improved Tool Reliability** - Error-proofed design prevents common issues
5. **Context Management** - Intelligent handling of conversation limits

## **Future Roadmap**

### Planned Enhancements

- **Multi-file Operations**: Batch processing capabilities
- **Project Templates**: Predefined scaffolds for common projects
- **Integration APIs**: REST endpoints for external integration
- **Research-preview Context Compression**: More sophisticated summarization algorithms

### Research Areas

- **Multi-modal Support**: Images, diagrams, and audio processing
- **Collaborative Workflows**: Enhanced human-agent teaming
- **Domain Specialization**: Industry-specific optimizations
- **Performance Benchmarking**: SWE-bench style evaluation capabilities

## **Contributing**

### Development Guidelines

- **Feature Branches**: Create feature branches for new capabilities
- **Comprehensive Testing**: Include tests for all new functionality
- **Documentation Updates**: Update README, BUILD.md, and this CHANGELOG
- **Code Standards**: Follow established Rust idioms and best practices

### Areas of Interest

- **Tool Enhancements**: Additional tools for specific use cases
- **Workflow Patterns**: New specialized workflows and patterns
- **Performance Optimization**: Further improvements for complex tasks
- **Documentation**: Tutorials, examples, and user guides

---

## **Related Breakthroughs**

This release incorporates insights from Anthropic's engineering approach that achieved **49% on SWE-bench Verified**, including:

- **Minimal Scaffolding**: Give maximum control to language models
- **Error-Proofed Tools**: Extensive documentation and validation
- **Thorough Reasoning**: Encourage deep thinking for complex problems
- **Context Preservation**: Never lose important information during failures
- **Decision Transparency**: Complete audit trail of agent actions

These improvements position vtcode as a state-of-the-art coding assistant with exceptional transparency, reliability, and performance on complex software engineering tasks.
