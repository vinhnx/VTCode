# Updating Model Catalogs

This project now derives OpenRouter model metadata from `docs/models.json` during the build. The `vtcode-core` crate ships a build script (`build.rs`) that reads the provider entry, validates the per-model `vtcode` block, and emits generated Rust modules with:

- Constant identifiers under `config::constants::models::openrouter` (including vendor-scoped groupings).
- Compile-time metadata used by `ModelId` for parsing, stringification, and capability checks.
- Alias constants such as `OPENROUTER_X_AI_GROK_CODE_FAST_1` for backwards compatibility.

## Adding or Updating OpenRouter Models

1. **Edit `docs/models.json`:**
   - Add or update the OpenRouter model entry.
   - Provide a `vtcode` object with the following fields:
     - `variant`: `ModelId` variant name (e.g., `OpenRouterGrok4`).
     - `constant`: base constant identifier (e.g., `X_AI_GROK_4`).
     - `vendor`: vendor slug used for `vendor::<slug>::MODELS` groupings.
     - `display` and `description`: human-friendly strings for documentation.
     - `efficient`, `top_tier`, `generation`: trait flags reused by `ModelId` helpers.
     - `doc_comment` (optional): overrides the auto-generated doc comment.
2. **Set the provider default** via the `openrouter.default_model` key when necessary. The build script enforces that the value matches one of the declared model IDs.
3. **Regenerate outputs** by running `cargo check -p vtcode-core`. The build script re-creates:
   - `openrouter_constants.rs`
   - `openrouter_aliases.rs`
   - `openrouter_model_variants.rs`
   - `openrouter_metadata.rs`
4. **Format and review** the diff with `cargo fmt`. The generated code is excluded from formatting and is deterministic when the JSON order is stable.

## Verifying Changes

- `cargo check -p vtcode-core` ensures the build script succeeds and the crate compiles with the new metadata.
- Unit tests in `vtcode-core/src/config/models.rs` cover parsing, provider lookup, and capability flags; run `cargo test -p vtcode-core` if the workspace passes.
- When adding new vendor slugs, verify the generated module is referenced through `config::constants::models::openrouter::vendor::<slug>::MODELS`.

## Notes

- The build script fails fast if required `vtcode` fields are missing or duplicated, preventing stale metadata from entering the codebase.
- Downstream code should reference model IDs through the generated constants or the vendor modules to stay aligned with the source JSON.
- Only OpenRouter uses the build-generated metadata today, but the pattern can be extended to other providers by enriching `docs/models.json`.
