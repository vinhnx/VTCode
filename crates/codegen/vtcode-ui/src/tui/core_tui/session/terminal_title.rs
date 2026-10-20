use std::collections::HashSet;
use std::io::Write;

use crate::tui::config::constants::ui;

use super::{Session, title_spinner_frame};

use vtcode_commons::ansi_codes::{set_iterm2_profile, set_terminal_title_and_icon};
use vtcode_commons::terminal_detection::{
    ITERM2_PROFILE_NAME, original_iterm2_profile_name, should_apply_iterm2_profile_now,
};

const MAX_TITLE_LENGTH: usize = 128;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalTitleItem {
    AppName,
    Project,
    Spinner,
    Status,
    Thread,
    GitBranch,
    Model,
    TaskProgress,
}

impl TerminalTitleItem {
    fn from_id(id: &str) -> Option<Self> {
        match id.trim() {
            "app-name" => Some(Self::AppName),
            "project" => Some(Self::Project),
            "spinner" => Some(Self::Spinner),
            "status" => Some(Self::Status),
            "thread" => Some(Self::Thread),
            "git-branch" => Some(Self::GitBranch),
            "model" => Some(Self::Model),
            "task-progress" => Some(Self::TaskProgress),
            _ => None,
        }
    }

    fn default_items() -> [Self; 2] {
        [Self::Spinner, Self::Project]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TerminalTitleStatus {
    Ready,
    Thinking,
    Working,
    Waiting,
    Undoing,
    ActionRequired,
}

/// Attention needles (case-insensitive) matched against the runtime left
/// input status for the `Action Required` classification. Shared with the
/// binary's `/title` preview (`slash_commands/ui/terminal_title.rs`) so a
/// reword cannot desynchronize the preview from the shipped mapping.
pub const TERMINAL_TITLE_ATTENTION_NEEDLES: &[&str] = &[
    "action required",
    "approval",
    "input required",
    "blocked",
    "tools disabled",
    "recovery",
];

/// Undo-word needles for the `Undoing` classification, shared with the
/// `/title` preview.
pub const TERMINAL_TITLE_UNDOING_NEEDLES: &[&str] = &["undo", "rewind", "revert"];

/// Waiting-state word needles for the `Waiting` classification, shared with
/// the `/title` preview.
pub const TERMINAL_TITLE_WAITING_NEEDLES: &[&str] = &["waiting", "queued", "paused"];

impl TerminalTitleStatus {
    fn label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Thinking => "Thinking",
            Self::Working => "Working",
            Self::Waiting => "Waiting",
            Self::Undoing => "Undoing",
            Self::ActionRequired => "Action Required",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RenderedTitlePart {
    text: String,
    spinner: bool,
}

impl Session {
    pub(crate) fn set_workspace_root(&mut self, workspace_root: Option<std::path::PathBuf>) {
        self.workspace_root = workspace_root;
    }

    fn extract_project_name(&self) -> String {
        self.workspace_root
            .as_ref()
            .and_then(|path| {
                path.file_name()
                    .or_else(|| path.parent()?.file_name())
                    .map(|name| name.to_string_lossy().to_string())
            })
            .unwrap_or_else(|| self.app_name.clone())
    }

    fn strip_spinner_prefix(text: &str) -> &str {
        text.trim_start_matches(|c: char| {
            c == '⠋'
                || c == '⠙'
                || c == '⠹'
                || c == '⠸'
                || c == '⠼'
                || c == '⠴'
                || c == '⠦'
                || c == '⠧'
                || c == '⠇'
                || c == '⠏'
                || c == '-'
                || c == '\\'
                || c == '|'
                || c == '/'
                || c == '.'
        })
        .trim_start()
    }

    fn terminal_title_status(&self) -> TerminalTitleStatus {
        let left = self
            .input_status_left
            .as_deref()
            .map(Self::strip_spinner_prefix)
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();

        if self.title_attention_required()
            || TERMINAL_TITLE_ATTENTION_NEEDLES.iter().any(|needle| left.contains(needle))
        {
            TerminalTitleStatus::ActionRequired
        } else if TERMINAL_TITLE_UNDOING_NEEDLES.iter().any(|needle| left.contains(needle)) {
            TerminalTitleStatus::Undoing
        } else if TERMINAL_TITLE_WAITING_NEEDLES.iter().any(|needle| left.contains(needle)) {
            TerminalTitleStatus::Waiting
        } else if self.thinking_spinner.is_active || left.contains("thinking") || left.contains("processing") {
            TerminalTitleStatus::Thinking
        } else if self.loading_activity_active() {
            TerminalTitleStatus::Working
        } else {
            TerminalTitleStatus::Ready
        }
    }

    fn resolve_terminal_title_items(&self) -> Option<Vec<TerminalTitleItem>> {
        let items = match &self.terminal_title_items {
            Some(items) if items.is_empty() => return None,
            Some(items) => items
                .iter()
                .filter_map(|item| TerminalTitleItem::from_id(item))
                .collect::<Vec<_>>(),
            None => TerminalTitleItem::default_items().to_vec(),
        };

        (!items.is_empty()).then_some(items)
    }

    fn title_item_value(&self, item: TerminalTitleItem) -> Option<RenderedTitlePart> {
        let status = self.terminal_title_status();
        let text = match item {
            TerminalTitleItem::AppName => Some(self.app_name.clone()),
            TerminalTitleItem::Project => Some(self.extract_project_name()),
            TerminalTitleItem::Spinner => self.spinner_title_value(status),
            TerminalTitleItem::Status => Some(status.label().to_string()),
            TerminalTitleItem::Thread => self.terminal_title_thread_label.clone(),
            TerminalTitleItem::GitBranch => self.terminal_title_git_branch.clone(),
            TerminalTitleItem::Model => strip_header_value(&self.header_context.model, ui::HEADER_MODEL_PREFIX),
            TerminalTitleItem::TaskProgress => self.terminal_title_task_progress.clone(),
        }?;

        Some(RenderedTitlePart { text, spinner: item == TerminalTitleItem::Spinner })
    }

    fn spinner_title_value(&self, status: TerminalTitleStatus) -> Option<String> {
        match status {
            TerminalTitleStatus::Ready => None,
            TerminalTitleStatus::ActionRequired => Some("!".to_string()),
            _ if self.loading_activity_active() => {
                if self.appearance.should_animate_progress_status() {
                    // Follow the shared shimmer phase so the title and the
                    // footer's loading shimmer advance together.
                    Some(title_spinner_frame(self.shimmer_state.phase()).to_string())
                } else {
                    // Reduced motion / screen reader policy: static label.
                    Some(status.label().to_string())
                }
            }
            // Loading already ended (e.g. Undoing between steps): static label.
            _ => Some(status.label().to_string()),
        }
    }

    fn render_terminal_title(&self) -> Option<String> {
        let items = self.resolve_terminal_title_items()?;
        let mut parts = Vec::new();
        let mut seen = HashSet::new();
        for item in items {
            let Some(part) = self.title_item_value(item) else {
                continue;
            };
            let key = normalize_title_part(&part.text);
            if key.is_empty() || seen.contains(&key) {
                continue;
            }
            seen.insert(key);
            parts.push(part);
        }
        if parts.is_empty() {
            return None;
        }

        let mut title = String::new();
        for (index, part) in parts.iter().enumerate() {
            if index > 0 {
                let previous_spinner = parts[index - 1].spinner;
                title.push_str(if previous_spinner || part.spinner { " " } else { " | " });
            }
            title.push_str(&part.text);
        }

        sanitize_terminal_title(&title)
    }

    pub(crate) fn update_terminal_title(&mut self) {
        let Some(new_title) = self.render_terminal_title() else {
            self.clear_terminal_title();
            return;
        };

        if self.last_terminal_title.as_ref() != Some(&new_title) {
            if let Err(error) = write_terminal_title(&new_title) {
                tracing::debug!(%error, "failed to update terminal title");
            } else {
                self.last_terminal_title = Some(new_title);
            }
        }
    }

    pub(crate) fn clear_terminal_title(&mut self) {
        if self.last_terminal_title.is_none() {
            return;
        }

        if let Err(error) = write_terminal_title("") {
            tracing::debug!(%error, "failed to clear terminal title");
            return;
        }
        self.last_terminal_title = None;
    }
}

fn strip_header_value(value: &str, prefix: &str) -> Option<String> {
    let trimmed = value.trim();
    let stripped = trimmed.strip_prefix(prefix).unwrap_or(trimmed).trim();
    if stripped.is_empty() || stripped == ui::HEADER_UNKNOWN_PLACEHOLDER {
        None
    } else {
        Some(stripped.to_string())
    }
}

fn terminal_title_sequence(title: &str) -> String {
    set_terminal_title_and_icon(title)
}

fn write_terminal_title(title: &str) -> std::io::Result<()> {
    // stderr is the TUI's canonical stream (CrosstermBackend + restore path);
    // stdout may be redirected or a pipe, where title sequences would leak.
    let mut stderr = std::io::stderr();
    stderr.write_all(terminal_title_sequence(title).as_bytes())?;
    stderr.flush()
}

/// One-shot switch to the installed VT Code iTerm2 profile, if applicable.
///
/// Called once at TUI startup (never on title updates): the profile carries
/// the tab icon, and repeated switches would fight manual profile changes
/// mid-session. Sessions without the installed profile file are untouched,
/// so deleting it uninstalls the behavior.
///
/// `OSC 1337;SetProfile=` is a sticky session-profile change with no automatic
/// reversion, so the session's original profile is recorded here and restored
/// by the canonical teardown path ([`restore_tui`](super::super::panic_hook::restore_tui)).
/// Sessions whose original profile is unknown or already [`ITERM2_PROFILE_NAME`]
/// are skipped rather than left switched.
pub(crate) fn apply_iterm2_profile_once() {
    if !cfg!(target_os = "macos") {
        return;
    }
    if !should_apply_iterm2_profile_now() {
        return;
    }
    let Some(original) = original_iterm2_profile_name() else {
        return;
    };
    if let Err(error) = write_iterm2_profile_switch() {
        tracing::debug!(%error, "failed to switch to iTerm2 profile");
        return;
    }
    super::super::panic_hook::state::mark_iterm2_profile_switched(original);
}

fn write_iterm2_profile_switch() -> std::io::Result<()> {
    // Same canonical stream as the title (see `write_terminal_title`).
    let mut stderr = std::io::stderr();
    stderr.write_all(set_iterm2_profile(ITERM2_PROFILE_NAME).as_bytes())?;
    stderr.flush()
}

fn sanitize_terminal_title(title: &str) -> Option<String> {
    let collapsed = title
        .chars()
        .filter_map(|ch| {
            if is_stripped_terminal_title_char(ch) {
                None
            } else if ch.is_control() {
                Some(' ')
            } else {
                Some(ch)
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");

    if collapsed.is_empty() {
        return None;
    }

    Some(truncate_title(&collapsed))
}

fn is_stripped_terminal_title_char(ch: char) -> bool {
    matches!(
        ch,
        '\u{00ad}'
            | '\u{200b}'
            | '\u{200c}'
            | '\u{200d}'
            | '\u{200e}'
            | '\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
    )
}

fn truncate_title(title: &str) -> String {
    vtcode_commons::formatting::truncate_within(title, MAX_TITLE_LENGTH, crate::design::constants::ELLIPSIS_ASCII)
}

fn normalize_title_part(value: &str) -> String {
    vtcode_commons::formatting::collapse_whitespace(value).to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{
        Session, TerminalTitleStatus, is_stripped_terminal_title_char, normalize_title_part, sanitize_terminal_title,
        terminal_title_sequence, title_spinner_frame, truncate_title,
    };
    use crate::tui::core_tui::types::{
        ActivityState, InlineCommand, InlineListItem, InlineListSelection, ListOverlayRequest, OverlayRequest,
    };
    use vtcode_commons::ui_protocol::{ProgressOperation, ProgressPhase, ProgressUpdate};

    fn session_for_title_tests() -> Session {
        let mut session = Session::new(Default::default(), None, 24);
        session.app_name = "VT Code".to_string();
        session
    }

    #[test]
    fn default_title_uses_spinner_and_project_items() {
        let mut session = session_for_title_tests();
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
        session.input_status_left = Some("Thinking".to_string());
        session.thinking_spinner.start();

        assert_eq!(session.render_terminal_title().as_deref(), Some("⠋ demo-project"));
    }

    #[test]
    fn unavailable_items_are_omitted() {
        let mut session = session_for_title_tests();
        session.terminal_title_items =
            Some(vec!["thread".to_string(), "project".to_string(), "git-branch".to_string()]);
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));

        assert_eq!(session.render_terminal_title().as_deref(), Some("demo-project"));
    }

    #[test]
    fn spinner_uses_plain_space_separator() {
        let mut session = session_for_title_tests();
        session.terminal_title_items = Some(vec!["project".to_string(), "spinner".to_string(), "status".to_string()]);
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
        session.input_status_left = Some("Thinking".to_string());
        session.thinking_spinner.start();

        assert_eq!(session.render_terminal_title().as_deref(), Some("demo-project ⠋ Thinking"));
    }

    #[test]
    fn status_label_mapping_prefers_short_labels() {
        let mut session = session_for_title_tests();
        session.input_status_left = Some("Action Required: approve command".to_string());
        assert_eq!(session.terminal_title_status(), TerminalTitleStatus::ActionRequired);

        session.input_status_left = Some("Rewinding last turn".to_string());
        assert_eq!(session.terminal_title_status(), TerminalTitleStatus::Undoing);

        session.input_status_left = Some("Waiting for tool".to_string());
        assert_eq!(session.terminal_title_status(), TerminalTitleStatus::Waiting);
    }

    #[test]
    fn explicit_empty_items_disable_title_updates() {
        let mut session = session_for_title_tests();
        session.terminal_title_items = Some(Vec::new());
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));

        assert_eq!(session.render_terminal_title(), None);
    }

    #[test]
    fn sanitization_strips_control_and_bidi_chars() {
        let sanitized = sanitize_terminal_title("demo\u{1b}]0;bad\u{7}\u{202e} title\tok")
            .expect("title should survive sanitization");

        assert_eq!(sanitized, "demo ]0;bad title ok");
    }

    #[test]
    fn invisible_formatting_chars_are_removed() {
        assert!(is_stripped_terminal_title_char('\u{2066}'));
        assert!(is_stripped_terminal_title_char('\u{200b}'));
        assert!(!is_stripped_terminal_title_char('a'));
    }

    #[test]
    fn sanitization_returns_none_when_title_is_empty_after_cleanup() {
        assert_eq!(sanitize_terminal_title("\u{200b}\u{202e}\t"), None);
    }

    #[test]
    fn title_truncation_caps_length() {
        let title = "x".repeat(200);
        let truncated = truncate_title(&title);

        assert_eq!(truncated.chars().count(), 128);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn task_progress_item_uses_metadata_progress_label() {
        let mut session = session_for_title_tests();
        session.terminal_title_items = Some(vec!["task-progress".to_string(), "project".to_string()]);
        session.terminal_title_task_progress = Some("2/5".to_string());
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));

        assert_eq!(session.render_terminal_title().as_deref(), Some("2/5 | demo-project"));
    }

    #[test]
    fn task_panel_metadata_drives_progress_label() {
        let mut session = session_for_title_tests();
        session.terminal_title_items = Some(vec!["task-progress".to_string()]);
        session.set_task_panel_progress_label(Some("3/8".to_string()));

        assert_eq!(session.terminal_title_task_progress.as_deref(), Some("3/8"));
        assert_eq!(session.render_terminal_title().as_deref(), Some("3/8"));
    }

    #[test]
    fn invalid_terminal_title_items_are_ignored() {
        let mut session = session_for_title_tests();
        session.terminal_title_items = Some(vec!["not-real".to_string(), "project".to_string()]);
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));

