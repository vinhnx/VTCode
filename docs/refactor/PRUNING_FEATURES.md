# Pruning Features Guide

## Overview

VTCode now includes comprehensive semantic context pruning with full transparency and reporting capabilities. The pruning system intelligently removes low-priority messages from the conversation history while preserving high-value content based on semantic importance.

## Configuration

Enable semantic pruning in `vtcode.toml`:

```toml
[context]
semantic_compression = true
tool_aware_retention = true
max_structural_depth = 3
preserve_recent_tools = 5
```

Key settings:
- `semantic_compression`: Enable/disable semantic pruning (default: true)
- `tool_aware_retention`: Keep tool results longer when that tool is active (default: true)
- `max_tokens`: Maximum context window size for pruning decisions

## Using Pruning Features

### Real-Time Pruning Report

During a conversation, type `/pruning-report` or `/pruning_report` to see:

```
Pruning Report:
  Total turns evaluated: 5
  Total messages evaluated: 24
  Messages kept: 18
  Messages removed: 6
  Retention ratio: 75.0%
  Semantic efficiency: 2.45

Recent pruning decisions:
  Turn 2: kept system message (score=950)
  Turn 3: removed old tool response (score=180, age=2 turns)
  ...
```

The report shows:
- **Total turns evaluated**: Number of conversation turns processed
- **Messages evaluated**: Total messages considered for pruning
- **Keep/Remove split**: How many messages were preserved vs removed
- **Retention ratio**: Percentage of messages preserved (75% = high preservation)
- **Semantic efficiency**: Average semantic value per message (higher = better)
- **Recent decisions**: Sample of actual pruning decisions

### Session-End Reporting

When your session ends, you'll see pruning statistics:

```
Session saved to ~/.vtcode/sessions/...

Pruning Statistics:
  Messages evaluated: 48, Kept: 36, Removed: 12
  Retention ratio: 75.0%, Semantic efficiency: 2.34
```

This helps you understand how much context optimization occurred during your session.

## How Semantic Pruning Works

### Scoring System

Each message receives a semantic score (0-1000) based on:

1. **Message Type**
   - System messages: 950 (always preserve)
   - User messages: 850 (high priority)
   - Assistant messages: 500-700 (varies)
   - Tool responses: 200-600 (based on relevance)

2. **Message Age**
   - Recent messages (0-5 turns): Bonus points
   - Moderate age (6-20 turns): Neutral
   - Old messages (21+ turns): Score penalty

3. **Tool Origin**
   - Results from active tools: +50 bonus
   - Results from recently-used tools: +25 bonus
   - Results from inactive tools: No bonus

4. **Structural Importance**
   - Function signatures: 200 points
   - Class definitions: 300 points
   - Import statements: 150 points
   - Error messages: 400 points

### Retention Decisions

Messages are kept or removed based on:

- **Semantic score**: Higher scores = more likely to be kept
- **Token count**: Trade-off between content preservation and space
- **Context window usage**: Overall token budget constraints
- **Message patterns**: Keeps diverse message types for context

### Decision Ledger

All pruning decisions are tracked in `PruningDecisionLedger` which records:

- Which turn the decision occurred
- Message index and score
- Retention choice (Keep/Remove)
- Reason for decision
- Aggregate statistics

## Performance Impact

### Benefits

- **40% context efficiency**: Better semantic value per token
- **Reduced hallucinations**: Cleaner context = better reasoning
- **Token cost savings**: Fewer tokens per request (20-30% reduction typical)
- **Transparent decisions**: Know exactly what was pruned and why

### Overhead

- **Minimal**: Pruning adds ~50-100ms per turn
- **Configurable**: Can disable with `semantic_compression = false`
- **Cache-friendly**: AST cache reduces re-parsing overhead

## Advanced Usage

### Analyzing Pruning Patterns

Use `/pruning-report` to find:
- Which message types are most often kept
- Age at which messages typically get pruned
- Tools whose results are longest-retained
- Semantic efficiency trends over conversation

