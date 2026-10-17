# vtcode-macros

Procedural macros for VT Code.

`vtcode-macros` provides derive macros that eliminate boilerplate for common patterns in the VT Code codebase.

<!-- cargo-rdme -->

## Derive macros

### `StringNewtype`

Derive macro for tuple structs wrapping a single `String` field. Generates:

- Inherent methods: `new()`, `as_str()`, `into_inner()`
- `Deref<Target = str>`
- `Borrow<str>`
- `AsRef<str>`
- `Display`
- `From<String>`, `From<&str>`, `From<Self> for String`

### `DebugNoInline`

Derive macro equivalent to `#[derive(Debug)]`, but the generated `fmt` is marked `#[inline(never)]`. Rust's built-in
`Debug` derive emits `#[inline]`; for large or deeply nested types formatted on fan-out paths, that can inline the
whole `Debug` tree into every `{:?}` / `?err` call site and bloat the binary. Output is byte-identical to the standard
derive; only the inlining hint differs. Use it for large/nested types (often error enums); keep `#[derive(Debug)]` for
small, hot, leaf types. See `docs/development/rust-performance-principles.md`.

## Usage

### `StringNewtype`

```rust,ignore
use vtcode_macros::StringNewtype;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize, StringNewtype)]
#[serde(transparent)]
pub struct SessionId(String);

let id = SessionId::new("abc-123");
assert_eq!(id.as_str(), "abc-123");
assert_eq!(id.to_string(), "abc-123");

let inner: String = id.into_inner();
```

### `DebugNoInline`

```rust,ignore
use vtcode_macros::DebugNoInline;

#[derive(DebugNoInline)]
pub struct Widgets {
    foo: u32,
    bar: usize,
}

assert_eq!(format!("{:?}", Widgets { foo: 1, bar: 2 }), "Widgets { foo: 1, bar: 2 }");
```

## API reference

See [docs.rs/vtcode-macros](https://docs.rs/vtcode-macros).
