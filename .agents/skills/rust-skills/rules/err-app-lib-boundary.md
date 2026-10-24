# err-app-lib-boundary

> Application and library error handling are different contracts

## Why It Matters

Libraries and applications fail differently. A library caller is another programmer
whose tools need `std::error::Error` dispatch, typed variants, and `Send + Sync`
for cross-thread propagation. An application caller is `main` and a human reading a
crash report. Mixing the two - an `anyhow::Error` escaping a library API, or five
hand-rolled app error types - makes both sides worse.

## Bad

```rust
// Library exposing anyhow - callers can't match, and their type
// checker can't see any failure mode.
pub fn load_index(path: &Path) -> anyhow::Result<Index> { /* ... */ }

// Application with five unrelated error types.
fn run() -> Result<(), ConfigError> { /* ... */ }
fn serve(cfg: &Config) -> Result<(), ListenError> { /* ... */ }
```

## Good

```rust
// Library: typed errors implement the full std contract.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("index file not found: {path}")]
    NotFound { path: PathBuf },
    #[error("corrupt index: {source}")]
    Corrupt {
        #[source]
        source: io::Error,
    },
}
impl std::error::Error for IndexError {} // thiserror provides it

// Application: ONE error type, and main returns it directly.
fn main() -> anyhow::Result<()> {
    run()?;  // one app-level error chain with .context()
    Ok(())
}
```

## The Boundary Rules

| Rule | Library | Application |
|------|---------|-------------|
| Error type | Typed enum/struct implementing `std::error::Error + Display + Send + Sync + 'static` | One app-level type (`anyhow::Error` or `eyre`) |
| Never | `anyhow::Error` in the public API, `()`, `String` as error type | Several competing app error types |
| Conversion | `From<Upstream> for Error` so `?` converts; `map_err` only for foreign errors or context | `.context("while opening config")` per step |
| Cause chain | `#[source]` / `source()` for wrapped causes | `thiserror` chains automatically down to `std::error::Error` |

## Return the consumed argument on error

If a fallible function takes ownership, give callers back what they passed - retry
becomes clone-free:

```rust
pub struct ParseError {
    pub kind: ParseErrorKind, // children can check without carrying the whole frame
    pub input: String,        // like FromUtf8Error::into_bytes
}

impl ParseError {
    pub fn into_input(self) -> String { self.input }
}
```

## See Also

- [err-thiserror-lib](err-thiserror-lib.md) - `thiserror` for library errors
- [err-anyhow-app](err-anyhow-app.md) - `anyhow` for applications
- [err-source-chain](err-source-chain.md) - `#[source]` chains
- [err-custom-type](err-custom-type.md) - Custom error types over `Box<dyn Error>`
