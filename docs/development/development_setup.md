# Development Setup

Canonical local setup for contributing to VT Code.

## Prerequisites

- Rust toolchain (stable) via [rustup](https://rustup.rs/). The repository pins an exact release in
  `rust-toolchain.toml`; see [Rust Toolchain and MSRV](./toolchain-updates.md) for the pin map and bump checklist.
- Git
- An LLM provider credential: either (a) a shell/workspace env var like `OPENAI_API_KEY`, `ANTHROPIC_API_KEY`,
  `GEMINI_API_KEY`, `GOOGLE_API_KEY`, `ZAI_API_KEY`, `MOONSHOT_API_KEY`, `STEPFUN_API_KEY`, or `MINIMAX_API_KEY`, (b) an
  OAuth session for an auth-managed provider, or (c) a key stored via `vtcode secret add <provider>`.

## Dev Container (Optional)

Opening an unfamiliar or untrusted checkout? Use the preconfigured [dev container](../../.devcontainer/devcontainer.json)
(VS Code: "Reopen in Container") instead of your host toolchain. It tracks the same pinned Rust image as
`Dockerfile.build`, so builds behave identically, while untrusted repo content stays inside the container boundary.

## One-Time Setup

```bash
git clone https://github.com/vinhnx/vtcode.git
cd vtcode
./scripts/setup.sh --with-hooks
```

`./scripts/setup.sh` verifies `rustfmt`/`clippy`, installs `cargo-nextest` when missing, and runs `cargo check`.

## Credential identities

Secure API-key storage is scoped by `(provider, key name)`, where the key name is the environment variable used for that
credential. This keeps multiple profiles for one provider independent:

```bash
vtcode secret add mimo --key-name MIMO_API_KEY
vtcode secret add mimo --key-name MIMO_TOKEN_PLAN_KEY
vtcode secret status mimo --key-name MIMO_TOKEN_PLAN_KEY
```

Environment variables and workspace `.env` values take precedence over secure storage. Existing provider-only entries
are migrated lazily only when the requested key is the provider default and the legacy account is unambiguous;
non-default profiles require an explicit key name. Configured `[[custom_providers]]` and `[providers.<name>]`
overrides use the same identity rules.
Workspace `.env` values are scoped to credential lookup. Older key-scoped secure-storage entries cannot be safely
attributed and must be added again with `vtcode secret add`.

For debug or release launches:

```bash
./scripts/run.sh
./scripts/run-debug.sh
```

On macOS 14 and later, `run-debug.sh` packages the freshly built CLI into a local app bundle and uses `gktool` to
prewarm Gatekeeper before launching it. This avoids the brief first-launch “Verifying…” sheet after a debug rebuild. If
Gatekeeper cannot scan the bundle, the launcher stops instead of starting a process that could repeatedly show the sheet
when VT Code starts its helpers. The bundle is only re-copied, re-signed, and re-scanned when the debug binary actually
changed, so relaunching without edits skips that work and keeps the bundle signature stable for Gatekeeper's cache.

`run-debug.sh` also forwards extra command-line arguments to the binary, so `./scripts/run-debug.sh --resume` runs a
resumed debug session. It appends those arguments after the built-in defaults and after the `MODEL`, `PROVIDER`, and
`WORKSPACE` environment variables, so an explicit argument overrides the environment-derived one.

## Daily Development Loop

```bash
# Fast compile check
cargo check

# Fast test loop (recommended)
cargo nextest run

# Fallback if nextest is unavailable
cargo test --workspace
```

## Compiler caching (sccache)

[sccache](https://github.com/mozilla/sccache) is a rustc wrapper that stores compilation results outside
`target/`. A cache hit skips rustc entirely, and the results survive `cargo clean`, so clean rebuilds do not start
from scratch. It is optional but recommended; the largest wins are clean builds and repeated CI jobs.

```bash
brew install sccache              # or: cargo install sccache --locked
export RUSTC_WRAPPER=sccache
export SCCACHE_CACHE_SIZE=2G      # size for at least one full build per profile/target/toolchain
./scripts/check-dev.sh            # or: cargo check
sccache --show-stats              # Rust cache hits/misses, non-cacheable calls, evictions
```

Cargo also accepts the wrapper through its `build.rustc-wrapper` config key (`CARGO_BUILD_RUSTC_WRAPPER`);
`check-dev.sh` recognizes both. Settings such as the cache size are read when the sccache server starts — after
changing them, run `sccache --stop-server` before the next build. `sccache --zero-stats` resets counters only, not
stored results.

**Incremental compilation:** sccache rejects `-C incremental`, so `Cargo.toml` disables it in `[profile.dev]`. The
local, gitignored `.cargo/config.toml` re-enables it for workspace crates (fast edit loop) while keeping dependencies
out of incremental mode (`[profile.dev.package."*"] incremental = false`) so they stay cacheable. Do not set
`CARGO_INCREMENTAL=1` globally on sccache hosts: `check-dev.sh` deliberately omits it when a wrapper is detected.

**Cache care:** a changed compiler, feature, profile, target, flag, source path, or environment produces a different
cache key; it does not delete older entries. Local disk space is reclaimed by least-recently-used eviction once
`SCCACHE_CACHE_SIZE` is reached. A limit that is too small evicts entries before the next build can reuse them, so
compare `2G` against the peak size reported by `sccache --show-stats` before raising it — a much larger limit can
thrash. `cargo clean` removes `target/` but not the sccache store, so prefer it only when disk space or
troubleshooting calls for it. Binaries, proc macros, build scripts, and linking are never cached. For CI, sccache is
an opt-in pilot; see [Compiler-cache pilot](./ci-cd.md#compiler-cache-pilot) and measure the whole job, including
cache restore and upload, before enabling it by default.

## Full Quality Gate

```bash
./scripts/check.sh
```

This runs formatting checks, linting, governance checks, build, tests (nextest-first), and docs generation.

## Common Commands

```bash
# Format
cargo fmt --all

# Lint (deny warnings)
cargo clippy --workspace --all-targets --all-features -- -D warnings

# Build
cargo build --workspace

# Docs
cargo doc --workspace --no-deps

# Docs, including private items (maintainer inspection)
cargo doc --workspace --no-deps --document-private-items
```

## Troubleshooting

- `cargo nextest` missing:
  - Run `cargo install cargo-nextest --locked`
- No provider credential found:
  - Run `vtcode secret add <provider>` to store a key in your OS keyring (recommended), or
  - Use `vtcode secret add <provider> --key-name <ENV_VAR>` for a non-default provider profile, or
  - `export OPENAI_API_KEY="sk-..."` (or the equivalent env var for your provider) in your shell, or
  - Run `vtcode login <provider>` for OAuth/managed-auth providers (copilot, openai, openrouter).
- Script permissions:
  - Run `chmod +x scripts/*.sh`
