# Rust Toolchain and MSRV

`rust-toolchain.toml` is the single source of truth for the compiler used by every surface: local `cargo`, CI,
Docker images, Clippy, and the `rust-version` floor that crate manifests advertise. This guide is the checklist for
moving that pin.

## Policy

- The workspace targets **one exact stable release**. `channel` is a concrete `x.y.z`, and every `rust-version`
  matches it, including patch releases (`1.99.1`, not `1.99`).
- The MSRV *is* that value. Crates fail closed on older compilers instead of silently compiling with a toolchain CI
  and Docker never test. Do not introduce a second (lower) MSRV floor without replacing the pin guard.
- Edition 2024 and the dependency floors (ratatui, darling, sysinfo, zip) are documented in
  [COMPATIBILITY](../COMPATIBILITY.md).
- Local runtime state and vendored forks are never part of the pin: `target/`, `.worktrees/`, `.vtcode/`, `patches/`,
  `fuzz/`, and `extensions/` are excluded by the guards. `.vtcode/perf/` snapshots copy whole workspaces and keep the
  toolchain they were audited with.

## Pin map

| Surface | File | Guard |
| ------- | ---- | ----- |
| Compiler channel | `rust-toolchain.toml` | `tests/toolchain_pins_test.rs` (`pinned_toolchain`) |
| Clippy MSRV | `clippy.toml` (`msrv`) | pin test + `scripts/check_compatibility.sh --msrv` |
| Workspace floor | `Cargo.toml` (`[package]`, `[workspace.package]`) | pin test |
| Crate floors | each crate `Cargo.toml` (`rust-version` or `rust-version.workspace = true`) | pin test |
| Build images | `Dockerfile.build`, `.oss-scanner/Dockerfile` | pin test (build image only); keep both in sync |
| CI | every `dtolnay/rust-toolchain` step in `.github/workflows/*.yml` | pin test (`workflow_toolchain_pins`) |
| Human report | `scripts/check_compatibility.sh --msrv` | script compares every pin to the channel |
| Auxiliary analyzer | `scripts/hawk.sh` (`HAWK_TOOLCHAIN`, `HAWK_VERSION`) | manual, see below |

The pin test scans the workspace, so manifests, Docker, and workflows cannot drift from `rust-toolchain.toml`
without failing `cargo nextest run`. Documentation pins (README badge, guides, issue templates) are not guarded;
update them in the same change.

## Bump checklist

1. Install the release: `rustup toolchain install X.Y.Z`.
2. Update the pin map. A version-wide search finds the current value:
   `rg -n '<old version>' --hidden -g '!target' -g '!.git' -g '!.vtcode/**' .`
   Update the channel, `clippy.toml`, the workspace and crate manifests, both Dockerfiles, every workflow
   `toolchain:` input, the compatibility script's expected value, and the docs listed above.
3. Keep the hawk pin consistent with its upstream release (see [Auxiliary compiler pins](#auxiliary-compiler-pins)).
4. Reformat: `cargo fmt --all` — a new rustfmt can reflow code that was previously stable.
5. Run the fast gate: `./scripts/check-dev.sh --lints`.
6. Run the full lint gate: `cargo clippy --workspace --all-targets --all-features -- -D warnings`.
7. Confirm the pins: `cargo nextest run --locked -p vtcode --test toolchain_pins_test` and
   `./scripts/check_compatibility.sh --msrv`.
8. Run the affected tests, then broaden: `cargo nextest run --locked -p vtcode -p vtcode-core -p vtcode-llm` and finally
   the workspace suite.
9. If the change adds or removes a documentation page, regenerate the embedded map with
   `python3 scripts/generate_docs_map.py`.

## Compiler and Clippy fallout

A compiler bump promotes deprecations and new lints into the `-D warnings` gate. Fix them at the call site; do not
add blanket crate-level allows unless the intent matches an existing documented allow.

| Release | Gate change | Action |
| ------- | ----------- | ------ |
| 1.99 | `Atomic::*::fetch_update` deprecated in favor of `try_update` | Rename call sites; `Result` contract is unchanged |
| 1.99 | Constant float comparisons lint as `clippy::float_cmp_const`, separate from `clippy::float_cmp` | Allow both where an exact constant is under test; drop expectations that no longer fire |
| 1.98 | `unnecessary_sort_by`, `manual_checked_ops`, `chunks_exact`, `result_large_err`, and related lints | Run `cargo clippy --fix` for autofixes, then review boxed error paths by hand |

Find the full list before editing:

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo report future-incompatibilities       # dependency-level warnings
```

When investigating a suspected compiler regression without changing the pin, run the older toolchain explicitly with
`cargo +<version> check --ignore-rust-version`. That flag is for diagnosis only; the committed floor stays exact.

## Auxiliary compiler pins

- **hawk** links rustc internals and only runs under the exact compiler its build shipped with.
  `scripts/hawk.sh` pins `HAWK_TOOLCHAIN` and `HAWK_VERSION` separately from the workspace. Check the release notes for
  the rustc it tracks. When the archive lags the workspace pin (for example 0.1.15 tracks rustc 1.99.0 while the
  workspace moved to 1.99.1), a 1.99.1 floor makes `cargo +1.99.0` refuse the workspace, so use the from-source build
  in the script's error text and keep the comment accurate until upstream publishes a matching archive.
- **Nightly checks** are separate channels (`toolchain: nightly` or `nightly-YYYY-MM-DD`). The pin test skips them on
  purpose; never point a nightly step at the stable pin.
- **`cargo-msrv`** is optional tooling for reporting (`cargo msrv verify`). CI does not require it, and it must agree
  with the exact patch pin rather than discovering a lower floor.

## Do not rewrite history

Recorded evidence keeps the compiler it was produced with:

- Session fixtures under `src/agent/runloop/**/tests/` and `crates/codegen/vtcode-core/src/core/agent/state/fixtures/`
  replay real transcripts, including the toolchain line they captured.
- Dated measurements in [performance](performance.md) and
  [execution-policy matching](execution-policy-matching.md) name the compiler used for the numbers.
- Published posts under `docs/blog/` keep their original claim.

## Verification summary

| Check | Command |
| ----- | ------- |
| Formatting | `cargo fmt --all -- --check` |
| Fast gate | `./scripts/check-dev.sh --lints` |
| Full lints | `cargo clippy --workspace --all-targets --all-features -- -D warnings` |
| Pin agreement | `cargo nextest run --locked -p vtcode --test toolchain_pins_test` |
| MSRV report | `./scripts/check_compatibility.sh --msrv` |
| Workspace tests | `cargo nextest run --locked` |
| Unsafe docs gate | `./scripts/check.sh unsafe-docs` |
