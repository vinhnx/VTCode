# async-send-sync-assert

> Pin `Send`/`Sync` on public types and futures at compile time

## Why It Matters

`Send`/`Sync` are auto-traits: adding one innocent field (`Rc`, a `RefCell` borrow held
across `.await`, a raw pointer) silently makes a type or future `!Send`. Downstream
tokio-based users then fail to compile - or worse, you find out yourself during a
refactor. A compile-time assertion converts that regression into a build error today,
in your crate, not a broken build in three releases.

## Bad

```rust
// Compiles today. If a future refactor adds an Rc to Ctx, every
// tokio::spawn user of this type breaks in their build, not ours.
struct Client {
    ctx: Ctx,
}

async fn call(c: &Client) -> Result<(), E> { /* ... */ }
```

## Good

```rust
const _: () = {
    const fn assert_send<T: Send>() {}
    const fn assert_sync<T: Sync>() {}
    assert_send::<Client>();
    assert_sync::<Client>();
};

// Assert the future is too (the type holding Rc or a RefCell borrow
// across .await makes it !Send).
fn assert_is_send<F: Send>(_: &F) {}
assert_is_send(&call(&client));
```

## The Common Offenders

| Pattern | Why it breaks |
|---------|--------------|
| `Rc<T>` / `Cell<f64>` / non-atomic types in a struct | `!Send` (and often `!Sync`) |
| Holding a `RefCell`/`Mutex` guard across `.await` | Future carries `Ref` -> `!Send`; prefer cloning data before the await, or holding locks only within a blocking scope (`tokio::sync::Mutex` if truly required) |
| A closure that captures `&mut access to a Rc` in a spawned task | `!Send` once `move`d into the task |

## When NOT to apply

Types that are deliberately single-threaded (`RefCell`-based, UI state) do not need
this - and forcing `Send` on them would be wrong. Apply the assertion where the type's
contract promises thread-safe use (anything handed to `tokio::spawn` or shared across
tasks), and where the type reaches public API.

## See Also

- [async-clone-before-await](async-clone-before-await.md) - Free locks and borrows before `.await`
- [async-no-lock-await](async-no-lock-await.md) - Never hold `Mutex`/`RwLock` across `.await`
- [own-mutex-interior](own-mutex-interior.md) - `Mutex` for multi-thread interior mutability
- [own-rc-single-thread](own-rc-single-thread.md) - `Rc` stays single-threaded
