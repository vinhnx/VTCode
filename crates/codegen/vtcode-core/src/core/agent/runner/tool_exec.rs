//! Tool-call admission, dispatch, and outcome recording for the agent runner.
//!
//! Split by responsibility: [`admission`] admits calls and plans batch layout,
//! [`dispatch`] executes parallel/sequential batches, [`fallback`] owns repeat
//! throttling and fallback chains, and [`outcome`] records results and halt
//! policy.

mod admission;
mod dispatch;
mod fallback;
mod outcome;
