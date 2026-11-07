#![expect(
    clippy::cast_possible_truncation,
    reason = "Progress percentages are clamped to the documented byte-sized display range."
)]

//! Pure data types with no dependencies beyond `std`.

/// Message kind tag for inline transcript lines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InlineMessageKind {
    Agent,
    Error,
    Info,
    Policy,
    Pty,
    Tool,
    User,
    Warning,
}

/// A single slash-command entry for the suggestion palette.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlashCommandItem {
    pub name: String,
    pub description: String,
}

impl SlashCommandItem {
    pub fn new(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self { name: name.into(), description: description.into() }
    }
}

/// Search configuration for a list overlay.
#[derive(Clone, Debug, Default)]
pub struct InlineListSearchConfig {
    pub label: String,
    pub placeholder: Option<String>,
    /// Match candidates by fuzzy subsequence (via nucleo) and rank results by
    /// relevance, instead of requiring every query term to be a substring.
    /// Opt-in: palettes that filter on enumerated values (e.g. model pickers)
    /// keep exact-term filtering to avoid over-matching.
    pub fuzzy: bool,
}

/// Configuration for a secure (masked) prompt input.
#[derive(Clone, Debug)]
pub struct SecurePromptConfig {
    pub label: String,
    /// Optional placeholder shown when input is empty.
    pub placeholder: Option<String>,
    /// Whether the input should be masked (e.g., API keys).
    pub mask_input: bool,
}

/// Standalone surface preference for selecting inline vs alternate rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SessionSurface {
    Auto,
    Alternate,
    #[default]
    Inline,
}

/// Standalone keyboard protocol settings for terminal key event enhancements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KeyboardProtocolSettings {
    pub enabled: bool,
    pub mode: String,
    pub disambiguate_escape_codes: bool,
    pub report_event_types: bool,
    pub report_alternate_keys: bool,
    pub report_all_keys: bool,
}

impl Default for KeyboardProtocolSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "default".to_owned(),
            disambiguate_escape_codes: true,
            report_event_types: true,
            report_alternate_keys: true,
            report_all_keys: false,
        }
    }
}

/// UI mode variants for quick presets.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiMode {
    #[default]
    Full,
    Minimal,
    Focused,
}

/// Override for responsive layout detection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutModeOverride {
    #[default]
    Auto,
    Compact,
    Standard,
    Wide,
}

/// Reasoning visibility behavior in the transcript.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReasoningDisplayMode {
    Always,
    #[default]
    Toggle,
    Hidden,
}

/// How a busy plain-Enter submission enters the runloop while a turn runs.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum BusyEnterBehavior {
    /// Queue the message for the next turn (default; each Enter stays its own turn).
    #[default]
    Queue,
    /// Steer plain-text input into the live turn via the steering channel.
    Steer,
}

/// Default collapse state of agent thinking/reasoning blocks in the transcript.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ThinkingBlockState {
    /// Thinking blocks render collapsed (a single summary line) by default.
    #[default]
    Collapsed,
    /// Thinking blocks render fully expanded by default.
    Extended,
}

/// Wizard modal behavior variant.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WizardModalMode {
    /// Traditional multi-step wizard behavior (Enter advances/collects answers).
    MultiStep,
    /// Tabbed list behavior (tabs switch categories; Enter submits immediately).
    TabbedList,
}

/// Diff preview layout for file-edit approval overlays.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum DiffPreviewMode {
    /// Single-column unified view (default).
    #[default]
    Inline,
    /// Old/new columns side by side.
    SideBySide,
}

// ---------------------------------------------------------------------------
// Task tracker row status
// ---------------------------------------------------------------------------

/// Display status of a single task tracker row, shared between the agent
/// runloop (which owns checklist state) and the terminal surface (which owns
/// styling). Keeps status typed across the panel/transcript boundary instead
/// of re-parsing glyphs out of display strings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskItemStatus {
    Pending,
    InProgress,
    Completed,
    Blocked,
}

impl TaskItemStatus {
    /// Canonical wire string, matching `TaskTrackingStatus::as_str`.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Blocked => "blocked",
        }
    }
}

impl std::str::FromStr for TaskItemStatus {
    type Err = ();

    /// Parse a canonical status string; unknown shapes fail closed to `Err`.
    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        match raw {
            "pending" => Ok(Self::Pending),
            "in_progress" => Ok(Self::InProgress),
            "completed" => Ok(Self::Completed),
            "blocked" => Ok(Self::Blocked),
            _ => Err(()),
        }
    }
}

// ---------------------------------------------------------------------------
// Plan types
// ---------------------------------------------------------------------------

/// A step in an implementation plan.
#[derive(Clone, Debug)]
pub struct PlanStep {
    pub number: usize,
    pub description: String,
    pub details: Option<String>,
    pub files: Vec<String>,
    pub completed: bool,
}

/// A phase in an implementation plan (groups related steps).
#[derive(Clone, Debug)]
pub struct PlanPhase {
    pub name: String,
    pub steps: Vec<PlanStep>,
    pub completed: bool,
}

