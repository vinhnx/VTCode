# vtcode-process-hardening

This crate provides `pre_main_hardening()`, which is designed to be called pre-`main()` (using `#[ctor::ctor]`) to perform various process hardening steps, such as:

- disabling core dumps
- disabling ptrace attach on Linux and macOS
- removing dangerous environment variables such as `LD_PRELOAD` and `DYLD_*`

## Usage

Add to your binary's `Cargo.toml`:

```toml
[dependencies]
ctor = "0.2"
vtcode-process-hardening = { path = "../vtcode-process-hardening" }
```

In your `main.rs`:

```rust
#[ctor::ctor]
fn init() {
    vtcode_process_hardening::pre_main_hardening();
}

fn main() {
    // Your code here
}
```

## Security Hardening

### Linux/Android
- **PR_SET_DUMPABLE**: Prevents ptrace attachment and disables core dumps at the process level
- **RLIMIT_CORE**: Sets core file size limit to 0 for defense in depth
- **LD_* removal**: Strips `LD_PRELOAD` and similar variables that could subvert library loading

### macOS
- **PT_DENY_ATTACH**: Prevents debugger attachment via the ptrace system call
- **RLIMIT_CORE**: Disables core dumps
- **DYLD_* removal**: Removes dynamic linker environment variables that could compromise library loading

### BSD (FreeBSD, OpenBSD)
- **RLIMIT_CORE**: Sets core file size limit to 0
- **LD_* removal**: Strips dynamic linker variables

## Notes

- This crate calls `unsafe` libc functions but validates all return codes and exits cleanly on failure
- Environment variable removal uses `std::env::vars_os()` to handle non-UTF-8 variable names correctly
- On Windows, hardening is a placeholder for future implementation
