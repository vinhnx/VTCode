# CI/CD and Code Quality

This document describes the CI/CD pipeline and code quality tools used in the vtcode project.

## GitHub Actions Workflows

The project uses several GitHub Actions workflows to ensure code quality and automate testing:

### 1. CI Workflow (`ci.yml`)

**Triggers:**

- Push to `main` (filtered by `.rs`, `.toml`, `.lock`, `.yml`, `.json`, `.md`, `scripts/`)
- Pull requests to `main` (same path filters)
- Weekly schedule (Monday 5 AM UTC)
- Manual `workflow_dispatch`

**Jobs:**

- **Format Check (rustfmt)**: Ensures code is properly formatted
- **Lint Check (clippy)**: Runs comprehensive linting with `-D warnings`
- **Test**: Runs `cargo nextest run` on Ubuntu and focused harness regressions using the same Cargo `ci` profile
- **Windows**: Workspace compilation and Clippy, plus focused UI, input, terminal-setup, and detection regressions
- **Security Audit**: Workflow policy, `cargo audit`, and license-notice checks
- **Documentation**: Markdown linting, documentation placement, and core link checks
- **Scheduled/manual checks**: Cross-platform compilation and an advisory nightly smoke check

### 2. Tool Eval Workflow (`tool-eval.yml`)

**Triggers:**

- Push and PR to `main` on `.rs`, `.toml`, `.lock`, `scripts/`, `.github/workflows/`

**Jobs:**

- **Tool Evaluation**: Validates built-in tool behavior and safety gateways
- **Integration tests**: End-to-end tool execution checks

### 3. Build Linux & Windows (`build-linux-windows.yml`)

**Triggers:**

- Manual `workflow_dispatch` with release tag input
- Called from `release.yml` on publish

**Jobs:**

- **Build Linux**: Compiles `x86_64-unknown-linux-gnu`, `x86_64-unknown-linux-musl`, and `aarch64-unknown-linux-gnu`
  binaries
- **Build Windows**: Compiles `x86_64-pc-windows-msvc` binary
- **Upload Artifacts**: Stores compiled binaries + extension-stripped `.sha256` sidecars for release

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

### 4. Coverage (`coverage.yml`)

**Triggers:**

- Push and PR to `main` on `.rs`, `Cargo.toml`, `Cargo.lock`, `coverage.yml`

**Jobs:**

- **Code Coverage**: `cargo tarpaulin` with XML output
- **Coverage Report**: Uploads to code coverage service

### 5. Release (`release.yml`)

**Triggers:**

- Manual `workflow_dispatch` with version tag

**Jobs:**

- **Build Binaries**: Triggers `build-linux-windows.yml`
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
3. Require status checks to pass:
   - `fmt`
   - `clippy`
   - `test`
   - `security-audit`

### Status Badges

Add these badges to your README:

```markdown
[![CI](https://github.com/yourusername/vtcode/actions/workflows/ci.yml/badge.svg)](https://github.com/yourusername/vtcode/actions/workflows/ci.yml)
[![Code Quality](https://github.com/yourusername/vtcode/actions/workflows/code-quality.yml/badge.svg)](https://github.com/yourusername/vtcode/actions/workflows/code-quality.yml)
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

Current uses: `webmcp.yml` typecheck + test, `ci.yml` `large-files` (3 python checks),
`zen-governance` (3 baselines), `lint-markdown` (2 selector tests + lint), and independent Rust setup tasks.

In `ci.yml`, Clippy and scheduled cross-platform checks restore Cargo caches alongside sccache setup. The Ubuntu test
job also installs pinned nextest in that group; Windows overlaps Cargo cache restoration with nextest installation.
Toolchain setup completes first because the cache action needs compiler metadata. These groups have two or three
members, and their implicit barrier finishes before any Cargo command runs. `cache-bin: false` prevents cache
restoration from overwriting binaries that a concurrent installer writes into `~/.cargo/bin`. Cache keys retain their
existing job and target isolation. The nextest installer uses `fallback: none`, so missing prebuilt binaries fail instead
of starting a Cargo build inside the parallel group.

The focused PTY, pipe, and inline-event suites run through one nextest command with
`--profile ci-harness --cargo-profile ci`.
This reuses the main test job's build profile, schedules tests through nextest's bounded runner, and reports all selected
suites even when one fails. It remains a separate step with `if: success() || failure()` so a main-suite failure does not
skip focused regressions. Cargo processes remain sequential within each job to avoid target-directory lock contention.

All CI nextest commands explicitly load the tracked `.github/nextest.toml`; local `.config/nextest.toml` is intentionally
ignored and cannot provide profiles on a clean checkout. `ci-harness`, `ci-windows-ui`, and `ci-windows-terminal` inherit
the nextest `ci` settings. Their separate profile
directories preserve each suite's JUnit report; using `--profile ci` for every sequential command would overwrite earlier
reports, including failure evidence. The Ubuntu test job runs `scripts/tests/test_ci_nextest_reports.py` against the
configured workflow profiles to verify that a passing follow-up suite preserves an earlier failing report.

This applies the batching, bounded-concurrency, and shared-resource lessons from
[Principles for fast Tokio applications](https://dial9-rs.github.io/blog/principles-for-fast-tokio-applications/)
and [uv PR #21372](https://github.com/astral-sh/uv/pull/21372) to CI scheduling. It is an analogy, not a Tokio runtime
optimization. The existing Python check groups already follow this pattern.

To validate a scheduling change, check dependency barriers, action inputs, isolated output paths, nonzero nextest
selection, and failure propagation before comparing hosted job/step durations on the same commit and cache state.
Separate queue/setup time, compilation time, and test runtime. Do not infer a speedup from local Rust runtime benchmarks
or from a shorter YAML file. As of 2026-10-07, the registered CI workflow is `disabled_manually`; source changes and local
checks do not establish a hosted performance result or re-enable the workflow.

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

The `license-notices` CI job runs `scripts/generate-notices.sh --check` on every PR to catch stale license notices
before merge. The file has a manual header (`scripts/templates/third-party-header.txt` for in-tree source ports) and an
auto-generated dependency listing (`scripts/templates/third-party-notices.hbs` via cargo-about).

## References

- [rustfmt Documentation](https://rust-lang.github.io/rustfmt/)
- [clippy Documentation](https://rust-lang.github.io/rust-clippy/)
- [GitHub Actions Documentation](https://docs.github.com/en/actions)
- [Cargo Documentation](https://doc.rust-lang.org/cargo/)