/// Structured plan content for display in the Implementation Blueprint panel.
#[derive(Clone, Debug)]
pub struct PlanContent {
    pub title: String,
    pub summary: String,
    pub file_path: Option<String>,
    pub phases: Vec<PlanPhase>,
    pub open_questions: Vec<String>,
    pub raw_content: String,
    pub total_steps: usize,
    pub completed_steps: usize,
}

impl PlanContent {
    /// Parse plan content from markdown.
    pub fn from_markdown(title: String, content: &str, file_path: Option<String>) -> Self {
        let mut phases = Vec::new();
        let mut open_questions = Vec::new();
        let mut current_phase: Option<PlanPhase> = None;
        let mut total_steps = 0;
        let mut completed_steps = 0;
        let mut summary = String::new();
        let mut reading_summary = false;

        for line in content.lines() {
            let trimmed = line.trim();

            // The planning prompt emits both conventional markdown headings
            // (`## Summary`) and sparse section labels (`Summary`). Treat
            // either form as a section marker so the label itself is not
            // displayed as the plan summary.
            if trimmed.eq_ignore_ascii_case("summary") || trimmed.eq_ignore_ascii_case("## summary") {
                reading_summary = true;
                continue;
            }

            if reading_summary {
                if !trimmed.is_empty() {
                    if summary.is_empty() {
                        summary = trimmed.to_string();
                    }
                    reading_summary = false;
                }
                continue;
            }

            // Extract summary from first paragraph
            if summary.is_empty() && !trimmed.is_empty() && !trimmed.starts_with('#') {
                summary = trimmed.to_string();
                continue;
            }

            // Phase headers (## Phase X: ...)
            if let Some(phase_name) = trimmed.strip_prefix("## ") {
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

            if let Some(rest) = trimmed.strip_prefix("[x] ").or_else(|| trimmed.strip_prefix("[X] ")) {
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

            // Question items
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

    /// Get progress as a percentage.
    #[allow(
        clippy::cast_sign_loss,
        reason = "Intentional compatibility, platform, or test-only suppression."
    )]
    pub fn progress_percent(&self) -> u8 {
        if self.total_steps == 0 {
            0
        } else {
            ((self.completed_steps as f32 / self.total_steps as f32) * 100.0) as u8
        }
    }
}

// ---------------------------------------------------------------------------
// App icon preference
// ---------------------------------------------------------------------------

/// Selected application-icon artwork, shared by the desktop Dock/bundle icon
/// picker and the CLI terminal tab icon (`ui.app_icon`).
///
/// `Dynamic` (default) follows the active light/dark mode, while
/// `Light`/`Dark`/`Clear`/`Tinted` pin one artwork. `Unknown` is the serde
/// catch-all for values added by future versions and behaves like `Dynamic`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum AppIconPreference {
    /// Follow the active light/dark mode (default).
    #[default]
    Dynamic,
    /// Always use the light-mode artwork.
    Light,
    /// Always use the dark-mode artwork.
    Dark,
    /// Always use the clear (glass V on a subtle translucent tile) artwork.
    Clear,
    /// Always use the tinted (monochrome slate) artwork.
    Tinted,
    /// Catch-all for unknown values added by future versions.
    #[serde(other)]
    Unknown,
}

impl AppIconPreference {
    /// Selectable variants, in picker order. `Unknown` is never offered.
    pub const ALL: [Self; 5] = [Self::Dynamic, Self::Light, Self::Dark, Self::Clear, Self::Tinted];

    /// Canonical persisted id.
    #[must_use]
    pub const fn id(self) -> &'static str {
        match self {
            Self::Dynamic | Self::Unknown => "dynamic",
            Self::Light => "light",
            Self::Dark => "dark",
            Self::Clear => "clear",
            Self::Tinted => "tinted",
        }
    }

    /// Parse a persisted id; missing or unknown values fall back to `Dynamic`.
    #[must_use]
    pub fn from_id(id: Option<&str>) -> Self {
        match id.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
            Some("light") => Self::Light,
            Some("dark") => Self::Dark,
            Some("clear") => Self::Clear,
            Some("tinted") => Self::Tinted,
            _ => Self::Dynamic,
        }
    }

    /// Resolve to the concrete pinned artwork: `Dynamic`/`Unknown` follow the
    /// given UI mode, while explicit variants pin their own art.
    #[must_use]
    pub const fn resolve(self, dark_mode: bool) -> Self {
        match self {
            Self::Dynamic | Self::Unknown => {
                if dark_mode {
                    Self::Dark
                } else {
                    Self::Light
                }
            }
            pinned => pinned,
        }
    }

    /// Whether the dark-mode artwork applies under the given UI mode.
    ///
    /// Only meaningful for the light/dark/dynamic family: `Clear` and
    /// `Tinted` pin their own non-flat artwork, so this is always false for
    /// them. Prefer [`Self::resolve`] when selecting artwork bytes.
    #[must_use]
    pub const fn is_dark(self, dark_mode: bool) -> bool {
        matches!(self.resolve(dark_mode), Self::Dark)
    }
}

