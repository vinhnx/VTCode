# vtcode-terminal-detection

Shared terminal detection primitives for VT Code. Detects terminal emulator, color support, and capabilities.

## Conventions

- Detection results are cached after first call. Do not re-detect in hot paths.
- All detection functions return `anyhow::Result` and degrade gracefully (return "unknown" rather than error).
- Platform-specific detection uses `#[cfg]` attributes, not runtime OS checks.

## Dependencies

- `anyhow` (error handling)
- `dirs` (home/config directory resolution)