        assert_eq!(session.render_terminal_title().as_deref(), Some("demo-project"));
    }

    #[test]
    fn duplicate_title_items_are_deduplicated() {
        let mut session = session_for_title_tests();
        session.terminal_title_items = Some(vec!["thread".to_string(), "git-branch".to_string(), "status".to_string()]);
        session.terminal_title_thread_label = Some("main".to_string());
        session.terminal_title_git_branch = Some("main".to_string());
        session.input_status_left = Some("Ready".to_string());

        assert_eq!(session.render_terminal_title().as_deref(), Some("main | Ready"));
    }

    #[test]
    fn normalize_title_part_collapses_spacing_and_case() {
        assert_eq!(normalize_title_part(" Main   Branch "), "main branch");
    }

    #[test]
    fn title_sequence_sets_icon_before_title() {
        assert_eq!(terminal_title_sequence("demo-project"), "\u{1b}]1;demo-project\u{7}\u{1b}]2;demo-project\u{7}");
    }

    #[test]
    fn title_sequence_clear_resets_both_icon_and_title() {
        assert_eq!(terminal_title_sequence(""), "\u{1b}]1;\u{7}\u{1b}]2;\u{7}");
    }

    #[test]
    fn title_sequence_carries_sanitized_asymmetric_title() {
        let sanitized =
            sanitize_terminal_title("alpha\u{1b}]0;bad\u{7}\u{202e} beta").expect("title should survive sanitization");
        assert_eq!(sanitized, "alpha ]0;bad beta");
        assert_eq!(
            terminal_title_sequence(&sanitized),
            "\u{1b}]1;alpha ]0;bad beta\u{7}\u{1b}]2;alpha ]0;bad beta\u{7}"
        );
    }
    #[test]
    fn thinking_title_respects_accessibility_policy() {
        for (reduced, keep_animation, screen_reader, expected) in [
            (false, false, false, "⠋ demo-project"),
            (true, false, false, "Thinking demo-project"),
            (true, true, false, "⠋ demo-project"),
            (false, true, true, "Thinking demo-project"),
            (true, true, true, "Thinking demo-project"),
        ] {
            let mut session = session_for_title_tests();
            session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
            session.thinking_spinner.start();
            session.appearance.reduce_motion_mode = reduced;
            session.appearance.reduce_motion_keep_progress_animation = keep_animation;
            session.appearance.screen_reader_mode = screen_reader;
            assert_eq!(session.render_terminal_title().as_deref(), Some(expected));
        }
    }

    // ─── Global loading sync ─────────────────────────────────────────────────

    #[test]
    fn busy_tool_status_keeps_spinner_and_reports_working() {
        let mut session = session_for_title_tests();
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
        session.handle_command(InlineCommand::SetInputStatus {
            left: Some("Running tool: read_file".to_string()),
            right: None,
        });

        assert_eq!(session.terminal_title_status(), TerminalTitleStatus::Working);
        // The shared shimmer phase starts at zero, so the title carries the
        // Braille head — never the ActionRequired "!" the old mapping produced.
        assert_eq!(session.render_terminal_title().as_deref(), Some("⠋ demo-project"));
    }

    #[test]
    fn busy_tool_status_title_advances_and_clears_when_loading_ends() {
        let mut session = session_for_title_tests();
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
        session.handle_command(InlineCommand::SetInputStatus {
            left: Some("Running tool: read_file".to_string()),
            right: None,
        });
        let loaded = session.render_terminal_title().expect("title while loading");

        // The title frame follows the shared shimmer phase under the global
        // loading tick instead of freezing once the thinking spinner stops.
        let mut advanced = loaded.clone();
        for _ in 0..8 {
            std::thread::sleep(Duration::from_millis(50));
            session.handle_tick();
            advanced = session.render_terminal_title().expect("title while loading");
            if advanced != loaded {
                break;
            }
        }
        assert_ne!(advanced, loaded, "title spinner must advance under the loading tick");

        session.handle_command(InlineCommand::SetInputStatus { left: None, right: None });
        assert_eq!(session.render_terminal_title().as_deref(), Some("demo-project"));
    }

    #[test]
    fn shared_shimmer_phase_maps_to_braille_frames() {
        // turns-per-sweep = 2: index = floor((2·phase mod 1) · 10), derived
        // independently from the frame table rather than by reusing the
        // production mapping.
        for (phase, expected) in [
            (0.0, "⠋"),
            (0.06, "⠙"),
            (0.30, "⠦"),
            (0.49, "⠏"),
            (0.50, "⠋"),
            (0.75, "⠴"),
            (1.0, "⠋"),
        ] {
            assert_eq!(title_spinner_frame(phase), expected, "phase {phase}");
        }
    }

    #[test]
    fn blocked_turn_keeps_action_required_attention() {
        let mut session = session_for_title_tests();
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
        session.handle_command(InlineCommand::SetActivityState(ActivityState::Blocked));

        assert_eq!(session.terminal_title_status(), TerminalTitleStatus::ActionRequired);
        let title = session.render_terminal_title().expect("blocked title");
        assert!(title.contains('!'), "blocked title should carry the attention bang: {title}");

        // A blocked turn is quiescent: the loading tick must not animate it.
        std::thread::sleep(Duration::from_millis(160));
        session.handle_tick();
        assert_eq!(session.render_terminal_title().as_deref(), Some(title.as_str()));
    }

    #[test]
    fn approval_modal_requires_attention_but_pickers_do_not() {
        let mut approval = session_for_title_tests();
        approval.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
        approval.handle_command(InlineCommand::ShowOverlay {
            request: Box::new(OverlayRequest::List(ListOverlayRequest {
                title: "VT Code approval required".to_string(),
                lines: vec!["Allow this edit".to_string()],
                footer_hint: None,
                items: vec![InlineListItem {
                    title: "Approve".to_string(),
                    selection: Some(InlineListSelection::SlashCommand("approve".to_string())),
                    ..Default::default()
                }],
                selected: None,
                search: None,
                hotkeys: Vec::new(),
                status: None,
            })),
        });
        assert!(approval.title_attention_required());
        assert!(
            approval.render_terminal_title().expect("approval title").contains('!'),
            "approval overlays flag the title"
        );

        let mut picker = session_for_title_tests();
        picker.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
        picker.handle_command(InlineCommand::ShowOverlay {
            request: Box::new(OverlayRequest::List(ListOverlayRequest {
                title: "Terminal title setup".to_string(),
                lines: vec!["Choose items".to_string()],
                footer_hint: None,
                items: vec![InlineListItem {
                    title: "Save".to_string(),
                    selection: Some(InlineListSelection::SlashCommand("save".to_string())),
                    ..Default::default()
                }],
                selected: None,
                search: None,
                hotkeys: Vec::new(),
                status: None,
            })),
        });
        assert!(!picker.title_attention_required(), "configuring the title must not flag action required at itself");
        assert_ne!(picker.terminal_title_status(), TerminalTitleStatus::ActionRequired);
    }

    #[test]
    fn typed_approval_selections_flag_attention_despite_other_titles() {
        // Production approval modals are not guaranteed to carry needles in
        // their title (lifecycle hooks, shell permission prompts), so typed
        // HITL selections must be authoritative for the `!` spinner value.
        let mut session = session_for_title_tests();
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
        session.handle_command(InlineCommand::ShowOverlay {
            request: Box::new(OverlayRequest::List(ListOverlayRequest {
                title: "Would you like to run the following command?".to_string(),
                lines: Vec::new(),
                footer_hint: None,
                items: vec![
                    InlineListItem {
                        title: "Yes".to_string(),
                        selection: Some(InlineListSelection::ToolApproval(true)),
                        ..Default::default()
                    },
                    InlineListItem {
                        title: "No".to_string(),
                        selection: Some(InlineListSelection::ToolApprovalDenyOnce),
                        ..Default::default()
                    },
                ],
                selected: None,
                search: None,
                hotkeys: Vec::new(),
                status: None,
            })),
        });

        assert!(
            session.title_attention_required(),
            "typed approval selections flag attention regardless of title wording"
        );
        let title = session.render_terminal_title().expect("approval title");
        assert!(title.contains('!'), "approval overlay flags the title: {title}");
    }

    #[test]
    fn reduced_motion_loading_uses_static_label() {
        let mut session = session_for_title_tests();
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
        session.appearance.reduce_motion_mode = true;
        session.handle_command(InlineCommand::SetInputStatus {
            left: Some("Running tool: read_file".to_string()),
            right: None,
        });

        assert_eq!(session.terminal_title_status(), TerminalTitleStatus::Working);
        let title = session.render_terminal_title().expect("reduced-motion title");
        assert!(title.contains("Working"), "static loading label expected: {title}");
        assert!(
            !title.chars().any(|ch| (ch as u32) >= 0x2800 && (ch as u32) <= 0x28ff),
            "reduced motion must not animate Braille frames: {title}"
        );
    }

    #[test]
    fn background_only_activity_keeps_title_loader_spinning() {
        let mut session = session_for_title_tests();
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
        session.set_background_activity_count(2);

        assert_eq!(session.terminal_title_status(), TerminalTitleStatus::Working);
        assert_eq!(session.render_terminal_title().as_deref(), Some("⠋ demo-project"));
    }

    #[test]
    fn loading_observers_keep_the_animation_tick_alive() {
        // Every canonical loading observer must both flag the title spinner
        // and keep `needs_animation_tick` true; the title only advances while
        // the event loop is actually ticking.
        let mut session = session_for_title_tests();
        session.thinking_spinner.start();
        assert!(session.loading_activity_active() && session.needs_animation_tick(), "thinking");
        session.thinking_spinner.stop();

        session.handle_command(InlineCommand::SetInputStatus {
            left: Some("Running tool: read_file".to_string()),
            right: None,
        });
        assert!(session.loading_activity_active() && session.needs_animation_tick(), "tool activity");

        let operation = ProgressOperation::start();
        session.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation,
            phase: ProgressPhase::ReceivingResponse,
        }));
        assert!(session.loading_activity_active() && session.needs_animation_tick(), "progress row");

        session.set_background_activity_count(1);
        assert!(session.loading_activity_active() && session.needs_animation_tick(), "background work");

        // Static attention is not loading: waiting-for-approval rows and a
        // blocked turn must stay still even when nothing animates.
        let mut waiting = session_for_title_tests();
        let approval_operation = ProgressOperation::start();
        waiting.handle_command(InlineCommand::UpdateProgress(ProgressUpdate::Begin {
            operation: approval_operation,
            phase: ProgressPhase::WaitingForApproval,
        }));
        assert!(
            !waiting.loading_activity_active() && !waiting.needs_animation_tick(),
            "waiting-for-approval is static attention, not loading"
        );

        let mut blocked = session_for_title_tests();
        blocked.handle_command(InlineCommand::SetActivityState(ActivityState::Blocked));
        blocked.set_background_activity_count(1);
        assert!(!blocked.loading_activity_active(), "a blocked turn is quiescent for the title");
    }
}
