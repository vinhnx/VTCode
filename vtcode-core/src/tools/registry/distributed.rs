//! Distributed slice declaration for builtin tool registrations.
//!
//! Each tool module annotates a factory function with
//! `#[distributed_slice(BUILTIN_TOOLS)]` to self-register. The linker
//! collects all annotated functions into a contiguous slice at load time,
//! eliminating the need for a central enumeration in `builtins.rs`.

use crate::tools::handlers::PlanModeState;

use super::registration::ToolRegistration;

/// Factory function type for builtin tool registrations.
///
/// Each tool module defines a function matching this signature and annotates it
/// with `#[distributed_slice(BUILTIN_TOOLS)]`. The function receives an optional
/// `PlanModeState` reference for tools that depend on plan mode runtime state;
/// tools that do not need it simply ignore the parameter.
pub type BuiltinToolFactory = fn(Option<&PlanModeState>) -> ToolRegistration;

/// Distributed slice of builtin tool factory functions.
///
/// Elements are placed by `#[distributed_slice(BUILTIN_TOOLS)]` annotations
/// across tool modules. The linker collects them into a contiguous `&'static [BuiltinToolFactory]`.
#[linkme::distributed_slice]
pub static BUILTIN_TOOLS: [BuiltinToolFactory] = [..];
