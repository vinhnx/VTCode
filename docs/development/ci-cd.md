# CI/CD and Code Quality

This document describes the CI/CD pipeline and code quality tools used in the vtcode project.

## GitHub Actions Workflows

The project runs these GitHub Actions workflows: `ci.yml` (PR/main quality gate),
`build-linux-windows.yml` (release binaries), `webmcp.yml` (Pages deploy),
`hotpath-benchmark.yml` + `hotpath-relay.yml` (benchmarks), `desktop-macos.yml`
(desktop validation), `deny.yml` (dependency audit), and `verify-lockfile.yml`
(action pin verification). See [action lockfiles](#action-lockfiles)
for how `uses:` refs are pinned.

### Action lockfiles

Workflows reference actions by symbolic tag or branch (`actions/checkout@v7.0.1`); the committed
`.github/workflows/actions.lock` records the exact commit SHA and owner/repository IDs that the
Actions runner executes. This keeps `uses:` lines readable while still rejecting mutable or
tampered pins. A bare 40-character SHA is allowed inline only where the lockfile format cannot
cover it — reusable workflow calls at `jobs.<id>.uses`, such as `hotpath-relay.yml`.

After adding or changing any `uses:` ref, regenerate the lockfile:

```bash
gh extension install github/gh-actions-lock   # once
gh actions-lock
```

Commit the workflow and `actions.lock` changes together. Verification runs at two levels:

- **Offline / local:** `./scripts/check.sh workflow-security` checks trigger policy and that every
  symbolic ref has a lockfile entry with a 40-character commit. `gh actions-lock --verify-local`
  is an equivalent offline coverage check.
- **CI:** `verify-lockfile.yml` installs a pinned `gh-actions-lock` extension and runs
  `gh actions-lock --verify`, which additionally re-resolves every ref upstream and fails when a
  pin is stale or unreachable.

Two pins were orphaned when their upstream moving refs advanced and had to be moved to live refs
during onboarding: `dtolnay/rust-toolchain@stable` and `taiki-e/install-action@v2.90.5`. Every other
action resolves to the exact commit it was previously pinned to. Full semver tags are preferred
for actions with moving major tags; see the
[install-action security guidance](https://github.com/taiki-e/install-action#security). Dependabot
updates refs and the lockfile together with a 7-day `cooldown` so fresh releases settle first.

### Workflow static analysis (zizmor)

`verify-lockfile.yml` also runs [zizmor](https://docs.zizmor.sh/), a static analyzer for CI/CD
definitions, against every workflow, action, and `dependabot.yml`. It is pinned by release tag **and**
SHA-256 in the workflow (no new action ref, so `actions.lock` is unchanged), and runs with
`--offline --persona=regular`, which keeps the gate deterministic and token-free. `dependabot.yml`
is in the workflow's path filter, so cooldown regressions are caught too.

To run the exact gate locally (the version is read from the CI workflow, so it cannot drift):

```bash
./scripts/check.sh workflow-audit   # or ./scripts/check_zizmor.sh directly
```

The local gate needs `uv` for `uvx`, or a matching `zizmor` binary on `PATH`; if neither is present it
warns and skips, while CI always enforces the audit.

The audit complements `./scripts/check.sh workflow-security`: that script enforces the local trigger
policy and lockfile pinning, while zizmor covers template injection, permission scope, credential
persistence, Dependabot cooldown, and other audit rules that a regex gate cannot express. VT Code's
`actions.lock` is recognized by zizmor, so already-pinned `uses:` refs are suppressed rather than
reported.

Two conventions keep the gate meaningful:

- **Fix, don't blanket-disable.** Workflow inputs bound into `run:` bodies are consumed through a
  job-level `env` (e.g. `TAG: ${{ inputs.tag }}` then `$TAG`) instead of a `${{ ... }}` expansion in
  shell source. This is the only remediation for `template-injection` that survives a future caller
  passing an unexpected value.
- **Exceptions must be reviewed and narrow.** The single `dangerous-triggers` exception in
  `hotpath-relay.yml` is marked inline with `# zizmor: ignore[dangerous-triggers]` and points to the
  matching entry in `scripts/workflow_security_allowlist.txt`. Never disable an audit globally in a
  `zizmor.yml` to silence a finding; prefer a targeted ignore with a written rationale.

Bumping zizmor means updating both `ZIZMOR_VERSION` and `ZIZMOR_SHA256` in `verify-lockfile.yml` in
the same commit, and re-running `./scripts/check.sh workflow-audit` to confirm a clean audit.

### 1. Build Linux & Windows (`build-linux-windows.yml`)

**Triggers:**

- Manual `workflow_dispatch` with release tag input
- Dispatched from `scripts/release.sh` Step 3.5 via `gh workflow run`

**Rust toolchain:** every Rust setup step (three in this workflow, two in `hotpath-benchmark.yml`, one each in
`deny.yml` and `desktop-macos.yml`) explicitly selects the version in `rust-toolchain.toml`, currently 1.100.0.
The [setup action](https://github.com/dtolnay/rust-toolchain/blob/stable/action.yml#L12) otherwise defaults to
floating `stable`: `actions.lock` pins the action commit, but that does not pin the compiler.
Update the workflow inputs alongside the canonical toolchain version, following
[Rust Toolchain and MSRV](./toolchain-updates.md).
`cargo nextest run --locked -p vtcode --test toolchain_pins_test` checks active
`.yml`/`.yaml` workflow Rust setup steps, including missing inputs and drift,
without assuming an old CI filename. Explicit nightly checks remain separate.

**Jobs:**

- **Build GNU Linux**: Builds `x86_64-unknown-linux-gnu`, runs that binary's
  `man --all-pages --output man/man1`, and uploads its release archive plus the shared `man-pages` artifact.
  This removes the separate `xtask` development-profile build for current tags. Rebuilding older tags
  without `man --all-pages` falls back to `xtask`; generation failures do not trigger that fallback.
- **Build cross Linux**: Compiles `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-gnu`
  (`fail-fast: true`, `timeout 45m`). This job has no dependency edge on the GNU job, so both cross
  targets start compiling immediately instead of idling behind the native build. The only shared input
  is the `man-pages` artifact: after its own binary is built, the job polls the run's artifact list via
  `gh api` (40 attempts, 15s apart, ~10 min) and then downloads it. Polling the API rather than parsing
  `gh run download` output separates "not uploaded yet" from a genuine 404/auth failure, which fails
  immediately. If the GNU producer job completes unsuccessfully before uploading the artifact, the cross
  job also fails promptly instead of waiting out the full window. An empty artifact also fails the job,
  so a release can never ship an archive without man pages. Cross uses its container toolchains; no host
  Zig installation is needed.
- **Build Windows**: Compiles `x86_64-pc-windows-msvc` binary (`--jobs 4`, `timeout 45m`)
- **Upload Artifacts**: Stores compiled binaries + extension-stripped `.sha256` sidecars for release
  (`retention-days: 2`, `compression-level: 0` — archives already compressed)
- **Cache**: each job namespaces its own entry with `key: <target>`. Keep `hashFiles('Cargo.lock')` out
  of `prefix-key`: rust-cache restores the full key first and falls back to the key without the lockfile
  hash segment, so hashing the lockfile earlier would shrink that fallback. Cache storage is 10 GB per
  account on every plan, including public repos where minutes are free; let GitHub evict rather than
  adding a second cache lineage.
- **Wait for tag**: every build job first polls the remote (inline `bash`) until the release tag is
  visible before checkout, deriving the repo URL from `GITHUB_REPOSITORY` so forks and renames stay
  correct. It must stay inline: a local `uses: ./.github/actions/...` cannot run before
  `actions/checkout`, because the action file is not in the workspace yet.

**Cross-target cache keys:** `Swatinem/rust-cache` keys off the target triple only when `CARGO_BUILD_TARGET`
is set or you pass `key:`. Builds that pass `--target` via the CLI (e.g. `cross build --target`) share ONE cache
key across matrix jobs on the same runner OS, causing colliding/failing saves and every target restoring a
mismatched cache. Always namespace the cache per target (`with: { key: ${{ matrix.target }} }`) in cross-target
matrix jobs.

**Compiler-cache pilot:** the dispatch/reusable-workflow input `compiler_cache_pilot` defaults to
`false`. Set it to `true` to install sccache for native GNU Linux and Windows, with incremental
compilation disabled and cache statistics recorded. Cross-container builds are deliberately excluded.
Compare repeated runs of the same tag, target, toolchain, and build profile against the default;
include cache setup, restore/save, compilation, and linking in the comparison. Do not enable it by
default until net runner-minutes improve. sccache cannot cache Rust invocations that invoke the
system linker, including binary linking; see the [sccache Rust limitations](https://github.com/mozilla/sccache/blob/main/docs/Rust.md).
No hosted speedup is established by the local regression tests.

```bash
gh workflow run build-linux-windows.yml --field tag=<tag> --field compiler_cache_pilot=true
```

### Man-page generation reference

| Command                                    | Output                                                            |
| ------------------------------------------ | ----------------------------------------------------------------- |
| `vtcode man`                               | Main page on stdout                                               |
| `vtcode man chat --output chat.1`          | One command page in a file                                        |
| `vtcode man --all-pages --output man/man1` | Main page and every visible top-level command page in a directory |

`--all-pages` requires `--output` and conflicts with a single command name. Generation uses the same
compiled clap command tree as `--help`; there is no separately maintained command list. Filesystem
errors propagate to the caller. Existing files with matching names are overwritten; other files
in the directory are retained, so use a fresh staging directory when packaging.

### 2. WebMCP application (`webmcp.yml`)

**Triggers:** push to `main` touching `apps/webmcp/**` or the workflow itself, plus manual
`workflow_dispatch`.

**Jobs:** one `deploy` job sets up Bun, installs the browser dependencies with a frozen lockfile,
runs the editor typecheck and tests in `parallel:`, builds the Vite app, and publishes it to
GitHub Pages.

### hotpath Cloud benchmark (`hotpath-benchmark.yml`, `hotpath-relay.yml`)

**Triggers:** push to `main` and every pull request; the relay runs on `workflow_run` of
`Benchmark`, always from the default branch. That trigger is otherwise banned by
`check_workflow_security.sh`; the relay holds a scoped exception in
`scripts/workflow_security_allowlist.txt` because it never checks out or runs PR code.

**Jobs:** `upload` (push) builds `vtcode-ui`'s `tui_hotpath` example once with the `x-prod`
profile and `--features profiling,hotpath-cloud,hotpath-callgrind`, runs it natively and then
under pinned Valgrind Callgrind, and uploads the merged report with the job's OIDC token.
`report` (pull requests, forks included) runs the same pair without `id-token` and stores
`report.json` as the `hotpath-report` artifact, which the SHA-pinned hotpath relay uploads.
Policy lives in `hotpath/policy.toml`. Renaming the `Benchmark` workflow silently stops the
relay. See [performance](performance.md#hotpath-cloud-ci).

### Release build contracts

Release runs are not rate-limited by the release script: every invocation may dispatch the
full Linux/Windows build. The release contract tests run with
`./scripts/check.sh contracts` (or directly via `python3 scripts/tests/test_ci_load.py`) and
also in the full `./scripts/check.sh`.

Selective musl/ARM64/Windows omission is an alternative to daily batching and is not enabled:
the release publisher and legacy updater bridge require complete platform coverage by default.

**Required release target matrix** (enforced by `scripts/release.sh`):

| Target                      | Built by                  | Archive                      |
| --------------------------- | ------------------------- | ---------------------------- |
| `x86_64-apple-darwin`       | local (`release.sh`)      | `.tar.gz`                    |
| `aarch64-apple-darwin`      | local (`release.sh`)      | `.tar.gz`                    |
| `x86_64-unknown-linux-gnu`  | `build-linux-windows.yml` | `.tar.gz`                    |
| `x86_64-unknown-linux-musl` | `build-linux-windows.yml` | `.tar.gz`                    |
| `aarch64-unknown-linux-gnu` | `build-linux-windows.yml` | `.tar.gz`                    |
| `x86_64-pc-windows-msvc`    | `build-linux-windows.yml` | `.zip` (required by default) |

`release.sh` derives a raw `compat-vtcode-<v>-<target>.tar.gz.compat` executable from each normal archive. These are the
legacy updater compatibility bridge for v0.141.0-v0.141.4 (see [Update System Guide](../guides/update_system.md)). The
`compat-` prefix is load-bearing: GitHub returns release assets sorted alphabetically by name, and the prefix makes the
compat asset sort before `vtcode-<v>-<target>.tar.gz` so the broken legacy updater picks the raw binary instead of the
gzip archive it cannot extract. The release fails if any required target archive (including Windows by default) is
missing. Set `RELEASE_REQUIRE_WINDOWS=false` only for an emergency macOS/Linux rescue when Windows CI is flaky.

Release-note formatting and fixture checks are documented in the
[changelog ownership guide](release-changelog-ownership.md).

#### macOS signing and Gatekeeper

When Developer ID signing and notarization credentials are configured, both macOS release executables are signed with a
hardened runtime, secure timestamp, and the stable code identifier `com.vinhnx.vtcode`. The release scripts submit each
executable to Apple's notary service and verify its signature and Gatekeeper assessment before publishing the archive or
compatibility executable. The raw executable archive layout stays compatible with Homebrew, `install.sh`, and the
updater. Apple publishes notarization tickets for standalone command-line binaries online but does not allow stapling
tickets to those binaries, so Gatekeeper needs network access when it first checks a new release. See Apple's guides for
[notarizing macOS software](https://developer.apple.com/documentation/security/notarizing-macos-software-before-distribution)
and
[customizing the notarization workflow](https://developer.apple.com/documentation/security/customizing-the-notarization-workflow).
Gatekeeper may still show an informational first-launch dialog for a newly installed version, including a notarized one.

On a Mac with no signing credentials, the release scripts continue to package and publish unsigned, unnotarized macOS
binaries and print a warning. Gatekeeper may warn, block, or ask users to approve a downloaded binary; VT Code cannot
hide or dismiss those macOS security dialogs. This keeps macOS releases available without a paid developer account, with
the tradeoff that first-run security friction remains.

That friction is a documented install step, not a footnote: every desktop install surface (the CLI's
`desktop --install` notice, `scripts/install.sh`, `FIRST-LAUNCH.txt`, the Homebrew cask caveats, release notes, and the
install docs) tells users to open the app once, then click **Open Anyway** in System Settings › Privacy & Security.
macOS only offers that button for about an hour after a blocked launch, and macOS 15+ dropped the right-click › Open
bypass entirely, so the guidance must name the Settings route. `src/cli/desktop.rs` and
`scripts/tests/test_install_desktop.sh` pin the wording.

To enable signing and notarization, the Mac running the release needs a Developer ID Application certificate in its
keychain and a `notarytool` keychain profile. Create the profile with `xcrun notarytool store-credentials`, then set the
profile name and full signing identity before releasing:

```bash
export VTCODE_MACOS_SIGNING_IDENTITY='Developer ID Application: Name (TEAMID)'
export VTCODE_MACOS_NOTARY_PROFILE='VTCodeNotary'
./scripts/release.sh --patch
```

Leave both variables unset to publish unsigned macOS binaries. If either variable is set, both must be configured
correctly; a partial or invalid configuration fails rather than silently falling back to unsigned artifacts. Linux and
Windows artifacts do not use this macOS-only signing path.

**Binary size & cold-start optimization:**

All release profiles inherit `[profile.release]` which uses `opt-level = "z"` (size optimization) + full LTO +
`codegen-units = 1`. Binary size directly impacts cold-start time — dyld page-faults loading the Mach-O dominate the
first-launch latency.

| Build path                 | Profile                                           | Extra size flags                                 |
| -------------------------- | ------------------------------------------------- | ------------------------------------------------ |
| macOS local (`release.sh`) | `release`                                         | `-Wl,-dead_strip` via `CARGO_TARGET_*_RUSTFLAGS` |
| macOS CI (desktop .app)    | `release-fast` (thin LTO, 4 codegen units)        | `-Wl,-dead_strip` (same as local)                |
| Linux CI                   | `release-fast` (thin LTO, 4 codegen units)        | `-Wl,--gc-sections` via `RUSTFLAGS`              |
| Windows CI                 | `release-fast-windows` (no LTO, 16 codegen units) | MSVC `/OPT:REF` (default)                        |

`release` keeps `codegen-units = 1` with fat LTO: less IR to merge, smaller binary, and lower peak RSS
despite losing intra-crate parallelism (see [uv#22303](https://github.com/astral-sh/uv/pull/22303): -17%
size, -48-60% PGO build time, neutral runtime; savings dominated by the PGO-instrumented stage).
`release-fast`/`release-fast-windows` intentionally keep higher codegen units because thin/no LTO has
far lower merge cost, so parallelism still wins there. The opt-in
[local PGO experiment](../../scripts/perf/README.md#pgo-experiments) uses `codegen-units = 1` for both instrumented
and final builds. It does not alter CI or release packaging; adoption requires representative training and measured
baseline/candidate comparisons.

`release.sh` also runs a cold-start spot check (fresh `/tmp` copy → `--version` timing) after the macOS aarch64 build to
catch sub-1s regressions before shipping. All build commands use `--locked` to ensure the Cargo.lock matches Cargo.toml
so the size-optimized profiles are actually applied.

Homebrew formula transformations are shared by the three release scripts through `scripts/homebrew-formula.sh`;
`scripts/homebrew-tap.sh` owns checksum lookup, local commits, and tap publication for the active `release.sh` path.
All four digests (Intel/ARM macOS and Intel/ARM GNU Linux) must be present and valid before the formula version changes.
The helper also rejects missing or duplicate formula entries before writing. The same helper updates the Apple Silicon
desktop cask from `vtcode-desktop-<version>-aarch64-apple-darwin.zip.sha256`; the cask is published to
`vinhnx/homebrew-tap/Casks/vtcode-desktop.rb` and skipped when a release has no desktop archive. Run the fixture
checks without publishing:

```bash
python3 scripts/tests/test_homebrew_formula.py
python3 scripts/tests/test_homebrew_cask.py
```

### 3. Release (driven by `scripts/release.sh`, no dedicated workflow)

**Flow:**

- **Version & tag**: `cargo-release` bumps, tags, and pushes
- **Build Binaries**: Dispatches `build-linux-windows.yml` for Linux/Windows; macOS built locally
- **Create Release**: Drafts GitHub Release with changelog
- **Publish**: Publishes to crates.io and Homebrew

Crate publishing runs through `scripts/publish_extracted_crates.sh`. The script lists the crates to publish;
the order they are published in is derived from the workspace dependency graph (`scripts/publish_order.py`
resolves `cargo metadata` and topologically sorts the list) because crates.io rejects a crate whose
workspace dependency is not yet live. Adding a dependency edge therefore cannot silently invalidate the
release sequence. `--start-from <crate>` resumes a partially completed release and refuses to start when a
prerequisite of the selected crate is missing from crates.io. The derivation is covered by:

```bash
python3 scripts/tests/test_publish_order.py
```

### 4. CI quality gate (`ci.yml`)

**Triggers:** every pull request, pushes to `main`, and manual dispatch.

**Jobs (parallel, no `needs:` edges):**

- **fast-gates** (seconds): `cargo fmt --all -- --check` plus the governance gates
  sourced from `scripts/common.sh` (`run_repo_governance_gates`), the structured
  logging lint, and the agent legibility lint. Every step runs even after another
  fails, so one red run lists every failure.
- **clippy**: `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
  — the same workspace lint gate `scripts/check.sh` runs locally.
- **tests**: `cargo nextest run --locked --profile ci-partition`. Pushes to main and
  manual dispatches run `--workspace`; pull requests run only the packages the diff
  can reach via `scripts/select_tests.py` (see [test selection](#test-selection-on-pull-requests)).
- **platform-smoke**: a bounded macOS (`macos-15`) and Windows (`windows-latest`)
  nextest matrix. Both runners execute the sandbox path-policy tests
  (`vtcode-safety::sandboxing::policy`) and the platform-neutral TUI/tool-schema
  snapshot binaries in `vtcode`; macOS additionally runs the `#[cfg(unix)]` PTY
  harness (`vtcode-core::pty_tests`). Toolchain pin, pinned nextest version,
  ripgrep, and `profile.ci-partition` match the Linux `tests` job, with a per-OS
  rust-cache key. The Linux-only `VTCODE_LINUX_SANDBOX_EXECUTABLE` hook is not set
  because the selected tests do not need a sandbox launcher.

**Contract:**

- `profile.ci-partition` sets `retries = 0`: a test that passes only on retry has a
  bug, and retrying hides it. It also sets `fail-fast = false`, so all failures list.
- Pushes to `main` run the full suite and seed the `rust-cache` lineage that PR
  jobs restore (cache lookup falls back to the default branch), keeping PR cache
  hit rates high without a separate seeding workflow.
- Concurrency cancels a branch's in-progress run when a new commit lands.
- Platform coverage is deliberately bounded. The full suite stays on Linux;
  `platform-smoke` runs a small cross-platform subset on macOS and Windows to
  catch shared-code regressions early without letting platform quirks in
  unrelated tests block PRs. `desktop-macos.yml` and the release build workflow
  cover the desktop and release paths on those platforms.

### Test selection on pull requests

`scripts/select_tests.py` maps the PR diff to packages and takes the transitive
reverse-dependency closure (normal + dev + build edges), so the tests a diff can
affect all run and nothing else does. The pure closure is covered by
`python3 scripts/tests/test_select_tests.py`.

**Inputs that rerun everything** (checked first, fail-safe direction):
any `Cargo.toml` (manifest re-resolution), `Cargo.lock`, `rust-toolchain.toml`,
`.cargo/config.toml`/`.cargo/config`, `.github/workflows/ci.yml`,
`.config/nextest.toml`, and `scripts/select_tests.py` itself. Docs- and
scripts-only diffs select no packages: files outside a package's source
locations cannot change test outcomes.

The base diff is two-dot on purpose: if `main` moved past the recorded base sha,
its changes also appear in the selection — over-selection never hides a test.
The proof obligation when changing the selection logic: hide a real change from
it on purpose (commit an upstream break and confirm the dependent crate's tests
still go red) before merging.

### Test sharding

There are no test shards yet, on purpose: sharding before measuring multiplies
cold builds (each shard compiles its own test binaries). The plan, applied once
the full-suite job is the bottleneck:

1. `ci.yml` records per-test times on every `main` run via
   `profile.ci-partition.junit` and uploads `nextest-junit` (7-day retention).
2. Balance shards from those recorded times with nextest's partition support
   (`--partition`), not test counts.
3. Prove the shards together ran every file, and split any test file over 20
   seconds.

### 5. Dependency audit (`deny.yml`)

**Triggers:** pull requests and pushes to `main` touching any `Cargo.toml`/`Cargo.lock`/`deny.toml`
or the workflow itself, a weekly schedule (Mondays 05:00 UTC, default branch only — re-audits the
committed lockfile even when no PR touches it, so newly published advisories fail without a code
change), and manual `workflow_dispatch`.

**Jobs:** one `cargo-deny` job on `ubuntu-latest` runs `cargo-deny-action` (SHA-pinned) with
`--locked`. The action provisions its own pinned toolchain inside its Docker image, so no
`dtolnay/rust-toolchain` step is needed and no Cargo compilation cache is used — it only resolves
the committed lockfile. Policy lives in `deny.toml` at the repo root; the same policy runs locally
via `./scripts/check.sh deny` and opt-in via `./scripts/check-dev.sh --deny`.

**Policy summary** (full rationale comments live in `deny.toml`):

- **Advisories**: RustSec vulnerabilities and yanked crates fail the check. The ten unmaintained
  advisories whose dependency chains are already at their latest upstream releases carry explicit
  `ignore` entries with reasons (syntect → bincode/yaml-rust, image/exr → paste, notify-rust →
  zvariant family, gpui/resvg stack) and must be re-evaluated when a parent crate releases.
- **Licenses**: the allow list mirrors `about.toml` (`accepted`) so cargo-deny and THIRD-PARTY-NOTICES
  generation share one policy; adding a license means updating both files.
- **Bans**: duplicate crate versions warn (visible drift, non-blocking).
- **Sources**: crates.io only; git sources are fail-closed (`unknown-git = "deny"`) because a git
  URL has no registry checksum database — provenance (pinned rev, owner, signed commits) must be
  reviewed and the URL added to `allow-git` before it enters the graph.
- **Targets**: `[graph] targets` lists every shipped target triple (the release build matrix plus
  macOS), so target-gated dependencies such as the notify-rust/clipboard stacks are always audited.

## Code Quality Tools

### rustfmt

**Installation:**

```bash
rustup component add rustfmt
```

**Usage:**

```bash
# Check formatting
cargo fmt --all -- --check

# Auto-format code
cargo fmt --all

# Print current configuration
cargo fmt --print-config default rustfmt.toml
```

**Configuration:** Create a `rustfmt.toml` or `.rustfmt.toml` file in your project root:

```toml
edition = "2021"
max_width = 100
tab_spaces = 4
```

### clippy

**Installation:**

```bash
rustup component add clippy
```

**Usage:**

```bash
# Run clippy with warnings as errors
cargo clippy -- -D warnings

# Run on specific target
cargo clippy --lib

# Fix clippy suggestions automatically
cargo clippy --fix
```

**Common clippy lints:**

- `clippy::all`: Enable all lints
- `clippy::pedantic`: More strict lints
- `clippy::nursery`: Experimental lints
- `clippy::cargo`: Cargo.toml specific lints

### First-party debt scan

The lint migration keeps the actionable marker scan separate from generated or fixture content. Run it from the
repository root:

```bash
./scripts/first-party-debt-scan.sh
```

The scanner covers first-party `src/`, `crates/`, and `scripts/` content while excluding vendored, generated, fixture,
template, sample, and task-panel content. New `TODO:`, `FIXME:`, `HACK:`, or `XXX:` markers fail the check.

The workspace lint gate also enforces the previously suppressed result, indexing, string-slice, cast, and
allow-without-reason lint families:

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo nextest run --locked --workspace
```

## Local Development

### Development Check Script

Use the provided development check script to run the same checks locally:

```bash
# Run all checks
./scripts/check.sh

# Run specific checks
./scripts/check.sh fmt      # Format check
./scripts/check.sh clippy   # Clippy check
./scripts/check.sh test     # Run tests
./scripts/check.sh build    # Build project
./scripts/check.sh docs     # Generate docs
./scripts/check.sh unsafe-docs # Gate unsafe SAFETY/allowance + type-position contract docs
./scripts/check.sh deny     # cargo-deny policy (RustSec advisories, licenses, bans, sources)
./scripts/check.sh contracts # Release-artifact contract tests
```

### Repository Governance Gates

The governance gates run through both the local gate scripts and CI: `ci.yml`'s
fast-gates job sources the same `scripts/common.sh` functions
(`run_repo_governance_gates`) plus the structured logging and agent legibility
lints, so local and hosted gates cannot drift. Markdown lint stays local-only
(it needs Node.js 22+ with `npx`; run `npm install` so the pinned linter resolves
locally). Run `./scripts/check-dev.sh` before every commit and `./scripts/check.sh`
before a release:

| Function | Checks | Runs in |
| --- | --- | --- |
| `run_repo_governance_gates` | file length, docs/AGENTS.md links, top-level docs placement, tracked file sizes, out-of-scope MCP provider docs | `check-dev.sh`, `check.sh` |
| `run_markdown_lint_gate` | `scripts/check_markdown.py` (needs Node.js 22+ with `npx`; run `npm install` so the pinned linter resolves locally) | `check-dev.sh --lints`, `check.sh` |
| `run_check` / `run_clippy` | compilation plus Cargo's built-in `unused_dependencies` manifest lint (warn-level, configured in root `Cargo.toml`) | `check-dev.sh`, `check.sh` |

Unused dependencies are reported by Cargo 1.100's built-in `cargo::unused_dependencies` manifest
lint — no external tool. It is `warn` (not `deny`) because the root binary's mutually exclusive
allocator features make an `--all-features` build compile neither allocator static; the full
rationale is in root `Cargo.toml`. The lint surfaces in the existing
`cargo clippy --all-targets --all-features` gate (it needs `--all-targets` to see every target's
usage), so review those warnings rather than expecting a separate gate step. Plain `cargo check`
without `--all-targets` will not reliably surface them. A dependency whose only call site is feature-gated must be declared
`optional = true` with `dep:<name>` added to that feature, otherwise the lint flags it in builds
where the feature is off. See [dependency-bloat-audit](dependency-bloat-audit.md) for the audit that
introduced and later retired the external check.

Do not describe these checks as CI-enforced. If a CI job is added later, it should call the same
`scripts/common.sh` functions rather than re-implementing the checks in workflow YAML.

### Manual Setup

To set up the development environment manually:

```bash
# Install required components
rustup component add rustfmt clippy

# Install additional tools
cargo install cargo-audit      # Security auditing
cargo install cargo-outdated   # Dependency checking
# Unused dependencies: use Cargo 1.100's built-in `unused_dependencies` lint (root Cargo.toml)
cargo install cargo-msrv       # MSRV checking
cargo install cargo-license    # License checking
cargo install cargo-tarpaulin  # Code coverage
```

## Best Practices

### 1. Git Hooks

Fast local enforcement, installed via `core.hooksPath` (a hook, not a line in
AGENTS.md, because agents skip steps):

```bash
./scripts/install-hooks.sh
```

| Hook                       | Runs                                                                                                                                            | Budget       |
| -------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------- | ------------ |
| `scripts/hooks/pre-commit` | Compare staged `.rs` blobs against rustfmt output using the staged config + `run_repo_governance_gates` + docs-map regeneration for staged docs | ~2s          |
| `scripts/hooks/pre-push`   | `./scripts/check-dev.sh --quiet --workspace` (the CI fast gate, workspace scope)                                                                | ~10-30s warm |

Tests are deliberately not repeated in hooks: CI runs the full suite on `main`
and the diff-reachable selection on PRs, so a local re-run would only add the
waiting the gate design removes. Remove the hooks with
`git config --unset core.hooksPath`.

### 2. Editor Integration

#### VS Code

Add to `.vscode/settings.json`:

```json
{
  "rust-analyzer.checkOnSave.command": "clippy",
  "editor.formatOnSave": true,
  "rust-analyzer.rustfmt.enableRangeFormatting": true
}
```

#### Vim/Neovim

```vim
autocmd BufWritePre *.rs :silent! !cargo fmt -- %:p
```

### 3. IDE Integration

Most Rust IDEs support rustfmt and clippy:

- **IntelliJ/CLion**: Built-in Rust plugin
- **VS Code**: rust-analyzer extension
- **Vim**: rust.vim plugin
- **Emacs**: rustic-mode

## CI/CD Configuration

### Branch Protection

Configure branch protection rules in GitHub:

1. Go to repository Settings → Branches
2. Add rule for `main`/`master` branch
3. Require the status checks your branches rely on. `ci.yml`'s jobs (`Fast gates
   (fmt + governance)`, `Clippy (workspace, -D warnings)`, `Tests (nextest)`) are
   the quality gate; require them when the branch-protection settings are
   updated. `build-linux-windows.yml` is `workflow_dispatch`/`workflow_call`
   only, so add checks for whichever workflows you run on pull
   requests (for example `webmcp.yml` for browser-editor changes).

### Status Badges

Add these badges to your README:

```markdown
[![Build Linux and Windows](https://github.com/yourusername/vtcode/actions/workflows/build-linux-windows.yml/badge.svg)](https://github.com/yourusername/vtcode/actions/workflows/build-linux-windows.yml)
[![WebMCP](https://github.com/yourusername/vtcode/actions/workflows/webmcp.yml/badge.svg)](https://github.com/yourusername/vtcode/actions/workflows/webmcp.yml)
```

## Troubleshooting

### Common Issues

#### rustfmt not found

```bash
rustup component add rustfmt
rustup update
```

#### clippy warnings not showing

```bash
cargo clippy -- -W clippy::all
```

#### MSRV issues

```bash
cargo msrv --workspace
cargo msrv --workspace set 1.100.0  # Set specific version
```

#### Dependency issues

```bash
cargo update
cargo outdated
# Unused deps: Cargo 1.100's built-in lint reports them as warnings
cargo check --workspace --all-targets
```

### Performance Optimization

#### Faster CI builds

```yaml
# In workflow
- uses: actions/cache@v3
  with:
    path: |
      ~/.cargo/registry
      ~/.cargo/git
      target
    key: ${{ runner.os }}-cargo-${{ hashFiles('**/Cargo.lock') }}
```

#### Parallel jobs

```yaml
strategy:
  matrix:
    os: [ubuntu-latest, macos-latest, windows-latest]
```

#### Parallel steps within one job (GitHub Actions `parallel` / `background`)

Since June 2026, steps in a single job can run concurrently on the same runner while keeping
separate logs. This repo uses `parallel:` for independent, I/O-bound steps in the same job:

```yaml
steps:
  - parallel:
      - name: Typecheck browser editor
        working-directory: apps/webmcp
        run: bun run typecheck
      - name: Test browser editor
        working-directory: apps/webmcp
        run: bun run test
```

Rules applied in this repo (see
[workflow syntax](https://docs.github.com/en/actions/reference/workflows-and-actions/workflow-syntax)
and the [June 2026 changelog](https://github.blog/changelog/2026-06-25-actions-steps-can-now-be-run-in-parallel/)):

- Use `parallel:` for self-contained groups (lint + typecheck + python checks). It is shorthand for
  `background: true` + implicit `wait`. Use `background:` + `wait:`/`wait-all`/`cancel:` only when
  you need fine-grained control (long-running service, overlapping foreground work).
- Limits: max 10 concurrent background steps per job; `wait`/`wait-all`/`cancel` always run and do
  not support `if:`; `background` cannot be declared inside a composite action (a composite action
  itself may run as a background step); outputs/env from a background step are visible only after
  `wait`/`wait-all`; implicit `wait-all` runs before post-job cleanup.
- Prefer `parallel:` for I/O-wait overlap (python linters, `bun run typecheck` + `bun run test`).
  Consider job-level `matrix` across runners for CPU-bound Rust builds only after measuring runner costs and timings.
  Same-runner builds compete for available CPUs, and concurrent Cargo invocations on the same `target/` directory
  contend on the Cargo package/target lock.
- Do not parallelize steps that share one output file (e.g. two harnesses `tee`ing to the same log)
  or that both invoke `cargo` on the same workspace (e.g. `cargo build` + `cargo tree`).

Current use: `webmcp.yml` typecheck + test.

`webmcp.yml` restores dependencies before running its `parallel:` typecheck + test group, so the
barrier finishes before the build step. Keep `cache-bin: false` when cache restoration overlaps a
tool installer, so a concurrent install does not write binaries into a directory the cache step is
restoring.

This applies the batching, bounded-concurrency, and shared-resource lessons from
[Principles for fast Tokio applications](https://dial9-rs.github.io/blog/principles-for-fast-tokio-applications/)
and [uv PR #21372](https://github.com/astral-sh/uv/pull/21372) to CI scheduling. It is an analogy, not a Tokio runtime
optimization.

To validate a scheduling change, check dependency barriers, action inputs, isolated output paths, and
failure propagation before comparing hosted job/step durations on the same commit and cache state.
Separate queue/setup time, compilation time, and test runtime. Do not infer a speedup from local Rust
runtime benchmarks or from a shorter YAML file.

## Security

### Dependency Auditing

The primary auditor is **cargo-deny** (policy in `deny.toml`, enforced in CI by `deny.yml` and
locally by the check scripts). It covers RustSec advisories, yanked crates, license compliance,
duplicate dependencies, and registry sources in one pass:

```bash
# Install cargo-deny
brew install cargo-deny   # or: cargo install cargo-deny --locked

# Run the full policy (advisories, licenses, bans, sources)
cargo deny --locked check

# House shortcuts
./scripts/check.sh deny        # same policy as CI
./scripts/check-dev.sh --deny  # opt-in from the fast gate
```

RustSec vulnerabilities or a yanked crate fail the check. Unmaintained advisories that cannot be
fixed from this repo (dependency chain already at the latest upstream release) carry explicit,
reason-annotated `ignore` entries in `deny.toml`; each one names its blocking parent crate and must
be re-evaluated when that parent releases. Adding a new dependency license means updating the
`allow` list in `deny.toml` **and** the `accepted` list in `about.toml` — they mirror each other.

**cargo-audit** remains available as a secondary, lockfile-wide cross-check (it scans raw lockfile
entries that cargo-deny's graph resolution may not reach):

```bash
cargo install cargo-audit
cargo audit
cargo audit fix   # applies compatible version bumps when patched versions exist
```

### Binary Audit Data

The release builds embed the dependency list into each binary via
[cargo-auditable](https://github.com/rust-secure-code/cargo-auditable) (compressed `.dep-v0` section,
no runtime overhead), so shipped artifacts can be scanned directly:

```bash
cargo audit bin target/release/vtcode   # RustSec scan of a shipped binary
```

The `x86_64-unknown-linux-gnu`, `x86_64-pc-windows-msvc` jobs in `build-linux-windows.yml` and the
macOS builds in `scripts/release.sh` use `cargo auditable build`; `scripts/release.sh` falls back to
a plain build with a warning when the tool is absent. Two channels do not currently carry audit data:

- `cross`-built musl/aarch64 artifacts: cross runs cargo inside its container, so injecting would
  require custom cross images (see the comment on the cross build step).
- Windows MSVC (`x86_64-pc-windows-msvc`): the Windows job is built natively with
  `cargo auditable build`, but Rust 1.100's Cargo build-dir layout change drops the injected
  `.dep-v0` object during the MSVC link. This started at v0.182.3 (the 1.99.1 → 1.100.0 toolchain
  bump); v0.182.1 and earlier embedded the section. No cargo-auditable release fixes it yet. The
  build stays auditable-enabled so the data returns automatically once upstream is fixed — do not
  replace the Windows build with a plain `cargo build`.

`report_audit_data_coverage` reports these three known gaps as `MISSING (expected/known limitation)`
rather than a regression `WARNING`. Verify any artifact with `cargo audit bin` after building if the
data matters for that channel.

### License Compliance

```bash
# Check licenses (quick overview)
cargo install cargo-license
cargo license --workspace

# Regenerate THIRD-PARTY-NOTICES from Cargo.lock (full automation)
cargo install --locked --features cli cargo-about
scripts/generate-notices.sh          # regenerate the file
scripts/generate-notices.sh --check  # CI mode: exit 1 if out of date
```

Run `scripts/generate-notices.sh --check` before merge to catch stale license notices. The file has a manual header
(`scripts/templates/third-party-header.txt` for in-tree source ports) and an auto-generated dependency listing
(`scripts/templates/third-party-notices.hbs` via cargo-about).

## References

- [rustfmt Documentation](https://rust-lang.github.io/rustfmt/)
- [clippy Documentation](https://rust-lang.github.io/rust-clippy/)
- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [Cargo Documentation](https://doc.rust-lang.org/cargo/)
