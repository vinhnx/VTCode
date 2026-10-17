---
feature: clear-tool-inputs-default
status: delivered
updated: 2026-09-29
branch: feat/clear-tool-inputs-default
commits: e872b4c5d..d2f448adc
---

# Clear Tool Inputs by Default

## Report

**What was built** — `agent.harness.tool_result_clearing.clear_tool_inputs` now defaults to `true`, so request-only
clearing stubs paired `tool_calls[].function.arguments` (and freeform `text`) together with the old tool results it
already reclaimed. Edit-heavy sessions no longer re-pay full `apply_patch` patch bodies and `write_file` file bodies on
every request after the result stub landed. Explicit `clear_tool_inputs = false` remains the opt-out. A named serde
default fn (`default_tool_result_clearing_clear_tool_inputs`) makes missing TOML keys follow the same path as `Default`
— bare `#[serde(default)]` on a `bool` would have silently kept `false`. The same config field feeds Anthropic native
`clear_tool_uses` wire edits, so that payload now sends `clear_tool_inputs: true` as well. Durable history and
`vtcode-exec-events::ThreadEvent` are unchanged.

**Verification** — `./scripts/check-dev.sh` PASS. `cargo nextest run -p vtcode-config -E 'test(tool_result_clearing)'`
5/5 PASS. `cargo nextest run -p vtcode-core -E 'test(clear_old_tool) or test(tool_result_clearing)'` 10/10 PASS
(includes `clear_old_tool_results_default_config_clears_paired_inputs`).
`cargo nextest run -p vtcode -E 'test(request_builder) or test(clear_tool_uses) or test(local_tool_result)'` 31/31 PASS.
`cargo nextest run -p vtcode-config --lib` 453/453 PASS.
`RUSTFLAGS="-D warnings" cargo check --locked -p vtcode-config -p vtcode-core -p vtcode` PASS. Independent review:
approve (5/5 AC, 0 critical/high).

### Journey log

- Bare `#[serde(default)]` on a `bool` field is `false`, not the struct `Default` — the default fn must be named on the
  field or missing TOML keys silently keep the old behavior.
- `clear_tool_inputs` is shared by local `clear_old_tool_results` and Anthropic native `clear_tool_uses`; a default flip
  moves both pins (`request_builder.rs` context_management expectation).
- CONFIG_FIELD_REFERENCE.md is generator-sourced but routinely hand-edited (~119 lines of pre-existing drift); matching
  the rustdoc prose keeps a future regen stable.

## [S1] Problem

`agent.harness.tool_result_clearing` stubs old tool _results_ past `trigger_tokens` (default 40k / keep 2), but
`clear_tool_inputs` defaulted to `false`. Assistant `tool_calls[].function.arguments` therefore kept every historical
`apply_patch` patch body and `write_file` file body on every request after the paired result was already stubbed.

Edit-heavy coding sessions paid this cost twice: the result was reclaimed, but the model's own pasted patch/file content
— often the largest string in the transcript — rode every subsequent request until late compaction. This was the largest
remaining first-class token leak in the local-clearing path.

## [S2] Design

Settled: flip the default so input clearing follows result clearing. Keep the opt-out. Do not change the clearing
contract.

### S2-default: `clear_tool_inputs` defaults to true

- In `vtcode-config`, `ToolResultClearingConfig::default()` sets `clear_tool_inputs: true`.
- Missing TOML keys follow the same default: the serde field uses
  `#[serde(default = "default_tool_result_clearing_clear_tool_inputs")]` returning `true` (bare `#[serde(default)]` on a
  `bool` is `false` and would silently keep the leak).
- Explicit `clear_tool_inputs = false` remains a supported opt-out.
- No change to `clear_old_tool_results` semantics: inputs are replaced only for call ids whose paired results were
  stubbed, with the existing valid-JSON placeholder `{"cleared":"tool_input"}`. Tool name stays on `call.function.name`.
  Freeform `text` and `thought_signature` are cleared the same way as today.
- Anthropic mutual exclusion unchanged: local clearing (results + inputs) runs only when the wire will not carry native
  `clear_tool_uses`. When native edits are used, the same config field feeds the wire `clear_tool_uses_20250919` edit
  (`clear_tool_inputs: true`).
- Durable history and `vtcode-exec-events::ThreadEvent` remain untouched (request-only shaping).

### S2-docs: state the new default

- `docs/config/CONFIG_FIELD_REFERENCE.md`: default `false` → `true`.
- `docs/development/EXECUTION_POLICY.md` tool-result-clearing bullet: note that paired tool _inputs_ are cleared by
  default as well.

## [S3] Out of Scope

- Changing `trigger_tokens` / `keep_tool_uses` / `clear_at_least_tokens`.
- Compaction thresholds or compaction prompts.
- Wire-level Anthropic `clear_tool_uses` behavior beyond the default value.
- Placeholder shape changes or path-preserving input stubs.
- Startup warning text beyond what already covers disabled clearing.

## Tasks

- [x] T1: default `clear_tool_inputs` to true (struct Default + serde default fn) — acceptance:
      `ToolResultClearingConfig::default().clear_tool_inputs` is true; a TOML harness config that omits the key also
      yields true; explicit `false` still parses (covers: S2-default)
- [x] T2: update pin tests + docs — acceptance: `test_tool_result_clearing_defaults` asserts true;
      CONFIG_FIELD_REFERENCE and EXECUTION_POLICY show the new default (covers: S2-docs; depends: T1)
- [x] T3: regression that cleared results clear paired inputs by default — acceptance: `clear_old_tool_results` with
      default-on config replaces arguments for stubbed call ids and leaves kept calls intact (covers: S2-default;
      depends: T1)
