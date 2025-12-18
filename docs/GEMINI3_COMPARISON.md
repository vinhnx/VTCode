# Gemini 3 Models Comparison & Selection Guide

## Model Overview

| Feature | Gemini 3 Pro | Gemini 3 Flash | Gemini 2.5 Pro | Gemini 2.5 Flash |
|---------|--------------|----------------|---|---|
| **Best For** | Complex reasoning, broad knowledge | Fast inference, budget-conscious | Previous generation | Legacy compatibility |
| **Pricing (per 1M tokens)** | $2 input / $12 output | $0.50 input / $3 output | Legacy | Legacy |
| **Context Window** | 1M in / 64K out | 1M in / 64K out | 1M in / 65K out | 1M in / 65K out |
| **Thinking Levels** | 2 (`low`, `high`) | 4 (`minimal`, `low`, `medium`, `high`) | Deprecated | Deprecated |
| **Knowledge Cutoff** | Jan 2025 | Jan 2025 | Older | Older |
| **Status** | Preview | Preview | Stable | Stable |
| **Cost per Task** | Higher | Lower | Higher | Lower |
| **Intelligence Level** | Maximum | Pro-level at Flash cost | Good | Good |

## Thinking Level Capabilities

### Gemini 3 Pro (2 Levels)
```
Low ──────────────────────────────── High
  ↓                                    ↓
 30s                               300s+
Minimal reasoning          Maximum reasoning depth
```

Use cases:
- **Low**: Simple questions, instruction following
- **High** (default): Complex problems, deep analysis, multi-step reasoning

### Gemini 3 Flash (4 Levels) - Extended Control
```
Minimal ── Low ────── Medium ────── High
   ↓       ↓            ↓           ↓
  5s      15s          60s         180s
None   Basic       Balanced      Maximum
```

Use cases:
- **Minimal**: Chat, Q&A, high-throughput, no reasoning needed (~5s)
- **Low**: Simple tasks, fast responses (~15s)
- **Medium**: Balanced reasoning + speed (~60s)  ← *Flash advantage*
- **High**: Deep reasoning (default, ~180s)

**Key Advantage**: Flash's `medium` level provides sweet spot between speed and reasoning, unavailable on Pro.

## Cost Comparison

### Simple Query (1K input tokens)
```
Pro with High thinking:  $2 + thinking_tokens + $12
                        ≈ $4-15 per query

Flash with Medium:       $0.50 + thinking_tokens + $3
                        ≈ $0.75-3 per query
                        
SAVINGS: 75-80% cost reduction
```

### Complex Analysis (10K input tokens)
```
Pro (high thinking):     $20 + $120 = $140+ per analysis

Flash (medium):          $5 + $30 = $35+ per analysis

SAVINGS: ~75% cost reduction
TIME:    Comparable latency
QUALITY: Pro-level intelligence on Flash
```

## When to Use Each Model

### Use Gemini 3 Pro When:
✅ Maximum reasoning capability is critical
✅ Task absolutely requires deep multi-turn reasoning
✅ Complex agentic workflows with many tool calls
✅ Customer-facing high-stakes decisions
✅ Budget is not a primary constraint

**Example**:
```rust
model: "gemini-3-pro-preview",
reasoning_effort: Some(ReasoningEffortLevel::High),
// Complex bug analysis, design review, etc.
```

### Use Gemini 3 Flash When:
✅ Cost optimization is important
✅ Response time matters (high-throughput)
✅ Balanced reasoning works for your task
✅ Medium thinking level provides required depth
✅ Can fallback to Pro if needed for edge cases

**Example**:
```rust
model: "gemini-3-flash-preview",
reasoning_effort: Some(ReasoningEffortLevel::Medium),
// Code review, quick analysis, general task solving
```

### Use Gemini 3 Flash (Minimal) When:
✅ No reasoning needed (pure generation/chat)
✅ Maximum throughput priority
✅ Knowledge retrieval, summarization
✅ High-volume API usage

**Example**:
```rust
model: "gemini-3-flash-preview",
reasoning_effort: Some(ReasoningEffortLevel::Minimal),
// Chat, summarization, content generation
```

## Real-World Scenarios

### Scenario 1: Code Review
**Task**: Review PR for bugs, security issues, design patterns

| Model | Thinking | Cost | Time | Recommendation |
|-------|----------|------|------|---|
| Pro | High | $0.12 | 2-3s | ✓ Best |
| Flash | Medium | $0.03 | 2-3s | ✓ Recommended (75% savings) |
| Flash | Low | $0.01 | 0.5s | ⚠️ May miss issues |

**Recommendation**: Use **Flash Medium** - Catches most issues while saving costs

### Scenario 2: Complex Algorithm Analysis
**Task**: Find race condition in multi-threaded C++ code

