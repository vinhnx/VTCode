//! Safety validation utilities
//!
//! This module re-exports safety-related types and functions.

pub mod hitl;

pub use hitl::{HitlAuditTrail, HitlEvent, HitlGate, OversightDecision};
