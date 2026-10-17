# Fast mode

`/fast [on|off]` controls the active provider's saved speed preference. Bare `/fast` toggles, `on` enables,
and `off` removes that provider's override. Arguments are case-insensitive; invalid or extra arguments show usage.

| Native route | Setting | Eligible models | Header badge |
| --- | --- | --- | --- |
| OpenAI API key | `provider.openai.service_tier = "ultrafast"` | `gpt-6-astra`, `gpt-6.1-sol` | `ULTRAFAST` |
| Anthropic Claude API | `provider.anthropic.speed = "fast"` | `claude-opus-5-5`, `claude-opus-5`, `claude-opus-4-8` | `FAST` |

Dated Claude snapshots use the same eligibility as their exact base model IDs. Opus 4.8 can be entered explicitly
with `/model` as `anthropic claude-opus-4-8`; it is not added to the curated catalog. Cloud platforms, gateways,
custom providers, and nonstandard endpoints are excluded from native fast mode. Anthropic URL resolution uses
`provider_overrides.anthropic.base_url`, then `ANTHROPIC_BASE_URL`, then `https://api.anthropic.com/v1`.

OpenAI disabling restores the project default, without restoring a previous flex/priority selection. Anthropic
disabling removes `speed`, restoring standard speed. `/fast off` remains available after switching to an
incompatible model or endpoint. Other providers return guidance without changing either saved preference.
Preferences survive provider/model switches independently; incompatible preferences remain dormant and resume
when an eligible route is selected. The header shows only the effective active provider setting.

`/model` offers keep current, standard, or fast for eligible Anthropic selections in inline, ratatui, and typed
flows. Cancelling saves nothing. Explicit per-request `AnthropicRequestOverrides.speed` wins over configuration.
An explicit incompatible `fast` request fails before dispatch; an inherited preference on that route is omitted.
Both streaming and non-streaming generation use the same resolver. Opus 5.5 needs no automatic fast beta;
Opus 5 and 4.8 add `fast-mode-2026-02-01` whenever `speed` is serialized, including explicit `standard`.
Other beta headers are preserved and deduplicated.

Fast mode uses premium pricing. Switching Claude speeds invalidates prompt-cache reuse between those speeds.
Existing bounded 429/529 retries retain fast speed and error `retry-after` metadata; there is no automatic
standard-speed fallback. Refusal retries also resolve model eligibility: dormant preferences are omitted on an
incompatible fallback, while explicit incompatible fast requests prevent that retry.
See [Anthropic fast mode](https://platform.claude.com/docs/en/build-with-claude/fast-mode),
[Messages API](https://platform.claude.com/docs/en/api/messages/create), and the
[OpenAI ultrafast guide](https://developers.openai.com/api/docs/guides/ultrafast-mode).

The TUI offers one informational suggestion per session for an eligible API-key route when fast mode is off.
It names `/fast on`, premium pricing, and Claude's cache effect. It reevaluates at idle after model/auth changes,
without credential discovery or network calls. Using `/fast` or an already selected fast preference dismisses it.

Changes apply to subsequent requests and persist using the model picker's `ConfigManager` path. Repeated explicit
settings are idempotent in their resulting configuration. Each accepted command rebuilds the client and persists the
choice, reconciling defaults captured before a configuration reload and runtime/disk differences. The handler prepares
a replacement provider client with the new tier and the existing context-window wrapper before saving. It installs
that client and runtime configuration only after saving succeeds.
Initialization or save failures leave active settings and the client untouched. Only the active provider speed field
changes; model, reasoning effort, authentication, conversation history, and WebSocket configuration are preserved.

The header reflects the effective setting immediately. Anthropic `FAST` and OpenAI `ULTRAFAST` appear as padded badges
with a premium `#D99A4E` gold fill and black text in both header layouts, exceeding WCAG AAA text contrast (7:1).
The badge stays undimmed, respects bold-is-bright, uses ANSI black/yellow in safe-color mode, and falls back to a
monochrome inverse label when colors are disabled. Other tiers retain their normal labels.
Rebuilding the provider permits its WebSocket connection to reconnect normally. Both HTTP request builders and
the shared WebSocket payload path apply model compatibility to
explicit tiers and provider defaults. The existing server-rejection fallback remains: configuration success confirms
the requested tier, while server-reported telemetry identifies the tier actually served. Anthropic logs independently
parse `usage.speed` at completion for both transports; missing speed is not inferred from the requested setting.

## Implementation reference

Saved versus effective preference resolution was verified against
[`codex-rs/tui/src/service_tier_resolution.rs` at `bb64b894fd67158be0f874872e6d7bfe717e7368`](https://github.com/openai/codex/blob/bb64b894fd67158be0f874872e6d7bfe717e7368/codex-rs/tui/src/service_tier_resolution.rs),
Apache-2.0, copyright 2025 OpenAI. No upstream code was copied.

The active-context plus persistent-selection pattern was verified against
[`codex-rs/tui/src/chatwidget/service_tiers.rs` at `bb64b894fd67158be0f874872e6d7bfe717e7368`](https://github.com/openai/codex/blob/bb64b894fd67158be0f874872e6d7bfe717e7368/codex-rs/tui/src/chatwidget/service_tiers.rs),
Apache-2.0, copyright 2025 OpenAI. VT Code uses its existing factory and configuration manager; no upstream code was
copied. VT Code additionally prepares the client before saving and installs it after saving for atomic active state.

## Verification

Focused coverage includes command discovery and parsing, compatibility and picker availability, tier transitions,
idempotence, runtime/disk drift, clearing a stale client after reload, subscription-route rejection, clearing after a
model switch, preservation of unrelated configuration, initialization/save failure
atomicity, and HTTP/WebSocket payloads with explicit and inherited tiers. Header tests cover both layouts, narrow
terminals, theme contrast, and color/bold accessibility policies. Run:

```sh
cargo nextest run --locked -p vtcode -p vtcode-config -p vtcode-skills -p vtcode-llm --no-tests=fail \
  -E 'test(fast) | test(service_tier) | test(available_service_tiers)'
cargo nextest run --locked -p vtcode-ui --no-tests=fail -E 'test(header) | test(theme) | test(ultrafast_badge)'
./scripts/check-dev.sh
```

These fixtures do not make live billable provider calls or establish account-specific access, latency, or served tiers.
