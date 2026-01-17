# Reducing Latency with Claude

Latency refers to the time it takes for a model to process a prompt and generate an output. In the context of **VT Code**, where the agent often performs multiple tool calls and reasoning steps, minimizing latency is critical for a fluid, high-performance developer experience.

<Note>
**The Golden Rule**: Always engineer for correctness first. Once your prompt achieves the desired performance, apply latency reduction strategies. Optimization should never come at the expense of reliably solving the user's task.
</Note>

---

## Technical Latency Metrics

When benchmarking your workflows in VT Code, focus on these key metrics:

- **Time to First Token (TTFT)**: The delay between sending a request and receiving the first byte of the response. This is the primary driver of "perceived snappiness." Use **Streaming** to minimize this.
- **Tokens Per Second (TPS)**: The speed at which the model generates content. This depends primarily on the model's architecture (e.g., Haiku vs. Opus).
- **Queue Time**: The time spent waiting for infrastructure availability. Global regions and usage tiers affect this.
- **Cache Hit Latency**: The significantly reduced processing time when a prompt hits a [Prompt Cache breakpoint](https://docs.anthropic.com/en/docs/build-with-claude/prompt-caching).

---

## Strategies for Reducing Latency

### 1. Model Selection: The Right Tool for the Job

Anthropic provides a spectrum of models balancing speed and intelligence. For agentic loops in VT Code, we recommend:

| Model | Use Case | Latency Profile |
| :--- | :--- | :--- |
| **Claude 4.5 Haiku** | Fast code edits, unit tests, simple refactors. | Ultra-Fast (Near-instant) |
| **Claude 4.5 Sonnet** | General coding, complex architecture, deep debugging. | Balanced (Standard) |
| **Claude 4.5 Opus** | High-stakes logic, new project initialization. | Higher (Reasoning-heavy) |

```python
# VT Code automatically selects optimized models based on task complexity,
# but you can force Haiku for speed-critical automation:
message = client.messages.create(
    model="claude-haiku-4-5",
    max_tokens=150,
    messages=[{"role": "user", "content": "Refactor this function to be O(n)."}]
)
```

### 2. Leverage Prompt Caching (Beta)

Prompt caching is the single most effective way to reduce both latency and cost for long-running sessions. By caching your system prompt, tool definitions, and previous conversation history, Claude skips the redundant processing.

- **How it works**: Mark large, static blocks (like the `AGENTS.md` instructions) with `cache_control`.
- **Latency impact**: Reductions of **50-80%** for large prompts (>10k tokens).

*In VT Code, prompt caching is enabled by default for supported Anthropic models.*

### 3. Optimize Input and Output length

Every token processed or generated adds to the total wall-clock time.

- **Concise Role Prompting**: Instead of "You are a helpful assistant who knows everything about Rust...", use "You are a senior Rust systems engineer."
- **Curbing Chattiness**: Instruct Claude to skip conversational filler. Avoid "Sure, I can help with that. Here is the code:". Use "Provide code only, no preamble."
- **Paragraph/Sentence Limits**: LLMs struggle with exact word counts. Use structural constraints instead: *"Summarize in exactly 2 bullet points."*
- **Temperature Tuning**: For deterministic tasks, a lower `temperature` (e.g., `0.0` or `0.2`) often results in more direct, shorter answers compared to higher randomness.

### 4. Implement Streaming

Streaming doesn't reduce the *total* latency, but it drastically improves the *perceived* latency. VT Code uses streaming natively in the TUI to show you Claude's thought process (via `<thinking>` blocks) and code changes in real-time.

```bash
# Verify streaming is active in VT Code logs
vtcode --debug | grep "stream_event"
```

### 5. Efficient Tool Usage

When using tools (like `grep_search` or `find_by_name`), focus the context.
- **Narrow the Scope**: Instead of searching the whole `/src` directory, specify a subdirectory.
- **Parallel Tooling**: Claude 3.5/4.5 can call multiple tools in a single turn. VT Code executes these in parallel to save time.

---

## Latency Checklist for VT Code Agents

- [ ] Is the system prompt cached?
- [ ] Am I using Haiku for simple file-system operations?
- [ ] Have I restricted `max_tokens` for short-form responses?
- [ ] Is streaming enabled in my interface?
- [ ] Are tools being called with specific, narrow paths?
