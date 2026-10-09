use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

static NEXT_OPERATION: AtomicU64 = AtomicU64::new(1);

/// UI-only identity and monotonic origin. Never persisted or sent to a provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgressOperation {
    id: u64,
    started_at: Instant,
}

impl ProgressOperation {
    pub fn start() -> Self {
        Self {
            id: NEXT_OPERATION.fetch_add(1, Ordering::Relaxed),
            started_at: Instant::now(),
        }
    }

    pub fn id(self) -> u64 {
        self.id
    }

    pub fn started_at(self) -> Instant {
        self.started_at
    }
}

/// Actual work phases; these labels never grant admission or change input ownership.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressPhase {
    Initializing,
    PreparingContext,
    SavingCheckpoint,
    WaitingForModel,
    Processing,
    ReceivingResponse,
    Retrying,
    CheckingPermissions,
    WaitingForApproval,
    RunningTools,
}

impl ProgressPhase {
    pub fn label(self) -> &'static str {
        match self {
            Self::Initializing => "Initializing session",
            Self::PreparingContext => "Preparing context",
            Self::SavingCheckpoint => "Saving checkpoint",
            Self::WaitingForModel => "Waiting for model",
            Self::Processing => "Processing",
            Self::ReceivingResponse => "Receiving response",
            Self::Retrying => "Retrying request",
            Self::CheckingPermissions => "Checking permissions",
            Self::WaitingForApproval => "Waiting for approval",
            Self::RunningTools => "Running tools",
        }
    }

    pub fn is_animated(self) -> bool {
        self != Self::WaitingForApproval
    }

    pub fn format(self, elapsed_secs: u64) -> String {
        if elapsed_secs == 0 {
            self.label().to_string()
        } else {
            format!("{} · {elapsed_secs}s", self.label())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProgressUpdate {
    Begin {
        operation: ProgressOperation,
        phase: ProgressPhase,
    },
    Phase {
        operation: ProgressOperation,
        phase: ProgressPhase,
    },
    Finish {
        operation: ProgressOperation,
    },
}
