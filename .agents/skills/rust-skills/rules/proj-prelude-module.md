# proj-prelude-module

> Create prelude module for common imports

## When NOT to Do This

Note: newer ecosystem guidance (the Google Rust Style Guide and Microsoft Pragmatic
Rust Guidelines, via the BeautifulRust synthesis) actively advises *against* preludes
for most crates - fix the module design and let users import what they need, because
glob-importing a large prelude pollutes the user's namespace and makes the origin of a
name hard to trace. Treat a prelude as a deliberate affordance for large public APIs
where you genuinely re-export most of the facade (as `tokio`, `axum`, and `serde_json`
do), not a default. If you do publish one, keep it small, curated, and stable -
removing an item is a breaking change, and feature-gated items can't participate
unconditionally.

## Why It Matters

A `prelude` module collects the most commonly used types and traits for glob import. Users write `use my_crate::prelude::*` instead of many individual imports. This follows the pattern established by `std::prelude`.

## Bad

```rust
// Users must import everything individually
use my_crate::Client;
use my_crate::Config;
use my_crate::Error;
use my_crate::Request;
use my_crate::Response;
use my_crate::traits::Handler;
use my_crate::traits::Middleware;
use my_crate::types::Method;
```

## Good

```rust
// src/lib.rs
pub mod prelude {
    pub use crate::{
        Client,
        Config,
        Error,
        Request,
        Response,
    };
    pub use crate::traits::{Handler, Middleware};
    pub use crate::types::Method;
}

// Users write:
use my_crate::prelude::*;
```

## What to Include

| Include | Don't Include |
|---------|---------------|
| Core types users always need | Rarely-used types |
| Common traits | Implementation details |
| Error types | Internal helpers |
| Extension traits | Feature-gated items (usually) |
| Type aliases | Everything |

## Example: Web Framework Prelude

```rust
pub mod prelude {
    // Core request/response
    pub use crate::{Request, Response, Body};
    
    // Error handling
    pub use crate::Error;
    
    // Common traits
    pub use crate::traits::{FromRequest, IntoResponse};
    
    // Routing
    pub use crate::Router;
    
    // HTTP types
    pub use crate::http::{Method, StatusCode};
}
```

## Example: Database Library Prelude

```rust
pub mod prelude {
    // Connection and pool
    pub use crate::{Connection, Pool};
    
    // Query building
    pub use crate::query::{Query, Select, Insert, Update, Delete};
    
    // Traits for custom types
    pub use crate::traits::{FromRow, ToSql};
    
    // Error type
    pub use crate::Error;
}
```

## Pattern: Tiered Preludes

```rust
// Minimal prelude
pub mod prelude {
    pub use crate::{Client, Config, Error};
}

// Full prelude for power users
pub mod full_prelude {
    pub use crate::prelude::*;
    pub use crate::advanced::*;
    pub use crate::extensions::*;
}
```

## Pattern: Feature-Gated Prelude Items

```rust
pub mod prelude {
    pub use crate::{Client, Error};
    
    #[cfg(feature = "async")]
    pub use crate::async_client::AsyncClient;
    
    #[cfg(feature = "serde")]
    pub use crate::serde::{Serialize, Deserialize};
}
```

## Guidelines

1. **Be conservative** - Only include truly common items
2. **Avoid conflicts** - Don't include names that might clash (e.g., `Error`)
3. **Document it** - List what's included in module docs
4. **Stay stable** - Removing items is breaking change

## Documenting the Prelude

```rust
//! Common imports for convenient glob importing.
//!
//! # Usage
//!
//! ```
//! use my_crate::prelude::*;
//! ```
//!
//! # Contents
//!
//! This prelude re-exports:
//! - [`Client`] - The main API client
//! - [`Config`] - Client configuration
//! - [`Error`] - Error type
pub mod prelude {
    // ...
}
```

## See Also

- [proj-pub-use-reexport](./proj-pub-use-reexport.md) - Re-export patterns
- [api-extension-trait](./api-extension-trait.md) - Extension traits
- [doc-module-inner](./doc-module-inner.md) - Module documentation
