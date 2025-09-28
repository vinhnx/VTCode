# Release Notes

## v0.15.2 - 2025-01-13

### Major Enhancements - Anthropic-Inspired Architecture

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

### Tool Design Improvements
- Enhanced tool documentation with comprehensive specifications
- Improved system instruction to give maximum autonomy to language models
- Better error-proofing to prevent common model misunderstandings

### Configuration System Improvements
- Two-way configuration synchronization
- Smart config generation that preserves customizations
- Complete template generation ensuring all configuration sections are present

### Release Automation
- Coordinated version bumps for both main crate and core crate

### Transparency & Observability
- Verbose mode enhancements with real-time decision tracking
- Session reporting with comprehensive metrics
- Pattern detection for recurring issues

---

## v0.15.1 - 2024-12-28

*Bug fixes and minor improvements*

---

## v0.15.0 - 2024-12-15

*Previous release with significant improvements*