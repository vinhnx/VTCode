# err-panic-policy

> Panics are not exceptions - a panic means "stop now"

## Why It Matters

Panics are for bugs and violated invariants, not for reporting bad input. Treating them
as exceptions breaks under `panic = "abort"` (where `catch_unwind` does nothing) and
can leave shared state inconsistent. Pick the mechanism by contract, and know exactly
when each is right.

## The Contract Table

| Situation | Use |
|---|---|
| Absence is normal (`first()`, `checked_add`) | `Option<T>` |
| The caller may need to know what went wrong | `Result<T, E>` |
| A bug or violated invariant inside your code | `panic!` / `assert!` / `expect("why")` |

## Bad

```rust
// Panic as input validation - the caller can never recover.
fn parse_name(input: &str) -> &str {
    if input.is_empty() {
        panic!("empty name");
    }
    input
}

// Catching panics as control flow.
let result = std::panic::catch_unwind(|| parse_name(""));
```

## Good

```rust
// Fallible input returns Result (thiserror-style enum, or thiserror).
fn parse_name(input: &str) -> Result<&str, ParseError> {
    if input.is_empty() {
        return Err(ParseError::Empty);
    }
    Ok(input)
}

// Bugs and invariants are panics - with a message carrying the values.
assert!(buf.len() >= HEADER, "buffer too small: {} bytes, expected {HEADER}", buf.len());
let config = config.expect("config is loaded before workers start");
```

## Panic Rules

- **Prefer `expect("reason")` over `unwrap()`** - the message states why the invariant
  holds; don't restate the condition.
- **Pattern-match instead of check-then-unwrap:**
  `if let Some(v) = opt { .. }` or `let Some(v) = opt else { return .. };` - never
  `if opt.is_some() { opt.unwrap() }`.
- **Lock poisoning is fatal:** `.lock().expect("lock poisoned")`. Don't recover with
  `into_inner` - if a panic happened while the lock was held, the invariants guarded by
  the lock may be broken.
- **Pair fallible with infallible:** where a panicking convenience is justified (`new`,
  `get`), provide a fallible sibling (`try_new`, `get_checked`) and document the panic
  under `# Panics`.
- **`catch_unwind` is last-resort defense in depth**, never relied upon: it does
  nothing under `panic = "abort"`, only unwinds the panic frame (not all panics), and a
  library must never catch-and-continue. Use it at FFI boundaries or for per-request
  isolation followed by a controlled restart.

## See Also

- [anti-panic-expected](anti-panic-expected.md) - Don't panic on expected errors
- [anti-expect-lazy](anti-expect-lazy.md) - `expect` is for programming errors
- [err-result-over-panic](err-result-over-panic.md) - `Result` over panicking paths
- [own-drop-contract](own-drop-contract.md) - `Drop` must never panic or block
