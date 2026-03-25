# Adding New Models to VT Code

This guide documents the complete workflow for adding a new LLM model to VT Code. Follow these steps to ensure all systems are properly configured.

## Overview

Adding a model requires updates in **three layers**:

1. **Constants Layer** - Model strings & metadata
2. **Configuration Layer** - Model palette configuration
3. **Core Layer** - Runtime model resolution & capabilities

## Quick Checklist

- [ ] Add to OpenAI constants (`vtcode-config/src/constants/models/openai.rs`)
- [ ] Add to model metadata (`docs/models.json`)
- [ ] Add enum variant (`vtcode-config/src/models/model_id.rs`)
- [ ] Update `as_str.rs` - string mapping
- [ ] Update `display.rs` - human-readable name
- [ ] Update `description.rs` - model description
- [ ] Update `parse.rs` - string parsing
- [ ] Update `collection.rs` - all_models list
- [ ] Update `capabilities.rs` - generation version
- [ ] Update `provider.rs` - provider assignment
- [ ] Verify with `cargo check --package vtcode-config`

## Detailed Steps

### Step 1: Add to Constants (openai.rs)

**File:** `vtcode-config/src/constants/models/openai.rs`

```rust
// In SUPPORTED_MODELS array
pub const SUPPORTED_MODELS: &[&str] = &[
    // ... existing models
    "gpt-5.4-nano",    // Add here in order
    "gpt-5.4-mini",
];

// Add convenience constant (at bottom)
pub const GPT_5_4_NANO: &str = "gpt-5.4-nano";
pub const GPT_5_4_MINI: &str = "gpt-5.4-mini";
```

**When to update:**

- `SUPPORTED_MODELS` - always, for API availability
- `RESPONSES_API_MODELS` - if supports OpenAI Responses API
- `REASONING_MODELS` - if supports reasoning parameter
- `SERVICE_TIER_MODELS` - if supports service_tier parameter
- `TOOL_UNAVAILABLE_MODELS` - if NO tool calling support
- `HARMONY_MODELS` - if uses harmony tokenization (OSS models only)

### Step 2: Add to Model Metadata (models.json)

**File:** `docs/models.json`

```json
"gpt-5.4-nano": {
  "id": "gpt-5.4-nano",
  "name": "GPT-5.4 Nano",
  "description": "Lightweight variant optimized for speed and cost",
  "reasoning": false,
  "tool_call": true,
  "modalities": {
    "input": ["text"],
    "output": ["text"]
  },
  "context": 100000
}
```

**Fields to set:**

- `id` - matches constant name
- `name` - user-facing display name
- `description` - brief capability summary
- `reasoning` - has reasoning_effort support
- `tool_call` - supports function calling
- `modalities.input` - ["text"] or ["text", "image"] etc
- `modalities.output` - typically ["text"]
- `context` - context window size

Verify JSON: `python3 -m json.tool docs/models.json > /dev/null`

### Step 3: Add Enum Variant (model_id.rs)

**File:** `vtcode-config/src/models/model_id.rs`

Add in the appropriate provider section (OpenAI, Anthropic, etc.):

```rust
/// GPT-5.4 Nano - Lightweight GPT-5.4 variant optimized for speed and cost-efficiency
GPT54Nano,
/// GPT-5.4 Mini - Compact GPT-5.4 variant for cost-effective tasks
GPT54Mini,
```

**Naming convention:** `PascalCase` enum variant, no hyphens.

### Step 4: Update as_str.rs

**File:** `vtcode-config/src/models/model_id/as_str.rs`

Maps enum to constant string:

```rust
ModelId::GPT54Nano => models::openai::GPT_5_4_NANO,
ModelId::GPT54Mini => models::openai::GPT_5_4_MINI,
```

### Step 5: Update display.rs

**File:** `vtcode-config/src/models/model_id/display.rs`

Human-readable name for UI:

```rust
ModelId::GPT54Nano => "GPT-5.4 Nano",
ModelId::GPT54Mini => "GPT-5.4 Mini",
```

### Step 6: Update description.rs

**File:** `vtcode-config/src/models/model_id/description.rs`

Full description for help/info:

```rust
ModelId::GPT54Nano => {
    "Lightweight GPT-5.4 variant optimized for speed and cost-efficiency"
}
ModelId::GPT54Mini => {
    "Compact GPT-5.4 variant for cost-effective tasks with reduced reasoning overhead"
}
```

### Step 7: Update parse.rs

**File:** `vtcode-config/src/models/model_id/parse.rs`

String → Enum parsing:

```rust
s if s == models::openai::GPT_5_4_NANO => Ok(ModelId::GPT54Nano),
s if s == models::openai::GPT_5_4_MINI => Ok(ModelId::GPT54Mini),
```

