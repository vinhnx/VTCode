use std::collections::HashSet;
use std::io::Write;

use crate::config::constants::ui;

use super::Session;

const MAX_TITLE_LENGTH: usize = 128;
const OSC_SET_WINDOW_TITLE: &str = "\u{1b}]0;";
const OSC_TERMINATOR: &str = "\u{7}";

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
    pub fn set_workspace_root(&mut self, workspace_root: Option<std::path::PathBuf>) {
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

        if self.has_status_spinner()
            || left.contains("action required")
            || left.contains("approval")
            || left.contains("input required")
        {
            TerminalTitleStatus::ActionRequired
        } else if left.contains("undo") || left.contains("rewind") || left.contains("revert") {
            TerminalTitleStatus::Undoing
        } else if left.contains("waiting") || left.contains("queued") || left.contains("paused") {
            TerminalTitleStatus::Waiting
        } else if self.thinking_spinner.is_active
            || left.contains("thinking")
            || left.contains("processing")
        {
            TerminalTitleStatus::Thinking
        } else if self.is_running_activity() {
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
            TerminalTitleItem::Model => {
                strip_header_value(&self.header_context.model, ui::HEADER_MODEL_PREFIX)
            }
            TerminalTitleItem::TaskProgress => self.terminal_title_task_progress.clone(),
        }?;

        Some(RenderedTitlePart {
            text,
            spinner: item == TerminalTitleItem::Spinner,
        })
    }

    fn spinner_title_value(&self, status: TerminalTitleStatus) -> Option<String> {
        match status {
            TerminalTitleStatus::Ready => None,
            TerminalTitleStatus::ActionRequired => Some("!".to_string()),
            TerminalTitleStatus::Thinking => {
                Some(self.thinking_spinner.current_frame().to_string())
            }
            TerminalTitleStatus::Working
            | TerminalTitleStatus::Waiting
            | TerminalTitleStatus::Undoing => Some("...".to_string()),
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
                title.push_str(if previous_spinner || part.spinner {
                    " "
                } else {
                    " | "
                });
            }
            title.push_str(&part.text);
        }

        sanitize_terminal_title(&title)
    }

    pub fn update_terminal_title(&mut self) {
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

    pub fn clear_terminal_title(&mut self) {
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

fn write_terminal_title(title: &str) -> std::io::Result<()> {
    let mut stdout = std::io::stdout();
    stdout.write_all(OSC_SET_WINDOW_TITLE.as_bytes())?;
    stdout.write_all(title.as_bytes())?;
    stdout.write_all(OSC_TERMINATOR.as_bytes())?;
    stdout.flush()
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
    const ELLIPSIS: &str = "...";
    let char_count = title.chars().count();
    if char_count <= MAX_TITLE_LENGTH {
        return title.to_string();
    }

    let keep = MAX_TITLE_LENGTH.saturating_sub(ELLIPSIS.len());
    let truncated = title.chars().take(keep).collect::<String>();
    format!("{truncated}{ELLIPSIS}")
}

fn normalize_title_part(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::{
        Session, TerminalTitleStatus, is_stripped_terminal_title_char, normalize_title_part,
        sanitize_terminal_title, truncate_title,
    };

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

        assert_eq!(
            session.render_terminal_title().as_deref(),
            Some("⠋ demo-project")
        );
    }

    #[test]
    fn unavailable_items_are_omitted() {
        let mut session = session_for_title_tests();
        session.terminal_title_items = Some(vec![
            "thread".to_string(),
            "project".to_string(),
            "git-branch".to_string(),
        ]);
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));

        assert_eq!(
            session.render_terminal_title().as_deref(),
            Some("demo-project")
        );
    }

    #[test]
    fn spinner_uses_plain_space_separator() {
        let mut session = session_for_title_tests();
        session.terminal_title_items = Some(vec![
            "project".to_string(),
            "spinner".to_string(),
            "status".to_string(),
        ]);
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));
        session.input_status_left = Some("Thinking".to_string());
        session.thinking_spinner.start();

        assert_eq!(
            session.render_terminal_title().as_deref(),
            Some("demo-project ⠋ Thinking")
        );
    }

    #[test]
    fn status_label_mapping_prefers_short_labels() {
        let mut session = session_for_title_tests();
        session.input_status_left = Some("Action Required: approve command".to_string());
        assert_eq!(
            session.terminal_title_status(),
            TerminalTitleStatus::ActionRequired
        );

        session.input_status_left = Some("Rewinding last turn".to_string());
        assert_eq!(
            session.terminal_title_status(),
            TerminalTitleStatus::Undoing
        );

        session.input_status_left = Some("Waiting for tool".to_string());
        assert_eq!(
            session.terminal_title_status(),
            TerminalTitleStatus::Waiting
        );
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
    fn task_progress_item_uses_parsed_summary() {
        let mut session = session_for_title_tests();
        session.terminal_title_items =
            Some(vec!["task-progress".to_string(), "project".to_string()]);
        session.terminal_title_task_progress = Some("2/5".to_string());
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));

        assert_eq!(
            session.render_terminal_title().as_deref(),
            Some("2/5 | demo-project")
        );
    }

    #[test]
    fn invalid_terminal_title_items_are_ignored() {
        let mut session = session_for_title_tests();
        session.terminal_title_items = Some(vec!["not-real".to_string(), "project".to_string()]);
        session.set_workspace_root(Some(std::path::PathBuf::from("/tmp/demo-project")));

        assert_eq!(
            session.render_terminal_title().as_deref(),
            Some("demo-project")
        );
    }

    #[test]
    fn duplicate_title_items_are_deduplicated() {
        let mut session = session_for_title_tests();
        session.terminal_title_items = Some(vec![
            "thread".to_string(),
            "git-branch".to_string(),
            "status".to_string(),
        ]);
        session.terminal_title_thread_label = Some("main".to_string());
        session.terminal_title_git_branch = Some("main".to_string());
        session.input_status_left = Some("Ready".to_string());

        assert_eq!(
            session.render_terminal_title().as_deref(),
            Some("main | Ready")
        );
    }

    #[test]
    fn normalize_title_part_collapses_spacing_and_case() {
        assert_eq!(normalize_title_part(" Main   Branch "), "main branch");
    }
}
