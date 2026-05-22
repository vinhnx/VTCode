# Atlas Cloud Provider Review

## Goal

Enable Atlas Cloud as a minimal, production-usable provider option for VT Code
without introducing a brand-new runtime provider implementation.

## What Changed

- `src/cli/dispatch/commands.rs`
  - Registered `custom_providers` before CLI command dispatch so non-interactive
    flows such as `ask`, `review`, and `benchmark` can resolve Atlas Cloud.
- `docs/providers/atlascloud.md`
  - Added a dedicated setup guide for Atlas Cloud using `[[custom_providers]]`.
- `docs/providers/PROVIDER_GUIDES.md`
  - Added Atlas Cloud to the provider index.
- `vtcode.toml.example`
  - Added a commented Atlas Cloud example.
- `README.md`
  - Added Atlas Cloud to the supported-provider narrative.
  - Added a setup snippet and screenshot reference.

## Why This Approach

- Atlas Cloud's LLM API is OpenAI-compatible.
- VT Code already has a reusable `custom_providers` mechanism for
  OpenAI-compatible endpoints.
- The missing piece was CLI registration outside the interactive TUI path.
- This keeps the PR small and avoids duplicating provider logic that VT Code
  already owns.

## Live Validation

### Atlas Cloud API

- `GET /v1/models` succeeded with the provided local key.
- `POST /v1/chat/completions` succeeded against
  `deepseek-ai/DeepSeek-V3-0324`.

### VT Code Integration

- Before the fix, `vtcode ask -c atlascloud.local.toml ...` failed with
  `Unknown provider: atlascloud`.
- After registering `custom_providers` in the common CLI dispatch path, the same
  CLI flow resolved the provider correctly and completed a live request.

## Local-Only Files Used For Validation

These were intentionally kept out of git:

- `.env`
  - Stores `ATLASCLOUD_API_KEY`.
- `atlascloud.local.toml`
  - Local test config that points VT Code at Atlas Cloud.

## Suggested Reviewer Focus

- Confirm the CLI registration location is broad enough for non-interactive
  commands and harmless for commands that do not use providers.
- Confirm the documentation presents Atlas Cloud as an OpenAI-compatible custom
  provider rather than a brand-new built-in provider.
- Confirm the README change is concise enough for the main landing page.
