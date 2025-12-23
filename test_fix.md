# Fix Verification for OpenRouter Model Panic

## Problem
The application was panicking with:
```
internal error: entered unreachable code
in vtcode-core/src/config/models.rs:855
```

This occurred because three OpenRouter model variants existed in the `ModelId` enum but did not have corresponding entries in `docs/models.json`:
1. `OpenRouterOpenAIGpt4oSearchPreview`
2. `OpenRouterOpenAIGpt4oMiniSearchPreview`
3. `OpenRouterOpenAIChatgpt4oLatest`

When `display_name()` was called on these models, `openrouter_metadata()` returned `None`, causing the code to hit the `unreachable!()` at line 855.

## Solution
Added the three missing model entries to `docs/models.json` with appropriate metadata including:
- Model IDs: `openai/gpt-4o-search-preview`, `openai/gpt-4o-mini-search-preview`, `openai/chatgpt-4o-latest`
- Variant names matching the enum
- Vendor information
- Display names and descriptions
- Efficiency and tier settings
- Generation markers

## Verification
After rebuilding:
- All 40 OpenRouter model variants now have metadata entries
- The generated `openrouter_metadata.rs` contains all 40 variants
- `cargo test --package vtcode-core --lib config::models::tests` passes (9/10 tests, with 1 unrelated failure)
- The MODEL_OPTIONS initialization will no longer panic

## Files Modified
- `docs/models.json` - Added three missing OpenRouter model entries
