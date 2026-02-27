/// A step in an implementation plan
#[derive(Clone, Debug)]
pub struct PlanStep {
    /// Step number (1-indexed)
    pub number: usize,
    /// Short description of the step
    pub description: String,
    /// Detailed notes or context
    pub details: Option<String>,
    /// Files to be modified in this step
    pub files: Vec<String>,
    /// Whether this step is completed
    pub completed: bool,
}

/// A phase in an implementation plan (groups related steps)
#[derive(Clone, Debug)]
pub struct PlanPhase {
    /// Phase name (e.g., "Phase 1: Initial Understanding")
    pub name: String,
    /// Steps in this phase
    pub steps: Vec<PlanStep>,
    /// Whether all steps in this phase are completed
    pub completed: bool,
}

/// Structured plan content for display in Implementation Blueprint panel
#[derive(Clone, Debug)]
pub struct PlanContent {
    /// Plan title/name
    pub title: String,
    /// Summary description
    pub summary: String,
    /// Path to the plan file on disk
    pub file_path: Option<String>,
    /// Phases containing implementation steps
    pub phases: Vec<PlanPhase>,
    /// Open questions or issues
    pub open_questions: Vec<String>,
    /// Raw markdown content (for fallback display)
    pub raw_content: String,
    /// Total number of steps
    pub total_steps: usize,
    /// Number of completed steps
    pub completed_steps: usize,
}

impl PlanContent {
    /// Parse plan content from markdown
    pub fn from_markdown(title: String, content: &str, file_path: Option<String>) -> Self {
        let mut phases = Vec::new();
        let mut open_questions = Vec::new();
        let mut current_phase: Option<PlanPhase> = None;
        let mut total_steps = 0;
        let mut completed_steps = 0;
        let mut summary = String::new();

        for line in content.lines() {
            let trimmed = line.trim();

            // Extract summary from first paragraph
            if summary.is_empty() && !trimmed.is_empty() && !trimmed.starts_with('#') {
                summary = trimmed.to_string();
                continue;
            }

            // Phase headers (## Phase X: ...)
            if let Some(phase_name) = trimmed.strip_prefix("## ") {
                // Save previous phase
                if let Some(phase) = current_phase.take() {
                    phases.push(phase);
                }
                current_phase = Some(PlanPhase {
                    name: phase_name.to_string(),
                    steps: Vec::new(),
                    completed: false,
                });
                continue;
            }

            // Open questions section
            if trimmed == "## Open Questions" {
                if let Some(phase) = current_phase.take() {
                    phases.push(phase);
                }
                continue;
            }

            // Step items ([ ] or [x] prefixed)
            if let Some(rest) = trimmed.strip_prefix("[ ] ") {
                total_steps += 1;
                if let Some(ref mut phase) = current_phase {
                    phase.steps.push(PlanStep {
                        number: phase.steps.len() + 1,
                        description: rest.to_string(),
                        details: None,
                        files: Vec::new(),
                        completed: false,
                    });
                }
                continue;
            }

            if let Some(rest) = trimmed
                .strip_prefix("[x] ")
                .or_else(|| trimmed.strip_prefix("[X] "))
            {
                total_steps += 1;
                completed_steps += 1;
                if let Some(ref mut phase) = current_phase {
                    phase.steps.push(PlanStep {
                        number: phase.steps.len() + 1,
                        description: rest.to_string(),
                        details: None,
                        files: Vec::new(),
                        completed: true,
                    });
                }
                continue;
            }

            // Numbered steps (1. **Step 1** ...)
            if trimmed.starts_with(|c: char| c.is_ascii_digit()) && trimmed.contains('.') {
                total_steps += 1;
                if let Some(ref mut phase) = current_phase {
                    let desc = trimmed.split_once('.').map(|x| x.1).unwrap_or("").trim();
                    phase.steps.push(PlanStep {
                        number: phase.steps.len() + 1,
                        description: desc.to_string(),
                        details: None,
                        files: Vec::new(),
                        completed: false,
                    });
                }
                continue;
            }

            // Question items (- (...)
            if trimmed.starts_with("- (") || trimmed.starts_with("- ?") {
                open_questions.push(trimmed.trim_start_matches("- ").to_string());
            }
        }

        // Save last phase
        if let Some(mut phase) = current_phase.take() {
            phase.completed = phase.steps.iter().all(|s| s.completed);
            phases.push(phase);
        }

        // Update phase completion status
        for phase in &mut phases {
            phase.completed = !phase.steps.is_empty() && phase.steps.iter().all(|s| s.completed);
        }

        Self {
            title,
            summary,
            file_path,
            phases,
            open_questions,
            raw_content: content.to_string(),
            total_steps,
            completed_steps,
        }
    }

    /// Get progress as a percentage
    pub fn progress_percent(&self) -> u8 {
        if self.total_steps == 0 {
            0
        } else {
            ((self.completed_steps as f32 / self.total_steps as f32) * 100.0) as u8
        }
    }
}

/// Result of plan confirmation dialog
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanConfirmationResult {
    /// Execute the plan - transition to Edit mode
    Execute,
    /// Clear conversation context and execute with auto-accept enabled
    ClearContextAutoAccept,
    /// Return to planning to edit the plan
    EditPlan,
    /// Cancel execution and stay in Plan mode
    Cancel,
    /// Auto-accept all future plans in this session
    AutoAccept,
}
