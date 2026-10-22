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
    CheckingToolOutput,
    Compacting {
        estimated_input_tokens: Option<usize>,
        started_at: Instant,
    },
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
            Self::CheckingToolOutput => "Checking tool output...",
            Self::Compacting { .. } => "Compacting context",
        }
    }

    pub fn is_animated(self) -> bool {
        self != Self::WaitingForApproval
    }

    /// Elapsed whole seconds for this phase, using a phase-local clock only
    /// when the operation is being shown as compaction progress.
    pub fn elapsed_secs(self, operation: ProgressOperation, now: Instant) -> u64 {
        let started_at = match self {
            Self::Compacting { started_at, .. } => started_at,
            _ => operation.started_at(),
        };
        now.saturating_duration_since(started_at).as_secs()
    }

    pub fn format(self, elapsed_secs: u64) -> String {
        if let Self::Compacting { estimated_input_tokens, .. } = self {
            let estimate = estimated_input_tokens
                .map(|tokens| format!(" · ~{} input tokens", crate::format_token_count(tokens)))
                .unwrap_or_default();
            return if elapsed_secs == 0 {
                format!("{}{}", self.label(), estimate)
            } else {
                format!("{}{} · {elapsed_secs}s", self.label(), estimate)
            };
        }
        if elapsed_secs == 0 {
            self.label().to_string()
        } else {
            format!("{} · {elapsed_secs}s", self.label())
        }
    }

    /// Format a phase for a bounded transcript row, preserving the most useful
    /// compaction details when the full label would be truncated.
    pub fn format_for_width(self, elapsed_secs: u64, width: usize) -> String {
        let formatted = self.format(elapsed_secs);
        if formatted.chars().count() <= width {
            return formatted;
        }

        let Self::Compacting { estimated_input_tokens, .. } = self else {
            return formatted;
        };
        let compact = match estimated_input_tokens {
            Some(tokens) => format!("Compact ~{} · {elapsed_secs}s", crate::format_token_count(tokens)),
            None => format!("Compact · {elapsed_secs}s"),
        };
        if compact.chars().count() <= width {
            return compact;
        }
        // A longer timer must not switch back to the detailed label and hide
        // both metrics on the same narrow row.
        match estimated_input_tokens {
            Some(tokens) => format!("Compact ~{} {elapsed_secs}s", crate::format_token_count(tokens)),
            None => compact,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn compaction_phase_uses_its_own_elapsed_clock() {
        let operation = ProgressOperation::start();
        let started_at = operation.started_at() + Duration::from_secs(10);
        let phase = ProgressPhase::Compacting { estimated_input_tokens: Some(12_000), started_at };
        let now = started_at + Duration::from_secs(74);

        assert_eq!(phase.elapsed_secs(operation, now), 74);
        assert_eq!(ProgressPhase::WaitingForModel.elapsed_secs(operation, now), 84);
    }
}
