//! Task-scoped tool recursion limits and frame cleanup.

use hashbrown::HashMap;
use once_cell::sync::Lazy;
use parking_lot::Mutex;
use std::cell::RefCell;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::task::Id as TokioTaskId;

const REENTRANCY_STACK_DEPTH_LIMIT: usize = 64;
// Tools should never recursively re-enter themselves in a single task.
// Keeping this at 1 blocks the first re-entry (A -> ... -> A) to fail fast
// on alias/self-recursion bugs with minimal extra work.
const REENTRANCY_PER_TOOL_LIMIT: usize = 1;

/// Global reentrancy stacks for tokio tasks.
///
/// Uses `parking_lot::Mutex` for lower overhead on short critical sections.
/// Each entry/exit is a single Vec push/pop under a task ID key.
///
/// If contention becomes an issue under high concurrency, consider:
/// - Using a concurrent hash map (e.g., `dashmap`)
/// - Using task-local storage via `tokio::task_local!`
/// - Partitioning the map by task ID hash to reduce contention
#[derive(Debug)]
struct ReentrancyFrame {
    id: u64,
    tool_name: String,
}

static NEXT_REENTRANCY_FRAME_ID: AtomicU64 = AtomicU64::new(1);
static TOOL_REENTRANCY_STACKS: Lazy<Mutex<HashMap<TokioTaskId, Vec<ReentrancyFrame>>>> =
    Lazy::new(|| Mutex::new(HashMap::new()));
thread_local! {
    static THREAD_REENTRANCY_STACK: RefCell<Vec<ReentrancyFrame>> = const { RefCell::new(Vec::new()) };
}

fn lock_reentrancy_stacks() -> parking_lot::MutexGuard<'static, HashMap<TokioTaskId, Vec<ReentrancyFrame>>> {
    TOOL_REENTRANCY_STACKS.lock()
}

#[derive(Debug)]
pub(super) struct ReentrancyViolation {
    pub(super) stack_depth: usize,
    pub(super) tool_reentry_count: usize,
    pub(super) stack_trace: String,
}

enum ReentrancyContext {
    Task(TokioTaskId),
    Thread,
}

pub(super) struct ToolReentrancyGuard {
    context: Option<ReentrancyContext>,
    frame_id: u64,
}

impl ToolReentrancyGuard {
    pub(super) fn enter(tool_name: &str, allow_parallel_sibling: bool) -> Result<Self, ReentrancyViolation> {
        let frame_id = NEXT_REENTRANCY_FRAME_ID.fetch_add(1, Ordering::Relaxed);
        if let Some(task_id) = tokio::task::try_id() {
            let mut stacks = lock_reentrancy_stacks();
            let stack = stacks.entry(task_id).or_default();
            let stack_depth = stack.len();
            let tool_reentry_count = stack.iter().filter(|frame| frame.tool_name == tool_name).count();

            if stack_depth >= REENTRANCY_STACK_DEPTH_LIMIT
                || (!allow_parallel_sibling && tool_reentry_count >= REENTRANCY_PER_TOOL_LIMIT)
            {
                let stack_trace = if stack.is_empty() {
                    "<empty>".to_string()
                } else {
                    stack
                        .iter()
                        .map(|frame| frame.tool_name.as_str())
                        .collect::<Vec<_>>()
                        .join(" -> ")
                };
                return Err(ReentrancyViolation { stack_depth, tool_reentry_count, stack_trace });
            }

            stack.push(ReentrancyFrame { id: frame_id, tool_name: tool_name.to_string() });
            return Ok(Self {
                context: Some(ReentrancyContext::Task(task_id)),
                frame_id,
            });
        }

        let violation = THREAD_REENTRANCY_STACK.with(|stack_cell| {
            let mut stack = stack_cell.borrow_mut();
            let stack_depth = stack.len();
            let tool_reentry_count = stack.iter().filter(|frame| frame.tool_name == tool_name).count();

            if stack_depth >= REENTRANCY_STACK_DEPTH_LIMIT
                || (!allow_parallel_sibling && tool_reentry_count >= REENTRANCY_PER_TOOL_LIMIT)
            {
                let stack_trace = if stack.is_empty() {
                    "<empty>".to_string()
                } else {
                    stack
                        .iter()
                        .map(|frame| frame.tool_name.as_str())
                        .collect::<Vec<_>>()
                        .join(" -> ")
                };
                Some(ReentrancyViolation { stack_depth, tool_reentry_count, stack_trace })
            } else {
                stack.push(ReentrancyFrame { id: frame_id, tool_name: tool_name.to_string() });
                None
            }
        });

        if let Some(violation) = violation {
            return Err(violation);
        }

        Ok(Self { context: Some(ReentrancyContext::Thread), frame_id })
    }
}

impl Drop for ToolReentrancyGuard {
    fn drop(&mut self) {
        let Some(context) = self.context.take() else {
            return;
        };

        match context {
            ReentrancyContext::Task(task_id) => {
                let mut stacks = lock_reentrancy_stacks();
                let should_remove = if let Some(stack) = stacks.get_mut(&task_id) {
                    if let Some(position) = stack.iter().position(|frame| frame.id == self.frame_id) {
                        stack.remove(position);
                    }
                    stack.is_empty()
                } else {
                    false
                };
                if should_remove {
                    stacks.remove(&task_id);
                }
            }
            ReentrancyContext::Thread => {
                THREAD_REENTRANCY_STACK.with(|stack_cell| {
                    let mut stack = stack_cell.borrow_mut();
                    if let Some(position) = stack.iter().position(|frame| frame.id == self.frame_id) {
                        stack.remove(position);
                    }
                });
            }
        }
    }
}

#[cfg(test)]
mod tests;
