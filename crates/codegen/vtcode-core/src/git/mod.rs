//! Git worktree management for loop isolation.
//!
//! Provides `WorktreeManager` for creating, listing, and removing git worktrees
//! so that parallel loop runs cannot corrupt each other's working trees.
//! The `WorktreeReconciler` handles the diff → verify → merge cycle after
//! a worktree-isolated subagent completes.

pub mod reconciler;
pub mod verify;
pub mod worktree;

pub use reconciler::{ReconcileResult, WorktreeReconciler};
pub use verify::{DiffVerifier, HeuristicDiffVerifier, VerifyVerdict};
pub use worktree::{WorktreeInfo, WorktreeManager};
