# own-drop-contract

> Destructors never fail, panic, or block

## Why It Matters

`Drop` is the body of Rust's RAII model - resources are freed automatically on scope
exit, early returns via `?`, and unwinding. But `Drop` runs in places ordinary code doesn't - on every scope exit and early `?`
return, and during panicking unwinding - so breaking its contract aborts the process
or silently leaves resources dirty.

## Bad

```rust
impl Drop for Transaction {
    fn drop(&mut self) {
        // If rollback() fails or blocks, we panic during unwinding
        // and the process aborts mid-cleanup.
        self.rollback().unwrap();
    }
}
```

## Good

```rust
impl Drop for Transaction {
    fn drop(&mut self) {
        // Best-effort cleanup only; fallible teardown is explicit.
        let _ = self.rollback();
    }
}

impl Transaction {
    /// Deliberate teardown; reports failures to the caller.
    pub fn close(self) -> Result<(), TxError> {
        self.rollback()
    }
}
```

## The Contract

| Rule | Why |
|------|-----|
| Never `panic!` in `Drop` | A panic during unwinding aborts the process mid-cleanup. |
| Never `Result` in `Drop` | `Drop` can't fail. Use `close(self) -> Result<..>` for fallible teardown, and let `Drop` do best-effort cleanup. |
| Never block in `Drop` | Offer a separate method that prepares an infallible, non-blocking teardown. |
| Call `drop(x)`, never `x.drop()` | The explicit must be present. |

## Bind guards to a name

`let _ = guard;` drops *immediately*; only a named binding lives to the end of scope.

```rust
let _guard = self.lock.lock();     // lives to end of scope

let _ = self.lock.lock();          // dropped immediately - guard is useless
```

## Guards that Deref

Expose the resource through a `Deref` guard, as `MutexGuard` does; the borrow checker
then stops references obtained through the guard from outliving it.

## See Also

- [own-borrow-counter-ladder](own-borrow-counter-ladder.md) - Moving data instead of cloning
- [own-clone-explicit](own-clone-explicit.md) - Making `Clone` explicit and non-implicit
- [err-panic-policy](err-panic-policy.md) - When to panic, when to return `Result`
- [test-fixture-raii](test-fixture-raii.md) - RAII cleanup in test fixtures
