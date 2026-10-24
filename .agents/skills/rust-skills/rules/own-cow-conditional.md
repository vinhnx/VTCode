# own-cow-conditional

> Use `Cow<'a, T>` only for real conditional ownership

## Why It Matters

`Cow` (Clone-on-Write) lets you avoid allocations when you *might* need to own data but usually don't. It holds either a borrowed reference or an owned value, cloning only when mutation is needed.

`Cow` is not a free `&T`. It carries ownership state and drop/branch overhead, and that overhead can matter in hot loops or dense token/AST-style data structures. If data is always borrowed, store a plain reference (`&str`, `&[T]`). If data is always owned, store the owned type. Use `Cow` when both borrowed and owned cases are genuinely part of the API or algorithm.

## Bad

```rust
// Always allocates, even when input doesn't need modification
fn normalize_path(path: &str) -> String {
    if path.contains("//") {
        path.replace("//", "/")  // Allocation needed
    } else {
        path.to_string()  // Unnecessary allocation!
    }
}

// Always clones the error message
fn format_error(code: u32) -> String {
    match code {
        404 => "Not Found".to_string(),      // Unnecessary!
        500 => "Internal Error".to_string(), // Unnecessary!
        _ => format!("Error {}", code),      // This one needs allocation
    }
}
```

## Good

```rust
use std::borrow::Cow;

// Only allocates when needed
fn normalize_path(path: &str) -> Cow<'_, str> {
    if path.contains("//") {
        Cow::Owned(path.replace("//", "/"))  // Allocate
    } else {
        Cow::Borrowed(path)  // Zero-cost borrow
    }
}

// Static strings stay borrowed
fn format_error(code: u32) -> Cow<'static, str> {
    match code {
        404 => Cow::Borrowed("Not Found"),      // No allocation
        500 => Cow::Borrowed("Internal Error"), // No allocation
        _ => Cow::Owned(format!("Error {}", code)), // Allocate only for unknown
    }
}
```

## Real-World Example from ripgrep

```rust
// https://github.com/BurntSushi/ripgrep/blob/master/crates/globset/src/pathutil.rs
pub(crate) fn file_name<'a>(path: &Cow<'a, [u8]>) -> Option<Cow<'a, [u8]>> {
    let last_slash = path.rfind_byte(b'/').map(|i| i + 1).unwrap_or(0);
    match *path {
        Cow::Borrowed(path) => Some(Cow::Borrowed(&path[last_slash..])),
        Cow::Owned(ref path) => {
            let mut path = path.clone();
            path.drain_bytes(..last_slash);
            Some(Cow::Owned(path))
        }
    }
}
```

## Clone-on-Write Pattern

```rust
use std::borrow::Cow;

fn process_text(text: Cow<'_, str>) -> Cow<'_, str> {
    if text.contains("bad_word") {
        // to_mut() clones if borrowed, returns &mut if owned
        let mut owned = text.into_owned();
        owned = owned.replace("bad_word", "***");
        Cow::Owned(owned)
    } else {
        text  // Pass through unchanged
    }
}

// Usage
let borrowed: Cow<str> = Cow::Borrowed("hello world");
let result = process_text(borrowed);  // No allocation!

let with_bad: Cow<str> = Cow::Borrowed("hello bad_word");
let result = process_text(with_bad);  // Allocates only here
```

## Cow with Collections

```rust
use std::borrow::Cow;

// Mixed borrowed/owned in a collection
fn collect_errors<'a>(
    static_errors: &[&'static str],
    dynamic_errors: Vec<String>,
) -> Vec<Cow<'a, str>> {
    let mut errors: Vec<Cow<str>> = Vec::new();
    
    // Static strings - no allocation
    for &e in static_errors {
        errors.push(Cow::Borrowed(e));
    }
    
    // Dynamic strings - take ownership
    for e in dynamic_errors {
        errors.push(Cow::Owned(e));
    }
    
    errors
}
```

## When to Use Cow

| Situation | Use Cow? |
|-----------|----------|
| Usually borrow, sometimes own | Yes |
| Always need owned data | No, just use owned type |
| Always borrow | No, just use reference |
| Hot path with dense values that are always borrowed | No, use references/slices and keep variants compact |
| Hot path with measured conditional ownership benefit | Maybe, benchmark it |
| Returning static strings or formatted | Yes |

## Hot Path Guidance

- Do not put `Cow<str>` in a token or AST enum just because most values are borrowed. If mutation/ownership is never used, `&str` gives the compiler less drop and branch work.
- For dense streams, consider whether splitting variants keeps hot enum sizes small enough for better cache behavior.
- Measure before and after: removing `Cow` can help when ownership flexibility is unused, but replacing allocations with `Cow` can still be a win when many values pass through unchanged.

## See Also

- [own-borrow-over-clone](own-borrow-over-clone.md) - Prefer borrowing over cloning
- [mem-avoid-format](mem-avoid-format.md) - Avoid format! when possible
