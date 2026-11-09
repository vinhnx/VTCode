/// Sidebar snapshot of the session's approved plan, pushed once at the
/// approval boundary and cleared on the next planning entry.
///
/// The runloop owns the lifecycle (`set_active_plan` / `None`); this type is
/// display-only. Step titles are pre-truncated by the builder — the sidebar
/// renders every received title plus a `+N more` row when
/// [`ActivePlanEntry::remaining_step_count`] is nonzero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ActivePlanEntry {
    /// Workspace-relative plan path for display (e.g. `.vtcode/plans/task.md`).
    pub file_path: String,
    /// Preformatted approval badge (e.g. `approved -> build`).
    pub status: String,
    /// Flat implementation-step titles, capped by the builder.
    pub step_titles: Vec<String>,
    /// True implementation-step total (titles may be truncated).
    pub total_steps: usize,
}

impl ActivePlanEntry {
    /// Steps beyond the truncated sidebar list. Saturates instead of
    /// underflowing when the builder sends more titles than the total.
    #[must_use]
    pub fn remaining_step_count(&self) -> usize {
        self.total_steps.saturating_sub(self.step_titles.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(step_titles: usize, total_steps: usize) -> ActivePlanEntry {
        ActivePlanEntry {
            file_path: ".vtcode/plans/task.md".to_string(),
            status: "approved -> build".to_string(),
            step_titles: (0..step_titles).map(|n| format!("step {n}")).collect(),
            total_steps,
        }
    }

    #[test]
    fn remaining_counts_truncated_steps() {
        assert_eq!(entry(3, 10).remaining_step_count(), 7);
    }

    #[test]
    fn remaining_is_zero_when_nothing_truncated() {
        assert_eq!(entry(4, 4).remaining_step_count(), 0);
    }

    #[test]
    fn remaining_saturates_when_titles_exceed_total() {
        assert_eq!(entry(6, 2).remaining_step_count(), 0);
    }
}
