# Model Addition Checklist

Complete this checklist when adding a new LLM model to VT Code.

## Pre-Flight

- [ ] Model is officially released/available from provider
- [ ] Have access to model documentation
- [ ] Know context window, capabilities, costs
- [ ] Confirmed tool calling support status
- [ ] Confirmed reasoning support status (if applicable)

## Phase 1: Constants & Metadata (Database Layer)

**Files:** `openai.rs`, `models.json`

- [ ] Added to `SUPPORTED_MODELS` in `vtcode-config/src/constants/models/openai.rs`
- [ ] Added convenience constant (e.g., `GPT_5_4_NANO: &str = "gpt-5.4-nano"`)
- [ ] Updated relevant arrays:
  - [ ] `RESPONSES_API_MODELS` (if applicable)
  - [ ] `REASONING_MODELS` (if applicable)
  - [ ] `SERVICE_TIER_MODELS` (if applicable)
  - [ ] `TOOL_UNAVAILABLE_MODELS` (if NO tool support)
  - [ ] `HARMONY_MODELS` (if OSS model with harmony)
- [ ] Added complete entry to `docs/models.json` with:
  - [ ] `id` field
  - [ ] `name` field (human-readable)
  - [ ] `description` field
  - [ ] `reasoning` boolean
  - [ ] `tool_call` boolean
  - [ ] `modalities.input` array
  - [ ] `modalities.output` array
  - [ ] `context` field (token count)
- [ ] Validated JSON: `python3 -m json.tool docs/models.json > /dev/null`

## Phase 2: Model ID Resolution (Core Layer)

**Files:** `model_id.rs`, `as_str.rs`, `display.rs`, `description.rs`, `parse.rs`, `provider.rs`

### model_id.rs (Enum Definition)
- [ ] Added enum variant in correct provider section (OpenAI, Anthropic, etc.)
- [ ] Added doc comment with model description
- [ ] Used PascalCase naming (e.g., `GPT54Nano`)
- [ ] Variant appears in correct alphabetical position

### as_str.rs (String Mapping)
- [ ] Added match arm: `ModelId::GPT54Nano => models::openai::GPT_5_4_NANO`
- [ ] Constant reference matches defined constant

### display.rs (Human-Readable Name)
- [ ] Added match arm with display name: `"GPT-5.4 Nano"`
- [ ] Name matches `docs/models.json` "name" field

### description.rs (Full Description)
- [ ] Added match arm with description text
- [ ] Description matches `docs/models.json` "description" field

### parse.rs (String → Enum)
- [ ] Added parse rule: `s if s == models::openai::GPT_5_4_NANO => Ok(ModelId::GPT54Nano)`
- [ ] Handles variant correctly

### provider.rs (Provider Assignment)
- [ ] Added to correct provider match block (OpenAI, Anthropic, etc.)
- [ ] Match statement is exhaustive (no missing arms)

## Phase 3: Capabilities & Collections (Runtime Layer)

**Files:** `collection.rs`, `capabilities.rs`

### collection.rs (All Models List)
- [ ] Added to `all_models()` vector
- [ ] Positioned alphabetically within provider section
- [ ] Not duplicated elsewhere

### capabilities.rs (Trait Methods)
- [ ] Added to `generation()` match with version string (e.g., "5.4")
- [ ] Added to `non_reasoning_variant()` if NOT a reasoning model
- [ ] Added to `is_top_tier()` if flagship class (optional)
- [ ] Added to `is_pro_variant()` if pro variant (optional)
- [ ] Added to `is_efficient_variant()` if lightweight/cost-effective (optional)
- [ ] Added to `supports_shell_tool()` if shell execution capable (optional)

## Phase 4: Compilation & Verification

- [ ] Ran: `cargo check --package vtcode-config`
  - [ ] No errors
  - [ ] No warnings
- [ ] Ran: `cargo check --all-targets`
  - [ ] No compilation errors
- [ ] Ran: `cargo clippy --workspace --all-targets -- -D warnings`
  - [ ] No clippy violations

## Phase 5: Functional Testing

- [ ] Model appears in `/model` command help
- [ ] Model can be selected: `vtcode ask --model gpt-5.4-nano "test"`
- [ ] Model ID parses correctly: `"gpt-5.4-nano".parse::<ModelId>()`
- [ ] Model properties correct:
  - [ ] `provider()` returns correct provider
  - [ ] `generation()` returns correct version
  - [ ] `display_name()` returns correct name
  - [ ] `description()` returns correct description
  - [ ] `supports_tool_calls()` returns correct value
  - [ ] `is_reasoning_variant()` returns correct value

## Phase 6: Documentation

- [ ] Added to `docs/providers/PROVIDER_GUIDES.md` (if new provider)
- [ ] Added to relevant architecture docs
- [ ] Updated CHANGELOG.md with model addition
- [ ] Updated any example configurations that reference models

## Phase 7: Final Review

- [ ] All 10 files updated
- [ ] No duplicated entries
- [ ] Consistent naming across all files
- [ ] No hardcoded strings (use constants everywhere)
- [ ] JSON validates cleanly
- [ ] All tests pass
- [ ] Code review complete

## Quick Reference: Files to Update

| File | Update Type | Lines of Change |
|------|------------|-----------------|
| openai.rs | Add to array + const | 2 |
| models.json | Add full object | 10-15 |
| model_id.rs | Add enum variant | 2-3 |
| as_str.rs | Add match arm | 1 |
| display.rs | Add match arm | 1 |
| description.rs | Add match arm | 1-2 |
| parse.rs | Add match arm | 1 |
| provider.rs | Add to match | 1 |
| collection.rs | Add to vector | 1 |
| capabilities.rs | Add to match arms | 1-3 |

**Total: ~10 files, ~30-50 lines of code**

## Time Estimate

- First time: 15-20 minutes (following guide)
- Subsequent times: 5-10 minutes (familiar with flow)
- With script: 5 minutes

## Testing Template

```rust
#[cfg(test)]
mod model_tests {
    use crate::models::{ModelId, Provider};
    
    #[test]
    fn test_gpt_5_4_nano() {
        let model = "gpt-5.4-nano".parse::<ModelId>().expect("parse failed");
        assert_eq!(model, ModelId::GPT54Nano);
        assert_eq!(model.provider(), Provider::OpenAI);
        assert_eq!(model.display_name(), "GPT-5.4 Nano");
        assert_eq!(model.generation(), "5.4");
        assert!(model.supports_tool_calls());
        assert!(!model.is_reasoning_variant());
    }
}
```

## Common Issues & Fixes

### Error: Pattern not covered in `provider.rs`
- **Cause:** Added enum variant but forgot to add to provider match
- **Fix:** Add new variant to appropriate provider match arm

### Error: "Unknown model" when parsing
- **Cause:** Forgot parse rule or constant name mismatch
- **Fix:** Check parse.rs and ensure constant matches openai.rs

### JSON validation fails
- **Cause:** Missing quotes, trailing comma, or structural error
- **Fix:** Use `python3 -m json.tool` to find exact issue

### Model doesn't appear in `/model` help
- **Cause:** Forgot to add to collection.rs all_models()
- **Fix:** Add to all_models() vector

## Automation

To automate model addition:

```bash
./scripts/add_model.sh
```

This generates a summary of all required changes. Apply manually or integrate with editor snippets for faster workflow.

## Related Documentation

- Full guide: `docs/development/ADDING_MODELS.md`
- Provider setup: `docs/providers/PROVIDER_GUIDES.md`
- Config reference: `docs/config/CONFIGURATION_PRECEDENCE.md`
- Model metadata: `docs/models.json`
