//! RL optimization loop: adaptive action selection.
//!
//! VT Code couples its modular runtime with a data/evaluation strategy so the
//! harness can prefer low-latency, high-success actions (e.g. edge vs cloud
//! executors) over time. This is the real implementation behind the
//! `docs/ARCHITECTURE.md` "RL Optimization Loop" section:
//!
//! - Command/sandbox outcomes are captured through the existing `bash_runner`
//!   and PTY subsystems (no extra instrumentation).
//! - Each outcome becomes a [`RewardSignal`] (success + latency + cost).
//! - Signals accumulate in a rolling [`RewardLedger`] keyed by action id.
//! - [`RlEngine::select`] picks the next action via UCB / epsilon-greedy
//!   bandit logic, or an actor-critic stand-in, driven by `[optimization].rl`.
//!
//! The module is decomposed into independently testable chunks:
//! [`signal`] (reward math + strategy), [`ledger`] (rolling statistics),
//! [`engine`] (selection policy), and [`eval`] (eval-report bridge).

pub mod engine;
pub mod eval;
pub mod ledger;
pub mod signal;

pub use engine::{Action, PolicyContext, RlEngine, RlSnapshot};
pub use eval::reward_from_eval_report;
pub use ledger::RewardLedger;
pub use signal::{RewardSignal, RlStrategy};
