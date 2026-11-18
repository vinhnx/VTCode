//! Best-effort process-group cleanup for spawned lifecycle hooks.
//!
//! Split from `engine.rs` so the engine focuses on hook orchestration while
//! the platform-specific abort safety lives on its own. The guard is imported
//! back into the parent under `#[cfg(unix)]`, so every original call site is
//! unchanged.

/// Best-effort cleanup for a hook's process group on abort.
///
/// `kill_on_drop(true)` only kills the direct `sh` child, which leaves shell
/// grandchildren alive and holding the captured pipes open. When the owning
/// future is cancelled, this guard kills the whole group; it is disarmed once
/// the hook finishes normally so a deliberately detached descendant is not
/// retroactively killed. Mirrors `crate::exec::async_command`.
#[cfg(unix)]
pub(super) struct HookProcessGroupGuard {
    pid: Option<u32>,
    armed: bool,
}

#[cfg(unix)]
impl HookProcessGroupGuard {
    pub(super) fn new(pid: Option<u32>) -> Self {
        Self { pid, armed: true }
    }

    /// Kill at most once: once the child has been reaped its PID can be
    /// recycled, so a second best-effort kill could hit an unrelated group.
    pub(super) fn kill(&mut self) {
        if !self.armed {
            return;
        }
        self.armed = false;
        if let Some(pid) = self.pid {
            let _ = vtcode_bash_runner::process_group::kill_process_group_by_pid(pid);
        }
    }

    pub(super) fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(unix)]
impl Drop for HookProcessGroupGuard {
    fn drop(&mut self) {
        self.kill();
    }
}
