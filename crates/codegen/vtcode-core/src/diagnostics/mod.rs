//! Self-healing diagnostics including predictive monitors and recovery playbooks.

mod health;
mod recovery;

pub use health::{DiagnosticReport, HealthSample, PredictiveMonitor};
pub use recovery::{LabeledAction, RecoveryAction, RecoveryPlaybook};