#[cfg(test)]
mod tests {
    use super::PlanContent;
    use crate::ui_protocol::AppIconPreference;

    #[test]
    fn parses_sparse_summary_section_without_displaying_section_label() {
        let plan = PlanContent::from_markdown(
            "Implementation Plan".to_string(),
            "Summary\nFocus on startup latency.\n\n1. Measure startup -> src/startup.rs\n2. Defer refresh -> src/update.rs\n\nValidation\n- cargo check --locked",
            None,
        );

        assert_eq!(plan.summary, "Focus on startup latency.");
        assert_eq!(plan.total_steps, 2);
        assert_eq!(plan.raw_content.lines().next(), Some("Summary"));
    }

    #[test]
    fn app_icon_default_is_dynamic() {
        assert_eq!(AppIconPreference::default(), AppIconPreference::Dynamic);
    }

    #[test]
    fn app_icon_ids_round_trip_through_from_id() {
        for variant in AppIconPreference::ALL {
            assert_eq!(AppIconPreference::from_id(Some(variant.id())), variant, "{}", variant.id());
        }
        assert_eq!(AppIconPreference::from_id(None), AppIconPreference::Dynamic);
    }

    #[test]
    fn app_icon_from_id_is_forgiving_about_case_and_whitespace() {
        assert_eq!(AppIconPreference::from_id(Some(" Light ")), AppIconPreference::Light);
        assert_eq!(AppIconPreference::from_id(Some("DARK")), AppIconPreference::Dark);
        assert_eq!(AppIconPreference::from_id(Some("DYNAMIC")), AppIconPreference::Dynamic);
        assert_eq!(AppIconPreference::from_id(Some(" Clear ")), AppIconPreference::Clear);
        assert_eq!(AppIconPreference::from_id(Some("TINTED")), AppIconPreference::Tinted);
    }

    #[test]
    fn app_icon_from_id_falls_back_to_dynamic_for_unknown_or_empty() {
        for raw in ["", "   ", "twilight", "bright", "dynamic-v2"] {
            assert_eq!(AppIconPreference::from_id(Some(raw)), AppIconPreference::Dynamic, "{raw:?}");
        }
    }

    #[test]
    fn app_icon_is_dark_pins_light_and_dark_but_follows_mode_for_dynamic() {
        for dark_mode in [false, true] {
            assert!(!AppIconPreference::Light.is_dark(dark_mode), "light pins light art");
            assert!(AppIconPreference::Dark.is_dark(dark_mode), "dark pins dark art");
            assert_eq!(AppIconPreference::Dynamic.is_dark(dark_mode), dark_mode, "dynamic follows mode");
            assert_eq!(AppIconPreference::Unknown.is_dark(dark_mode), dark_mode, "unknown follows mode");
            assert!(
                !AppIconPreference::Clear.is_dark(dark_mode),
                "clear pins its own glass art, never the dark flat tile"
            );
            assert!(
                !AppIconPreference::Tinted.is_dark(dark_mode),
                "tinted pins its own slate art, never the dark flat tile"
            );
        }
    }

    #[test]
    fn app_icon_resolve_pins_explicit_variants_and_follows_mode_for_dynamic() {
        for dark_mode in [false, true] {
            for pinned in [
                AppIconPreference::Light,
                AppIconPreference::Dark,
                AppIconPreference::Clear,
                AppIconPreference::Tinted,
            ] {
                assert_eq!(pinned.resolve(dark_mode), pinned, "{pinned:?}");
            }
            let expected = if dark_mode {
                AppIconPreference::Dark
            } else {
                AppIconPreference::Light
            };
            assert_eq!(AppIconPreference::Dynamic.resolve(dark_mode), expected);
            assert_eq!(AppIconPreference::Unknown.resolve(dark_mode), expected);
        }
    }

    #[test]
    fn app_icon_serde_uses_ids_and_catches_unknown_variants() {
        assert_eq!(serde_json::from_str::<AppIconPreference>("\"light\"").unwrap(), AppIconPreference::Light);
        assert_eq!(serde_json::from_str::<AppIconPreference>("\"clear\"").unwrap(), AppIconPreference::Clear);
        assert_eq!(serde_json::from_str::<AppIconPreference>("\"tinted\"").unwrap(), AppIconPreference::Tinted);
        assert_eq!(
            serde_json::from_str::<AppIconPreference>("\"future-variant\"").unwrap(),
            AppIconPreference::Unknown
        );
        assert_eq!(serde_json::to_string(&AppIconPreference::Dynamic).unwrap(), "\"dynamic\"");
        assert_eq!(serde_json::to_string(&AppIconPreference::Clear).unwrap(), "\"clear\"");
        assert_eq!(serde_json::to_string(&AppIconPreference::Tinted).unwrap(), "\"tinted\"");
        assert_eq!(AppIconPreference::Unknown.id(), "dynamic");
    }
}
