use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_WAIT: AtomicU64 = AtomicU64::new(1);

/// Owns a presentation-only user wait. Dropping it restores the underlying
/// terminal status, even on cancellation, failed deferral, or early return.
#[must_use = "keep the guard alive while waiting for user interaction"]
pub struct ProgramStatusWaitGuard {
    resume: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl ProgramStatusWaitGuard {
    pub(crate) fn token() -> u64 {
        NEXT_WAIT.fetch_add(1, Ordering::Relaxed)
    }

    pub(crate) fn new(resume: impl FnOnce() + Send + Sync + 'static) -> Self {
        Self { resume: Some(Box::new(resume)) }
    }
}

impl Drop for ProgramStatusWaitGuard {
    fn drop(&mut self) {
        if let Some(resume) = self.resume.take() {
            resume();
        }
    }
}
