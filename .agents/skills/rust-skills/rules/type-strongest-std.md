# type-strongest-std

> Use the strongest std type at every layer

## Why It Matters

Catching a diameter mismatch in `PathBuf`/`Path` (or `Duration`, `SystemTime`,
`SocketAddr`, `RangeBounds`) saves one risk of passing it wrongly; catching it in raw
`&str`/`u64` requires upstream docs or an unlucky test. The std library has already
built the type - use it and the compiler validates the domain every call.

## Bad

```rust
fn copy(src: &str, dst: &str, timeout: u64) -> Result<(), E> { /* ... */ }
// u64: nanoseconds? milliseconds? seconds? It's guess-only.
// &str: no type guarantees (is it UTF-8? a path? a URL? tickets?);
// "why do we normalize separators?" is answered nowhere.

fn per_page(page: i32) -> Vec<Item> { /* ... */ } // i32: negative page sizes silent
```

## Good

```rust
use std::{path::PathBuf, time::Duration, num::NonZeroU32};

fn copy(src: impl AsRef<Path>, dst: impl AsRef<Path>, timeout: Duration) -> Result<(), E> { /* ... */ }

fn per_page(n: NonZeroU32) -> Vec<Item> { /* ... */ }

// Independent flags: `bitflags`. A single choice: enum (bitflags over
// enums produces invalid combinations).
bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Permissions: u8 { READ = 1; WRITE = 2; EXECUTE = 4; }
}
```

## Strongest types by layer

| Data | Strongest std type |
|------|--------------------|
| Filesystem location | `PathBuf` / `&Path` (fields own `PathBuf`; params may be `impl AsRef<Path>`) |
| Time span / instant | `Duration` / `SystemTime` - never a bare `u64` this layer passes around |
| Bounded quantity | `NonZeroU32` (internals only - keep plain integers at API boundaries) |
| Range | `impl RangeBounds<T>` instead of `(low, high)` pairs accepting `1..3`, `..` |
| Byte buffer read | `&[u8]`, not `&Vec<u8>` |
| Independent flags | `bitflags` crate - an enum models *exactly one* choice |
| OS string | `OsString` for opaque OS data (`std::env::args_os`), only converting to `String` as needed |

## When a bare scalar still wins

Human-facing units (`meters`, `seconds` since epoch) often stabilize in the domain -
wrap them with a newtype, they are documented elsewhere. Domain values (glass
thickness in mm, currency cents, orders of magnitude) encourage a newtype at *any*
layer (`type-newtype-ids`, `type-newtype-validated`).

## See Also

- [type-newtype-ids](type-newtype-ids.md) - Newtypes for IDs
- [type-newtype-validated](type-newtype-validated.md) - Newtypes for validated data
- [api-impl-asref](api-impl-asref.md) - `impl AsRef<T>` for borrowed parameter types
- [api-impl-into](api-impl-into.md) - `impl Into<T>` for owned parameter types
