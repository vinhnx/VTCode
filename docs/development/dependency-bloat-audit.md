# Dependency Bloat Audit (2026-11-14)

Audit of redundant and bloated dependencies across the workspace, with the goal of keeping the
`vtcode` CLI binary and the `vtcode-desktop` binary lightweight. Follows the precedent of
[prompt-and-dependency-simplification](prompt-and-dependency-simplification.md): every verdict carries
evidence and a citation. Related policy: [ci-cd](ci-cd.md) (cargo-deny/vet gates),
`.agents/skills/crate-ops` (manifest-change procedure), `.agents/skills/evaluate-crate` (adding deps).

## Method note: Cargo.lock presence ≠ compiled

`Cargo.lock` lists *optional* dependencies that a build would never compile. Verdicts below were
derived from manifests, registry `Cargo.toml` feature tables, and `cargo tree` with the workspace's
actual feature selection — not from raw lock entries.

## Baseline (2026-11-14, macOS aarch64, local)

| Metric | Value |
| --- | --- |
| `target/release/vtcode` size | 30.1 MB (`opt-level="z"`, fat LTO, `codegen-units=1`, strip) |
| Duplicate-version groups (`cargo tree --workspace -d`) | 160 lines / ~66 unique names |
| `vtcode` dependency closure (`cargo tree -p vtcode -e normal`) | 2186 nodes |
| Binary ↔ desktop relationship | separate binaries; CLI links only `vtcode-desktop-protocol` (JSON-RPC) |

The release profile itself is already optimal (see `decisions.md` 2026-10-09/2026-08-06: `codegen-units=1`
for fat LTO gave −17%, `opt-level="z"` took 61 MB → 34 MB, cold start 1.16 s → 0.29–0.65 s). Do not
re-tune profiles without re-measuring; this audit targets the dependency graph instead.

## Applied changes (this audit)

1. **Manifest centralization** (`6b7ed4ae7`): root path+version internals migrated to
   `workspace = true`; inline version bypasses aligned (`smallvec`, `zip`, `axum` in vtcode-auth,
   `arc-swap` in vtcode-core/vtcode-mcp, `async-stream` + `colorchoice` centralized, root dev `rand`
   0.9 → workspace 0.10); root `axum` made optional behind `anthropic-api`; `tokio-util`/`compat`
   dropped (only `CancellationToken` used); `itertools` removed (one call site inlined).
2. **Unused deps removed**: `flate2` + `zip` from vtcode-core (skill packaging lives in
   vtcode-skills, which declares its own `zip`; verified via `rg` + `cargo machete`).
3. **tokio "full" trimmed** (`6c1d696ce`): workspace entry is bare `tokio = "1.52"`; every
   workspace-inherited consumer declares its real feature subset (crate-ops rule). Lock drops
   tokio's `parking_lot 0.12.5` dep line. Compile-time win; LTO already dead-stripped most of it.
4. **once_cell removed** (`0749f9868`): 40 files migrated to `std::sync::LazyLock`/`OnceLock`
   (MSRV 1.100; std types stable since 1.80; 262 files already used the std convention).
   once_cell dropped from `[workspace.dependencies]` + 6 crate manifests.
5. **cargo-machete wired into the gates** (`run_cargo_machete` in `scripts/common.sh`; runs in the
   `check-dev.sh` fast gate and `scripts/check.sh` release gate, warn-and-skip when the tool is
   absent — mirroring `run_cargo_deny`).

## KEEP verdicts (verified, not bloat)

| Dependency | Why it stays |
| --- | --- |
| `tree-sitter` + `tree-sitter-bash` (vtcode-safety, unconditional) | Load-bearing for shell-command safety parsing; prior audit verdict (2026-11-12) reaffirmed. |
| `syntect` (`default-fancy`, ~250 grammars) | Product surface: markdown code blocks, diff/log highlighting; grammars are embedded dump data. Trimming needs a custom dump — not worth the maintenance. |
| `terminal_size` (vtcode-core) | Used in non-TUI code paths that must work without the optional `crossterm`. |
| `humantime` (2 sites) | Real API (`parse_duration`, `format_rfc3339_seconds`); std has no equivalent; crate is tiny and dep-free. |
| `glob` (vtcode-core) | Distinct API from `ignore` (Pattern/MatchOptions semantics); replacement is non-trivial. |
| `iana-time-zone` (vtcode-mcp) | Direct usage (`get_timezone()`); depending on chrono's transitive would be unsound. |
| `rmcp` client+server, `axum`/webmcp stack | Product surface (MCP client, WebMCP server, desktop-server mode). |
| `openai-harmony` | Only codec for OpenAI gpt-oss harmony encoding; `default = []` so its deps (clap, image, bstr, reqwest 0.12 blocking, fancy-regex 0.13) are mandatory upstream. |
| `mimalloc` / `tikv-jemallocator` | Allocator design (mutually exclusive cargo features) — the reason `unused_dependencies` stays `warn`, not `deny`. |
| `[patch.crates-io]` vendored patches | Intentional, each with `patches/<crate>/PATCH.md` rationale. |

