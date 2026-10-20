# CI/CD and Code Quality

This document describes the CI/CD pipeline and code quality tools used in the vtcode project.

## GitHub Actions Workflows

The project runs two GitHub Actions workflows.

### 1. Build Linux & Windows (`build-linux-windows.yml`)

**Triggers:**

- Manual `workflow_dispatch` with release tag input
- Dispatched from `scripts/release.sh` Step 3.5 via `gh workflow run`

**Rust toolchain:** all three Rust setup steps explicitly select the version in
`rust-toolchain.toml`, currently 1.98.1. The [pinned setup action](https://github.com/dtolnay/rust-toolchain/blob/29eef336d9b2848a0b548edc03f92a220660cdb8/action.yml#L12)
otherwise defaults to floating `stable`; pinning its commit alone does not pin the compiler.
Update the workflow inputs alongside the canonical toolchain version.
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
  immediately; an empty artifact also fails the job, so a release can never ship an archive without man
  pages. Cross uses its container toolchains; no host Zig installation is needed.
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

| Command | Output |
| --- | --- |
| `vtcode man` | Main page on stdout |
| `vtcode man chat --output chat.1` | One command page in a file |
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
legacy updater compatibility bridge for v0.141.0-v0.141.4 (see [Update System Guide](../guides/UPDATE_SYSTEM.md)). The
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

Homebrew formula transformations are shared by the three release scripts through `scripts/homebrew-formula.sh`.
All four digests (Intel/ARM macOS and Intel/ARM GNU Linux) must be present and valid before the formula version changes.
The helper also rejects missing or duplicate formula entries before writing. Callers retain ownership of checksum
retrieval, commits, and tap publication. Run the fixture checks without publishing:

```bash
python3 scripts/tests/test_homebrew_formula.py
```

### 3. Release (driven by `scripts/release.sh`, no dedicated workflow)

**Flow:**

- **Version & tag**: `cargo-release` bumps, tags, and pushes
- **Build Binaries**: Dispatches `build-linux-windows.yml` for Linux/Windows; macOS built locally
- **Create Release**: Drafts GitHub Release with changelog
- **Publish**: Publishes to crates.io and Homebrew

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
./scripts/check.sh contracts # Release-artifact contract tests
```

### Manual Setup

To set up the development environment manually:

```bash
# Install required components
rustup component add rustfmt clippy

# Install additional tools
cargo install cargo-audit      # Security auditing
cargo install cargo-outdated   # Dependency checking
cargo install cargo-udeps      # Unused dependencies
cargo install cargo-msrv       # MSRV checking
cargo install cargo-license    # License checking
cargo install cargo-tarpaulin  # Code coverage
```

## Best Practices

### 1. Pre-commit Hooks

Set up pre-commit hooks to run checks before committing:

```bash
# Install pre-commit (if using)
pre-commit install

# Or create .git/hooks/pre-commit manually:
#!/bin/bash
./scripts/check.sh
```

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
3. Require the status checks your branches rely on. `build-linux-windows.yml` is
   `workflow_dispatch`/`workflow_call` only, so add checks for whichever workflows you run on pull
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
cargo msrv --workspace set 1.98.1  # Set specific version
```

#### Dependency issues

```bash
cargo update
cargo outdated
cargo udeps
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

```bash
# Install cargo-audit
cargo install cargo-audit

# Run audit
cargo audit

# Fix vulnerabilities
cargo audit fix
```

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
