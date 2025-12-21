# Quick Start: Pi-Inspired Minimal Mode

**Want 87% less system prompt overhead? Here's how.**

## 1. Edit Your Config

Open `vtcode.toml` and add:

```toml
[agent]
system_prompt_mode = "minimal"
```

## 2. Restart VT Code

```bash
cargo run
```

## 3. Verify

Look for debug log (optional):
```bash
RUST_LOG=vtcode_core::prompts=debug cargo run
```

Should see:
```
DEBUG Selected system prompt mode: mode=minimal base_tokens_approx=175
```

## That's It!

You're now using **~700 tokens** instead of **~6,500 tokens** for your system prompt.

---

## Token Comparison

| Before (Default) | After (Minimal) | Savings |
|-----------------|-----------------|---------|
| 6,500 tokens | 700 tokens | **87%** ↓ |
| $13/1M prompts | $1.40/1M prompts | **89%** ↓ |

*(Based on Claude Sonnet 4.5 pricing)*

---

## What Changes?

### You Get:
- ✅ **Faster responses** (less input to process)
- ✅ **Lower costs** (fewer prompt tokens)
- ✅ **More context** (+6.3K tokens for code)
- ✅ **Same capability** (on frontier models)

### You Lose:
- ❌ Verbose error recovery guidance
- ❌ Explicit loop prevention examples
- ❌ Hand-holding for edge cases

### Bottom Line:
Modern models like Claude Sonnet 4.5, GPT-5.1, and Gemini 2.0 **don't need 6,000 tokens** of instructions to code well.

---

## All Modes

```toml
# Choose one:
system_prompt_mode = "minimal"      # ~700 tokens (87% savings)
system_prompt_mode = "lightweight"  # ~1.8K tokens (67% savings)
system_prompt_mode = "default"      # ~6.5K tokens (current)
system_prompt_mode = "specialized"  # ~7K tokens (complex tasks)
```

---

## Troubleshooting

### "Agent seems less helpful"
→ Switch to `lightweight` or `default`

### "No token savings visible"
→ Check you edited `vtcode.toml` correctly
→ Restart vtcode after changes

### "Which mode should I use?"
→ **Minimal**: If you're experienced, using Claude Sonnet 4.5+
→ **Default**: If you're new or prefer guidance
→ **Specialized**: If doing large refactors

---

## Full Docs

See `docs/features/SYSTEM_PROMPT_MODES.md` for complete guide.

---

**Philosophy**: Let users choose their complexity. Pi-coding-agent proves minimalism works. VT Code gives you the choice.