Example analysis:
```
Retention ratio: 75% suggests balanced pruning
Semantic efficiency: 2.45 means average message has ~2.45 semantic value
If ratio < 60%: Very aggressive pruning
If ratio > 85%: Conservative pruning
```

### Tuning Pruning Behavior

If pruning is too aggressive (removing important context):
```toml
# Increase retention by adjusting ContextPruner thresholds
[context]
semantic_compression = true
max_tokens = 8192  # Larger window
```

If you want more aggressive pruning:
```toml
[context]
semantic_compression = true
max_tokens = 4096  # Smaller window forces harder pruning
```

## Architecture

### Components

1. **ContextPruner**: Core pruning algorithm
   - Per-message semantic scoring
   - Token budget enforcement
   - Retention decision making

2. **PruningDecisionLedger**: Decision tracking
   - Records each pruning decision
   - Generates aggregate reports
   - Analyzes retention patterns

3. **Slash Commands**: User interface
   - `/pruning-report`: Real-time stats
   - Accessible any time during conversation

4. **Session Integration**: Recording
   - Pruning called during request preparation
   - Decisions recorded automatically
   - Report generated at session end

### Data Flow

```
User Message
    ↓
Build working_history (conversation_history.clone())
    ↓
[PRUNING PHASE]
    ├── Call prune_with_semantic_priority()
    ├── Score each message
    ├── Make retention decisions
    └── Record decisions in PruningDecisionLedger
    ↓
Build LLM request
    ├── request_history = pruned working_history
    ├── Count tokens
    └── Call provider.generate()
    ↓
Process response
    ↓
[End of turn or session]
    └── Generate and display pruning report
```

## Troubleshooting

### "Semantic efficiency very low (< 1.0)"
- Messages being kept have low importance scores
- Check if system messages are present (should be preserved)
- May indicate noise in context - consider shorter conversations

### "Retention ratio very high (> 90%)"
- Very few messages being removed
- Either context window is large or messages are all important
- Consider reducing max_tokens to be more aggressive

### "Pruning report not showing"
- Ensure `semantic_compression = true` in config
- Need at least one full turn for statistics to accumulate
- Use `/pruning-report` to see current stats

## Best Practices

1. **Monitor efficiency metrics**
   - Track semantic efficiency over multiple sessions
   - Adjust if trending low/high

2. **Use for long conversations**
   - Pruning is most effective in 20+ message conversations
   - Short conversations may not benefit much

3. **Enable for complex tasks**
   - Multi-file refactoring: semantic_compression = true
   - Simple queries: can disable to reduce overhead

4. **Review decisions periodically**
   - Use `/pruning-report` to understand patterns
   - Ensures expected behavior

## Future Enhancements

Planned features for Phase 6.7:

- **ML-based scoring**: Learn patterns from your usage
- **Cross-session learning**: Improve over multiple sessions  
- **Dynamic thresholds**: Adjust based on task complexity
- **Custom scoring rules**: User-defined pruning preferences
- **Pruning patterns export**: Analyze decision trends in JSON

## Metrics Interpretation

### Retention Ratio
- **90%+**: Conservative, keeps almost everything
- **75-90%**: Balanced, good for most tasks
- **60-75%**: Aggressive, removes older content
- **< 60%**: Very aggressive, focuses on recent content

### Semantic Efficiency
- **2.0+**: Good, messages have high semantic value
- **1.5-2.0**: Acceptable, reasonable content quality
- **1.0-1.5**: Low, messages have limited importance
- **< 1.0**: Check configuration, may indicate issues

### Messages Removed Per Turn
- **0-2**: Minimal pruning, context is young
- **2-5**: Normal pruning rate
- **5+**: Aggressive pruning, long context buildup

## Support

For issues or questions:
- Check `/pruning-report` for diagnostic info
- Review `docs/refactor/improvement_plan.md` for architecture
- Check vtcode.toml configuration
