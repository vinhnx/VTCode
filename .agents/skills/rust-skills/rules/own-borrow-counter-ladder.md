# own-borrow-counter-ladder

> Fix borrow-checker errors by moving data, not cloning

## Why It Matters

A `.clone()` added only to silence the borrow checker creates a silent copy whose later
changes are lost to the original. Most borrow errors are ownership-design problems, and
the compiler has already told you exactly where they are. Work through the tactics below
in order - clone is the last resort, not the first.

## Bad

```rust
fn flush(&mut self, pending: &mut Option<String>) {
    if let Some(msg) = pending {
        let msg = msg.clone(); // clone just to end the borrow? no
        send(msg.as_str());
        self.log.push(msg); // pending is now empty but unchanged
    }
}
```

## Good

```rust
use std::mem::take;

// Move the field out of &mut self without cloning.
fn flush(&mut self, pending: &mut Option<String>) {
    let Some(msg) = pending.take() else { return };
    send(&msg);
    self.log.push(msg);
}
```

## The Ladder (in order)

0. **Extend a temporary with a named `let`, or shorten a borrow with a block.**

   ```rust
   let item = list.last().copied(); // extend the temporary's life
   if let Some(x) = item { /* x is now ownable */ }
   ```

1. **Split a long expression into typed intermediates** so the failing borrow step is
   visible to you (and dealt with) piece by piece.

2. **Move a field out of `&mut self`**, from `type-enum-states`:

   ```rust
   fn a_to_b(e: &mut MyEnum) {
       if let MyEnum::A { name, x: 0 } = e {
           *e = MyEnum::B { name: std::mem::take(name) };
       }
   }
   ```

3. **Split a large struct into smaller ones.** The compiler tracks disjoint field
   borrows *inside* one function, but not across method calls; composing smaller structs
   restores that, and the design usually improves.

4. **Clone small, immutable data deliberately** - with a comment if it isn't obvious.
   `Rc`/`Arc` clones are cheap and always fine.

5. **Use indices into an owning `Vec`, or `Rc<RefCell<_>>` with `Weak` back-pointers.**
   Avoid self-referential structs (`ouroboros` only if truly unavoidable).

## Reordering Disabled

Later code cannot re-mutate by accident - separate mutation and freezing:

```rust
let mut data = setup();
data.mutate();
let data = data; // freeze - no future accidental mutation
```

## See Also

- [type-enum-states](type-enum-states.md) - Enums for mutually exclusive states
- [perf-drain-reuse](perf-drain-reuse.md) - Reusing allocations
- [own-move-large](own-move-large.md) - Move large data instead of cloning
- [own-borrow-over-clone](own-borrow-over-clone.md) - Borrowing instead of cloning
