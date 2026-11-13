//! Per-tool metadata contracts for the shipped builtin catalog.
//!
//! The runtime registers builtins through `pack_impls` ->
//! `register_builtin_packs`. This module used to carry a parallel
//! `#[distributed_slice(BUILTIN_TOOLS)]` implementation of the same set, which
//! drifted from the shipped path: a description, alias, or schema present only
//! here would pass every assertion while never reaching the model. The tests
//! below now read the shipped registrations, so only the tests remain.

#[cfg(test)]
mod tests;
