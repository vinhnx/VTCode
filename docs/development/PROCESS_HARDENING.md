# Process Hardening

VT Code implements comprehensive process hardening based on the OpenAI Codex security model. This document outlines the hardening measures applied to protect the VT Code process from various attack vectors.

## Architecture

Process hardening is implemented as a dedicated crate (`vtcode-process-hardening`) that provides a single public function: `pre_main_hardening()`.

This function is called in the binary before `main()` executes using the `#[ctor::ctor]` pattern, ensuring that security hardening happens as early as possible in the process lifecycle.

```rust
#[ctor::ctor]
fn init() {
    vtcode_process_hardening::pre_main_hardening();
}
```

## Security Measures

### Linux/Android

1. **PR_SET_DUMPABLE (ptrace disable)**: Prevents the process from being attached by debuggers or ptrace and disables core dumps at the process level.
   - Exit code on failure: 5

2. **RLIMIT_CORE**: Sets the core file size limit to 0 for defense in depth, preventing core dumps even if process-level controls fail.
   - Exit code on failure: 7

3. **LD_* environment variable removal**: Strips `LD_PRELOAD` and similar dynamic linker variables that could subvert library loading.
   - VT Code is MUSL-linked in release builds, so these are ignored anyway, but removing them provides additional defense.

### macOS

1. **PT_DENY_ATTACH**: Calls `ptrace(PT_DENY_ATTACH)` to prevent debugger attachment.
   - Exit code on failure: 6

2. **RLIMIT_CORE**: Sets the core file size limit to 0 to prevent core dumps.
   - Exit code on failure: 7

3. **DYLD_* environment variable removal**: Removes dynamic linker environment variables like `DYLD_INSERT_LIBRARIES`, `DYLD_LIBRARY_PATH`, etc. that could compromise library loading.

### BSD (FreeBSD, OpenBSD)

1. **RLIMIT_CORE**: Sets the core file size limit to 0.
   - Exit code on failure: 7

2. **LD_* environment variable removal**: Strips dynamic linker environment variables.

### Windows

Placeholder for future Windows-specific hardening (DEP, CFG, etc.).

## Implementation Details

### Non-UTF-8 Environment Variable Handling

The process hardening code uses `std::env::vars_os()` instead of `std::env::vars()` to properly handle environment variables that cannot be decoded as UTF-8. This is critical for robustness:

- `vars_os()` returns `OsString` which can represent invalid UTF-8 sequences
- `vars()` panics if an environment variable contains non-UTF-8 bytes
- This ensures VT Code doesn't crash due to malformed environment variables

### Exit Codes

The following exit codes indicate process hardening failures:

| Exit Code | Meaning |
|-----------|---------|
| 5 | prctl(PR_SET_DUMPABLE) failed (Linux/Android) |
| 6 | ptrace(PT_DENY_ATTACH) failed (macOS) |
| 7 | setrlimit(RLIMIT_CORE) failed (Unix-like systems) |

If any of these fail, the process exits immediately to ensure it doesn't run in a compromised state.

## Testing

The `vtcode-process-hardening` crate includes unit tests for:

- Correct filtering of environment variables by prefix
- Handling of non-UTF-8 environment variable names
- Correct behavior with mixed UTF-8 and non-UTF-8 entries

Run the tests with:

```bash
cargo test --package vtcode-process-hardening
```

## Security Philosophy

This hardening approach follows the "defense in depth" philosophy:

1. **Multiple layers**: Rather than relying on a single security measure, we apply multiple complementary controls
2. **Early execution**: Hardening happens before any user code runs, in a pre-main constructor
3. **Fail-safe defaults**: If hardening fails, the process exits rather than running in a potentially compromised state
4. **Platform-specific**: Different platforms receive hardening appropriate to their security model
5. **Robustness**: Code handles edge cases like non-UTF-8 environment variables correctly

## References

- OpenAI Codex Process Hardening: https://github.com/openai/codex/tree/main/codex-rs/process-hardening
- POSIX Security: ptrace(2), prctl(2), setrlimit(2)
- macOS Security: ptrace(2)