### Step 8: Update collection.rs

**File:** `vtcode-config/src/models/model_id/collection.rs`

Add to `all_models()` vector (keep alphabetically sorted within provider):

```rust
ModelId::GPT54,
ModelId::GPT54Pro,
ModelId::GPT54Nano,      // Add here
ModelId::GPT54Mini,      // Add here
ModelId::GPT53Codex,
```

### Step 9: Update capabilities.rs

**File:** `vtcode-config/src/models/model_id/capabilities.rs`

Update methods that match on model families:

```rust
// non_reasoning_variant() - if not a reasoning model
ModelId::GPT52 | ModelId::GPT54 | ModelId::GPT54Pro | ModelId::GPT54Nano | ModelId::GPT54Mini | ModelId::GPT5 => {
    Some(ModelId::GPT5Mini)
}

// generation() - version string
ModelId::GPT54 | ModelId::GPT54Pro | ModelId::GPT54Nano | ModelId::GPT54Mini => "5.4",

// is_top_tier() - if flagship class (optional, depends on model positioning)
// is_pro_variant() - if pro/advanced variant (optional)
// is_efficient_variant() - if lightweight/fast variant (optional)
// supports_shell_tool() - if supports shell execution (depends on model class)
```

### Step 10: Update provider.rs

**File:** `vtcode-config/src/models/model_id/provider.rs`

Add to provider match:

```rust
ModelId::GPT5
 | ModelId::GPT52
 | ModelId::GPT52Codex
 | ModelId::GPT54
 | ModelId::GPT54Pro
 | ModelId::GPT54Nano    // Add here
 | ModelId::GPT54Mini    // Add here
 | ModelId::GPT5Mini
 | ModelId::GPT5Nano
 // ... rest
 => Provider::OpenAI,
```

## Verification

After all changes, verify compilation:

```bash
cargo check --package vtcode-config
cargo check --all-targets
cargo clippy --workspace --all-targets -- -D warnings
```

Test model resolution:

```bash
# Verify model is in palette
cargo run -- /model --help | grep -i "gpt-5.4"

# Test direct model selection
cargo run -- ask --model gpt-5.4-nano "test"
```

## Template for Copy-Paste

When adding a new model, use this template:

```
Model Name: gpt-5.4-nano
Enum Name: GPT54Nano
Provider: OpenAI
Generation: 5.4
Context: 100000
Reasoning: false
Tool Call: true
Input: ["text"]

--- Files to Update ---
1. openai.rs - SUPPORTED_MODELS + constant
2. models.json - full metadata
3. model_id.rs - enum variant
4. as_str.rs - ModelId::GPT54Nano => models::openai::GPT_5_4_NANO
5. display.rs - "GPT-5.4 Nano"
6. description.rs - description string
7. parse.rs - s if s == models::openai::GPT_5_4_NANO => Ok(ModelId::GPT54Nano)
8. collection.rs - add to all_models()
9. capabilities.rs - update version + optional trait methods
10. provider.rs - add to OpenAI provider match
```

## Automation Ideas

### Bash Script (Future Enhancement)

Could create `scripts/add_model.sh`:

- Prompt for model details (name, provider, context, etc.)
- Generate code snippets
- Auto-insert into files at proper locations
- Run cargo check

### Build Script (build.rs)

The `build.rs` generates model capabilities from `docs/models.json`. Ensure JSON is valid before running build.

### Testing

Add model to integration test:

```rust
#[test]
fn test_gpt_5_4_nano_parsing() {
    let model = "gpt-5.4-nano".parse::<ModelId>().unwrap();
    assert_eq!(model, ModelId::GPT54Nano);
    assert_eq!(model.provider(), Provider::OpenAI);
    assert_eq!(model.generation(), "5.4");
}
```

## Common Mistakes

x **Don't:**

- Add model only to JSON without enum
- Use hyphens in enum names (`GPT-5-4-Nano`)
- Forget to update `provider.rs` match
- Forget to update `collection.rs` all_models list
- Inconsistent naming across files

v **Do:**

- Keep naming consistent: `gpt-5.4-nano` (const), `GPT54Nano` (enum), `"GPT-5.4 Nano"` (display)
- Update all 10 files in order
- Run `cargo check` after each logical group
- Test with actual model resolution before submitting

## Related Files

- Provider setup: `docs/providers/PROVIDER_GUIDES.md`
- Configuration precedence: `docs/config/CONFIGURATION_PRECEDENCE.md`
- Model examples: `docs/models.json`
- Constants reference: `vtcode-config/src/constants/models/`
