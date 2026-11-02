# anti-unsound-api

> A safe function that can cause UB through safe calls is unsound

## Why It Matters

"Unsound" is not a style issue - it means a bug in a safe API that lets callers trigger
undefined behavior without writing any `unsafe` of their own. Unsound abstractions are
never permissible: if the invariant can't be encapsulated, the surface must be `unsafe
fn` with a documented contract instead.

## Bad

```rust
pub struct IndexView<'a> {
    data: &'a [u8], // must be sorted, but nothing enforces it
}

impl<'a> IndexView<'a> {
    // Safe signature, yet calling it later downcasts u64 to smaller
    // types based on internal (unchecked) sort assumptions - UB.
    pub fn new(data: &'a [u8]) -> Self { IndexView { data } }

    pub fn lookup(&self, key: u64) -> Option<&'a [u8]> {
        // binary search over unsorted data -> wrong indexes -> OOB reads
        // through get_unchecked: SOUNDNESS BUG even though no `unsafe`
        // appears in this function.
        self.lookup_unchecked(key)
    }
}

// Also bad: unsafe impl to silence the compiler, or enum-cast transmute.
struct Unicorn(*const ());
unsafe impl Send for Unicorn {}

let flags: SmallFlag = unsafe { std::mem::transmute(7u8) }; // enum invalid value = UB
```

## Good

```rust
pub struct IndexView<'a> {
    data: &'a [u8], // enforced sorted at construction
}

impl<'a> IndexView<'a> {
    // Funnel: the ONLY constructor validates the invariant once.
    pub fn new(data: &'a [u8]) -> Option<Self> {
        if is_sorted(data) { Some(IndexView { data }) } else { None }
    }

    // Or: refuse to promise; make the caller state the contract.
    pub unsafe fn new_unchecked(data: &'a [u8]) -> Self { /* ... */ }
}

// SAFETY: Unicorn owns its pointer exclusively and never frees it
// from another thread; raw pointer is Send because we synchronize
// all accesses behind a Mutex.
unsafe impl Send for Unicorn {}
```

## The Rules

| Rule | Why |
|------|-----|
| UB stays behind `unsafe fn` | A safe fn reachable path to UB = unsound; expose `unsafe fn` + `# Safety` docs instead. |
| `// SAFETY:` on every block | States why invariants hold; `undocumented_unsafe_blocks` lint enforces. |
| Keep `unsafe` in the smallest module | The module boundary is the soundness boundary; wrap in a safe API. |
| `unsafe impl Send`/`Sync` only with proof | Never to "silence the compiler"; `Send` needs real synchronization arguments. |
| No `transmute` to cast enums or dodge lifetimes | An invalid enum discriminant is instant UB; transmute cannot extend a lifetime soundly. |
| Mark functions `unsafe` only for UB risk | `delete_database()` is *dangerous*, not *unsafe* - it's a safe function. |
| Test with Miri and adversarial inputs | Tests should try to violate the invariant via the safe API. |

## See Also

- [api-ffi-boundary](api-ffi-boundary.md) - Structure the FFI boundary safely
- [lint-unsafe-doc](lint-unsafe-doc.md) - `SAFETY` comment enforcement
- [doc-safety-section](doc-safety-section.md) - `# Safety` section requirement
- [api-parse-dont-validate](api-parse-dont-validate.md) - Enforce invariants at construction