## DEFERRED — upstream-bound, with revisit triggers

| Item | Blocker | Revisit trigger |
| --- | --- | --- |
| reqwest 0.12 + 0.13 dual stack in the CLI binary | `openai-harmony` 0.0.8 (newest) pins reqwest ^0.12.5; rmcp 3.9 + rig 0.40 require 0.13. The `rmcp-reqwest` alias in vtcode-mcp is load-bearing (type-level, not a hygiene bug). | openai-harmony release with reqwest 0.13 → migrate workspace 0.12 → 0.13 (~30 sites), drop the alias. |
| `ratatui-cheese` re-enables ratatui default features via unification (calendar widget → duplicate `time` crate, `ratatui-macros`, `underline-color`) | cheese's own manifest requests plain `ratatui = "0.30"` with defaults; feature unification takes the union, so `default-features = false` on *our* edge is a no-op (verified: the flag was applied and reverted as ineffective). Load-bearing dep (13 files: input/help/theme/spinner), so vendoring a fork is disproportionate. Note: `time` stays compiled regardless via the mac-notification-sys patch and syntect→plist. | ratatui-cheese sets `default-features = false` upstream, or a `cargo bloat` run shows the calendar+macros chain is material. |
| fancy-regex ×3 (0.13 harmony / 0.16 syntect / 0.19 jsonschema), tokio-tungstenite 0.29+0.30 (axum ws / vtcode-llm), toml_edit 0.22+0.25, base64 0.22+0.23, dirs ×3, hashbrown ×5 (desktop) | transitive pins of always-on product deps | parent-crate bumps; `deny.toml` `multiple-versions = "warn"` tracks drift. |
| desktop gpui-fork duplicates (gpui-pre-reqwest 0.12.15, dirs 5/6, rand 0.8, toml 0.8, serde-saphyr 1.3) | `patches/gpui-pre-macos` fork | gpui-pre fork update; desktop-only, no CLI impact. |
| `async-process` (smol family, ~10 small crates) in vtcode-core | used in exactly one file (`src/exec/async_command.rs`); likely deliberate for child-reaping semantics that `tokio::process` handles differently (unix reaper) | separate evaluation with exec-layer behavioral tests + measurement. |

## Known manifest-lint expectations

- cargo ≥1.100 emits `warning: unused dependency \`mimalloc\`` under `--all-features` (check-dev
  clippy runs `--all-features`): with `profiling` + `allocator-jemalloc` both on, both allocators
  are cfg'd out. Documented in `.vtcode/memory/gotchas.md` (2026-11-14); keep
  `unused_dependencies = "warn"`.
- The previous "unused workspace deps vtcode-acp/vtcode-eval/vtcode-webmcp" warnings are resolved by
  the `workspace = true` migration (the crates are real dependencies; the lints flagged
  workspace-entry entries that nothing inherited from).

## Ongoing policy

- `cargo machete` now runs in the fast gate; keep `[package.metadata.cargo-machete] ignored` lists
  justified. Machete false positives (cfg-gated deps, re-exports) must be verified with `rg` + a
  `lib.rs` `pub use` check before removal (gotchas 2026-07-18, 2026-10-14).
- Adding a dependency goes through `evaluate-crate` (redundancy check is step 1 there).
- Manifest edits go through `crate-ops`: isolated `cargo check --locked -p <crate>` per touched
  crate — workspace-wide builds mask missing features via unification (observed twice in this
  audit: `tokio::join!` needed an explicit `macros` edge; vtcode-safety needed `process`).
