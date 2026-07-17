//! Context-Generic Programming (CGP) core wiring trait.
//!
//! CGP is a type-level composition pattern where a "context" struct maps
//! component names to concrete provider types. The foundational trait is
//! [`HasComponent`], which performs this mapping. The [`crate::delegate_components`]
//! macro generates bulk implementations.
//!
//! # Example
//!
//! ```rust,ignore
//! // Define component marker types
//! struct ApprovalComponent;
//! struct SandboxComponent;
//!
//! // Wire them for a context
//! delegate_components!(InteractiveCtx {
//!     ApprovalComponent => PromptApproval,
//!     SandboxComponent  => WorkspaceSandbox,
//! });
//!
//! // Access the wired provider type
//! let provider: ComponentProvider<InteractiveCtx, ApprovalComponent> = ...;
//! ```

/// Type-level lookup: maps a component **Name** to a concrete **Provider**
/// type for a given implementor (the "context").
///
/// This is the single foundational trait of the CGP substrate. All
/// composition flows through it.
pub trait HasComponent<Name> {
    /// The concrete provider type wired to `Name` for this context.
    type Provider;
}

/// The elaborated provider/dictionary selected by `Ctx` for component `Name`.
pub type ComponentProvider<Ctx, Name> = <Ctx as HasComponent<Name>>::Provider;

/// Wire multiple component names to provider types for a context.
///
/// Generates one `HasComponent<Name>` implementation per entry.
///
/// ```rust,ignore
/// delegate_components!(MyCtx {
///     ApprovalComponent => PromptApproval,
///     SandboxComponent  => WorkspaceSandbox,
/// });
/// ```
#[macro_export]
macro_rules! delegate_components {
    ($ctx:ty { $($name:ty => $provider:ty),* $(,)? }) => {
        $(
            impl $crate::cgp::HasComponent<$name> for $ctx {
                type Provider = $provider;
            }
        )*
    };
}
