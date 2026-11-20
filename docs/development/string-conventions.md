# String conventions: typed identity over raw literals

Applies to every Rust crate. This complements the localization rule: user-facing copy goes through `rust_i18n::t!`, while **identity/data strings must not be re-spelled at call sites**. A raw literal is fine only at a single producer with no two-way contract; once a spelling is written by one surface and matched by another, it needs one typed home.

## When to centralize

Centralize a literal when it is:
- a **closed-set discriminator** (status, role, mode, reason code) compared with `==`, `matches!`, `eq_ignore_ascii_case`, or used as a map key;
- a **two-way contract** whose writer and parser can drift silently (wire names, deeplink verbs, config keys, tool names, keystroke names, setting paths);
- **repeated across 2+ production files** as an identity string.

Leave inline: single-use data grammars, parser alias lists owned by one function, display copy with a locale key, and unrelated identities that merely share a spelling (e.g. `TerminalPosition::id()` returning `"left"` is a settings id, not the keystroke key).

## Canonical homes (as of 2026-11-20)

| Vocabulary | Home |
| --- | --- |
| Tool names (grouped + flat re-exports + `groups::*` slices) | `vtcode-commons::tool_types::tool_names`; re-exported as `vtcode-config::constants::tools` |
| Runtime wire `type` values | `vtcode-exec-events::wire_type::*` + `ThreadEvent::wire_name()` (guarded by `wire_name_matches_serialized_type_tag`) |
| Item status (`completed`/`failed`/`in_progress`/…) | `vtcode-exec-events` enum `as_str()` methods (guarded by `status_as_str_matches_serde_snake_case`); tracker statuses via `tracker::TrackerItemStatus` |
| Matrix worker-report outcomes (`executed`/`failed`/…) | `vtcode-utility-tool-specs::report_outcome` — deliberately **not** the snapshot `MatrixOutcome` enum (`Success` serializes as `"success"` there) |
| Agent type names | `AgentType` associated consts shared by `Display`/`FromStr` |
| Provider ids | `vtcode-commons::provider::{Provider, CODEX_PROVIDER_ID}`; match via `Provider::name_matches()` |
| Keystroke names | `vtcode-desktop::strings::key::{KeyName, *}` |
| Deeplink verbs / route segments | `vtcode-desktop::view::deeplinks::Verb`; tab names via `SettingsTab::{as_str, from_name}` |
| Archived phases / diff kinds / slash names | `transcript::ArchivedPhase`, `DiffFileKind::as_str()`, `composer_controls::SlashCommandName` |
| Dotted config keys | `vtcode-commons::config_keys::ConfigKey` |
| JSON-RPC param keys | `vtcode-desktop::strings::keys` |
| Wire method names | `vtcode-desktop-protocol::method` |

`ThreadEvent` is authoritative for runtime events — never add a parallel event or a second wire-name table.

## Rust mechanics that bite when centralizing

- `match` arms need a named or associated `const`, not a `const fn` call: derive `const X: &str = E::A.as_str();` and match on `X`.
- `format!` implicit capture accepts only bare identifiers: use `format!("{}/{}", Verb::CONFIG, path)`, not `format!("{Verb::CONFIG}/{path}")`.
- Guarded arms do not count toward exhaustiveness: keep a trailing `_ => ...`.
- `json!({ CONST_KEY: value })` accepts const-path keys directly.
- `#[serde(other)]` catch-all variants have **no** rename — they serialize as the variant name (`"Unknown"`, not `"unknown"`). Verify with a serialize-and-compare test.
- A local variable named `key` does not shadow a module named `key` (different namespaces), but rename it for readability.

## Verification

After touching a vocabulary: `./scripts/check-dev.sh`, focused `cargo nextest run` for the owning crate, and `python3 scripts/check_file_length.py --mode enforce`. Extract a const group into its own module file if `lib.rs` approaches 1000 lines.
