# Vendored: block

Patched copy of [`block` 0.1.6](https://github.com/SSheldon/rust-block)
(MIT, Steven Sheldon). Wired in via `[patch.crates-io]` at the workspace
root. Upstream is abandoned (0.1.6 is the final release); the ecosystem
successor is `block2`, but `cocoa` (through 0.27), `metal` (0.33), and
`screencapturekit-sys` (0.2.8) still depend on `block` 0.1 with no
`block2`-based releases, so the dependency cannot be upgraded away. It
reaches this workspace via `vtcode-desktop` -> `gpui-kit` -> `gpui-pre` /
`gpui-fast` -> `cocoa` / `metal` / `zed-scap` (macOS only).

## Why

Upstream declares the imported Objective-C class symbol with an uninhabited
type:

```rust
enum Class { }

extern {
    static _NSConcreteStackBlock: Class;
}
```

That trips the `static_of_uninhabited_type` future-compat lint
(rust-lang/rust#74840): every build prints `warning: the following packages
contain code that will be rejected by a future version of Rust: block v0.1.6`,
and the pattern will become a hard error. See
`cargo report future-incompatibilities` for the full report.

## Patch

- `src/lib.rs`: the extern static is now `static _NSConcreteStackBlock:
  c_void`, and the private `BlockBase::isa` field is `*const c_void`; the
  uninhabited `enum Class {}` is removed. `c_void` is inhabited, so the
  lint is gone. The static is only ever addressed (`isa:
  &_NSConcreteStackBlock`, coerced to `*const c_void`) and never read, so
  the change is ABI-identical and touches no public API. This mirrors the
  declaration used by the successor crate `block2`.
- `src/lib.rs`: spelled the ABI explicitly (`extern` -> `extern "C"`) on
  the extern block, the two `unsafe extern fn` definitions, and every
  `unsafe extern fn` pointer type (10 sites, covering all 46 `missing_abi`
  warnings). In edition 2015 bare `extern` already means `"C"`, so this is
  behavior-identical; it keeps workspace builds quiet because path patches
  are not lint-capped the way registry dependencies are.
- `Cargo.toml`: pinned `edition = "2015"` (the implicit upstream default)
  to silence the unspecified-edition manifest note, and dropped the
  dangling `[dev-dependencies.objc_test_utils]`
  (`path = "test_utils"`); the published 0.1.6 crate does not contain that
  directory. Dev-deps of a `[patch]` crate are not resolved for downstream
  builds. Otherwise identical to `Cargo.toml.upstream`, plus the standalone
  `[workspace]` table used by every crate in `patches/`.
