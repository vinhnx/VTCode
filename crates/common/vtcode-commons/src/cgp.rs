//! Context-Generic Programming (CGP) core wiring trait.
//!
//! CGP is a type-level composition pattern where a "context" struct maps
//! component names to concrete provider types. The foundational trait is
//! [`HasComponent`], which performs this mapping. The [`crate::delegate_components`]
//! macro generates bulk implementations.
//!
//! # Compile-time wiring checks (CGP v0.8.0 style)
//!
//! `delegate_components!` only records the mapping: it does not prove that the
//! chosen provider actually implements the capability's provider trait for the
//! context. That mistake otherwise surfaces deep inside a generic consumer
//! bound. [`crate::delegate_and_check_components`] and [`crate::check_components`]
//! move the failure back to the wiring entry, so `add a provider that does not
//! implement its trait` is rejected where the choice was made.
//!
//! # Example
//!
//! ```rust,ignore
//! // Define component marker types and a provider trait with an explicit context.
//! struct ApprovalComponent;
//! struct SandboxComponent;
//! trait ApprovalProvider<Ctx> {}
//! struct PromptApproval;
//! impl<C> ApprovalProvider<C> for PromptApproval {}
//!
//! // Wire and verify in one table.
//! delegate_and_check_components!(InteractiveCtx {
//!     ApprovalComponent => PromptApproval : ApprovalProvider<InteractiveCtx>,
//!     SandboxComponent  => WorkspaceSandbox : SandboxProvider<InteractiveCtx>,
//! });
//!
//! // Or verify wiring declared elsewhere against the resolved providers.
//! check_components!(InteractiveCtx {
//!     ApprovalComponent => ApprovalProvider<InteractiveCtx>,
//! });
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

/// Wire components and prove each provider implements its capability trait.
///
/// Each entry is `Component => Provider : ProviderTrait<Ctx>`. The trait check
/// is type-checked at the table entry, so a mis-wiring is reported next to the
/// choice rather than in the consumer bound that later needs the capability.
///
/// ```rust,ignore
/// delegate_and_check_components!(InteractiveCtx {
///     ApprovalComponent => PromptApproval : ApprovalProvider<InteractiveCtx>,
///     SandboxComponent  => WorkspaceSandbox : SandboxProvider<InteractiveCtx>,
/// });
/// ```
#[macro_export]
macro_rules! delegate_and_check_components {
    ($ctx:ty { $($name:ty => $provider:ty : $provider_trait:path),+ $(,)? }) => {
        $crate::delegate_components!($ctx { $($name => $provider),+ });
        $(
            const _: fn() = || {
                fn assert_provider<P: $provider_trait>() {}
                assert_provider::<$provider>();
            };
        )+
    };
}

/// Verify the providers already wired for a concrete context.
///
/// Unlike [`crate::delegate_and_check_components`], this checks the resolved
/// [`crate::cgp::ComponentProvider`] instead of a provider written at the call
/// site, so it can validate a context whose wiring lives in another module.
///
/// ```rust,ignore
/// check_components!(InteractiveCtx {
///     ApprovalComponent => ApprovalProvider<InteractiveCtx>,
///     SandboxComponent  => SandboxProvider<InteractiveCtx>,
/// });
/// ```
#[macro_export]
macro_rules! check_components {
    ($ctx:ty { $($name:ty => $provider_trait:path),+ $(,)? }) => {
        $(
            const _: fn() = || {
                fn assert_provider<P: $provider_trait>() {}
                assert_provider::<$crate::cgp::ComponentProvider<$ctx, $name>>();
            };
        )+
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Ctx;
    struct OtherCtx;
    struct Capability;
    struct GoodProvider;
    struct OtherProvider;

    trait CapabilityProvider<C> {}

    impl CapabilityProvider<Ctx> for GoodProvider {}
    impl CapabilityProvider<OtherCtx> for OtherProvider {}

    delegate_components!(Ctx { Capability => GoodProvider });

    // Checks the provider resolved from the existing wiring.
    check_components!(Ctx { Capability => CapabilityProvider<Ctx> });

    // Wires and verifies in one table.
    delegate_and_check_components!(OtherCtx {
        Capability => OtherProvider : CapabilityProvider<OtherCtx>,
    });

    #[test]
    fn checked_wiring_resolves_to_the_selected_provider() {
        fn assert_provider<P: CapabilityProvider<Ctx>>() {}
        assert_provider::<ComponentProvider<Ctx, Capability>>();
    }
}
