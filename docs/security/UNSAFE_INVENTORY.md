# Unsafe Code Inventory

This is the maintainer inventory for VT Code's intentional unsafe Rust and FFI boundaries. It complements the compiler
and Clippy lint families in the root `Cargo.toml` (`unsafe_code`, `undocumented_unsafe_blocks`,
`multiple_unsafe_ops_per_block`).

## Snapshot: 2026-08-10

The external audit reported 44 textual `unsafe` occurrences. That count also includes prose, diagnostics, and test
descriptions. The current source inventory below contains 24 syntactic unsafe sites: unsafe operations,
`unsafe extern "C"` ABI declarations, the two allocator link attributes, and test-only FFI callbacks.

| Source                                                                                                         |  Sites | Boundary and invariant                                                                                                                                                                                                                                     |
| -------------------------------------------------------------------------------------------------------------- | -----: | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| [`src/allocator.rs`](../../src/allocator.rs)                                                                   |      3 | `MallocConfPtr` shares an immutable, process-lifetime byte array; the exported jemalloc symbols use the platform ABI names and are only compiled for the matching allocator/platform configuration.                                                        |
| [`crates/common/vtcode-commons/src/env_lock.rs`](../../crates/common/vtcode-commons/src/env_lock.rs)           |      2 | `set_var` and `remove_var` are called only while `EnvGuard` owns the process-wide mutex, serializing all in-process environment mutation.                                                                                                                  |
| [`crates/common/vtcode-commons/src/memory.rs`](../../crates/common/vtcode-commons/src/memory.rs)               |      4 | macOS Mach calls use a zeroed ABI struct, the current task port, valid output pointers, and the expected count; Linux `sysconf` receives a compile-time selector.                                                                                          |
| [`crates/codegen/vtcode-skills/src/native_plugin.rs`](../../crates/codegen/vtcode-skills/src/native_plugin.rs) |     14 | Native plugins cross a C ABI: function signatures are fixed, libraries are canonicalized under trusted roots, returned pointers are non-null NUL-terminated strings, plugin deallocation uses the matching callback, and non-reentrant plugins are locked. |
| [`crates/codegen/vtcode-bash-runner/src/pipe.rs`](../../crates/codegen/vtcode-bash-runner/src/pipe.rs)         |      1 | The Unix `pre_exec` hook calls only the async-signal-safe `nix` process-group wrapper before `exec`.                                                                                                                                                       |
| **Total**                                                                                                      | **24** |                                                                                                                                                                                                                                                            |

## Native-plugin site breakdown

The 14 sites in `native_plugin.rs` are intentionally grouped by ABI boundary:

- Four `unsafe extern "C"` function-pointer type declarations define the plugin ABI (`PluginVersionFn`,
  `PluginMetadataFn`, `PluginExecuteFn`, and `PluginFreeStringFn`).
- `CStr::from_ptr` and the plugin free callback copy and then release a validated plugin-owned string.
- `Library::get` resolves only the fixed symbols from a canonicalized trusted library.
- The ABI version and metadata callbacks are called only after symbol loading; the execute callback receives a live
  `CString` and is serialized unless the plugin declares itself reentrant.
- `Library::new` is the privileged dynamic-library load and remains limited to canonical paths under trusted plugin
  directories.
- Test-only callbacks cover the matching free operation and delayed execution; their pointers are produced and reclaimed
  by the test fixtures.

## Change rule

Before adding an unsafe site:

1. Prefer a safe standard-library or crate wrapper.
2. Keep the smallest possible unsafe block and add a nearby `SAFETY:` comment stating the invariant, not merely that the
   operation is "trusted."
3. Add or update the corresponding row and site breakdown here.
4. Add a focused regression test for the invariant and run the relevant safety crate tests plus Clippy with
   `-D warnings`.

Unsafe code is not a substitute for command or sandbox policy. FFI plugins are trusted native code and execute with user
privileges; the command/sandbox boundary remains governed by `vtcode-safety` and `vtcode-bash-runner`.