| Model | Thinking | Cost | Time | Recommendation |
|-------|----------|------|------|---|
| Pro | High | $0.25 | 5-8s | ✓ Recommended |
| Flash | Medium | $0.06 | 4-5s | ~ Similar quality |
| Flash | Low | $0.02 | 1-2s | ✗ Likely misses subtleties |

**Recommendation**: Use **Flash Medium** for 75% cost savings with comparable results

### Scenario 3: Chatbot Response
**Task**: Answer user questions in chat interface

| Model | Thinking | Cost | Time | Recommendation |
|-------|----------|------|------|---|
| Pro | High | $0.15 | 2-3s | ✗ Overkill |
| Flash | Medium | $0.04 | 1-2s | ~ Unnecessary thinking |
| Flash | Minimal | $0.008 | 0.2s | ✓ Recommended |

**Recommendation**: Use **Flash Minimal** - Users expect instant chat responses

### Scenario 4: Document Summarization
**Task**: Summarize 50-page document

| Model | Thinking | Cost | Time | Recommendation |
|-------|----------|------|------|---|
| Pro | High | $0.40 | 3-4s | ✗ Expensive |
| Flash | Low | $0.05 | 1-2s | ✓ Recommended |
| Flash | Minimal | $0.02 | 0.5s | ~ May lose nuance |

**Recommendation**: Use **Flash Low** - Sufficient for factual summarization

## Migration Strategy

### From Gemini 2.5 to Gemini 3
1. **Step 1**: Switch to Gemini 3 Flash with default (high) thinking
   ```rust
   model: "gemini-3-flash-preview"
   // Keep existing reasoning_effort: Some(ReasoningEffortLevel::High)
   ```

2. **Step 2**: Evaluate results and cost
   - If cost is acceptable, done
   - If cost too high, try Medium

3. **Step 3**: If using Medium, measure quality
   - Most tasks: Medium provides sufficient reasoning
   - Complex tasks: Fall back to Pro

4. **Step 4** (Optional): Optimize thinking levels by task
   ```rust
   match task_complexity {
       TaskComplexity::Simple => ReasoningEffortLevel::Minimal,
       TaskComplexity::Normal => ReasoningEffortLevel::Medium,
       TaskComplexity::Complex => ReasoningEffortLevel::High,
   }
   ```

### A/B Testing Recommendation
```rust
// Test both models on sample tasks
let models = vec![
    "gemini-3-flash-preview",    // 75% cost savings
    "gemini-3-pro-preview",      // Baseline
];

for model in models {
    let quality_score = evaluate_output(generate(model));
    let cost = calculate_cost(model);
    println!("Model: {}, Quality: {}, Cost: ${}", model, quality_score, cost);
}
```

## Pricing Examples

### Batch Processing 1000 Documents

#### With Gemini 3 Pro (all high thinking)
```
Input:  1000 docs × 5K tokens × $2/1M = $10
Output: 1000 × 2K tokens × $12/1M = $24
Thinking: ~5000 + tokens per doc = ~$50
Total: ~$84
```

#### With Gemini 3 Flash (medium thinking)
```
Input:  1000 × 5K × $0.50/1M = $2.50
Output: 1000 × 2K × $3/1M = $6
Thinking: ~2000 tokens per doc = ~$12
Total: ~$20.50
```

**SAVINGS**: $63.50 / 1000 = $0.06 per document (76% reduction)

## Decision Tree

```
Task Requires Thinking?
├─ No (chat, retrieval, summarization)
│  └─ Use Flash with Minimal thinking
│
├─ Yes - What's the complexity?
│  ├─ Simple/Medium
│  │  └─ Try Flash with Low or Medium
│  │     └─ Results good?
│  │        ├─ Yes: Keep Flash (save 75%)
│  │        └─ No: Use Pro
│  │
│  └─ Very Complex/Safety-Critical
│     └─ Use Pro with High thinking
```

## Monitoring & Optimization

### Key Metrics
```rust
#[derive(Debug)]
struct ModelMetrics {
    model: String,
    thinking_level: String,
    quality_score: f64,      // Your evaluation metric
    latency_ms: u64,
    cost_per_request: f64,
    tokens_used: u32,
}
```

### Optimization Process
1. Log metrics for all requests (1-week sample)
2. Group by task type and complexity
3. Compare Pro vs Flash results
4. For each task type: identify optimal thinking level
5. Update task routing based on findings

**Expected Outcome**: 60-75% cost reduction while maintaining or improving quality

## See Also
- [Gemini 3 Reference Guide](./GEMINI3_REFERENCE.md)
- [Official Gemini 3 Docs](https://ai.google.dev/gemini-api/docs/gemini-3)
- [Pricing Calculator](https://ai.google.dev/gemini-api/docs/pricing)
