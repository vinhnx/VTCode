use hashbrown::{HashMap, HashSet};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::time::{Duration, Instant};

/// Rebindable user-facing actions.
///
/// Each variant corresponds to a command-level action that a user may want to
/// remap.  Fine-grained editing shortcuts (character insertion, cursor movement,
/// text selection, Backspace, Delete, Home/End, Ctrl+A/E/W/U/K, Enter/Tab/Esc
/// with their context-sensitive logic) remain hardcoded in `events.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    /// Send an interrupt signal (Ctrl+C) to the running agent.
    Interrupt,
    /// Exit the TUI session.
    Exit,
    /// Run the current operation in the background.
    BackgroundOperation,
    /// Open the model picker dialog.
    OpenModelPicker,
    /// Clear the terminal screen.
    ClearScreen,
    /// Scroll the conversation view up by one page.
    ScrollPageUp,
    /// Scroll the conversation view down by one page.
    ScrollPageDown,
    /// Open the input edit queue for reordering queued prompts.
    EditQueue,
    /// Recall the previous input from history.
    HistoryPrevious,
    /// Recall the next input from history.
    HistoryNext,
    /// Toggle the log panel visibility.
    ToggleLogs,
    /// Toggle compact per-call tool summaries.
    ToggleToolDisplayMode,
    /// Toggle the task panel visibility.
    ToggleTaskPanel,
    /// Open or close the whole-conversation transcript review overlay.
    OpenTranscriptReview,
    /// Toggle rich and raw transcript review rendering.
    ToggleTranscriptRenderMode,
    /// Generate an inline prompt suggestion via the LLM.
    GeneratePromptSuggestion,
    /// Jump to the most recent transcript change, pinned to the bottom edge.
    JumpToLastChange,
    /// Dismiss key-capturing overlays, pickers, and search states and return
    /// to the composer input, without Esc's interrupt or rewind side effects.
    FocusInput,
}

impl Action {
    /// Human-readable name for config file serialization.
    fn name(self) -> &'static str {
        match self {
            Action::Interrupt => "interrupt",
            Action::Exit => "exit",
            Action::BackgroundOperation => "background_operation",
            Action::OpenModelPicker => "open_model_picker",
            Action::ClearScreen => "clear_screen",
            Action::ScrollPageUp => "scroll_page_up",
            Action::ScrollPageDown => "scroll_page_down",
            Action::EditQueue => "edit_queue",
            Action::HistoryPrevious => "history_previous",
            Action::HistoryNext => "history_next",
            Action::ToggleLogs => "toggle_logs",
            Action::ToggleToolDisplayMode => "toggle_tool_display_mode",
            Action::ToggleTaskPanel => "toggle_task_panel",
            Action::OpenTranscriptReview => "open_transcript_review",
            Action::ToggleTranscriptRenderMode => "toggle_transcript_render_mode",
            Action::GeneratePromptSuggestion => "generate_prompt_suggestion",
            Action::JumpToLastChange => "jump_to_last_change",
            Action::FocusInput => "focus_input",
        }
    }

    /// All defined actions.
    fn all() -> &'static [Action] {
        &[
            Action::Interrupt,
            Action::Exit,
            Action::BackgroundOperation,
            Action::OpenModelPicker,
            Action::ClearScreen,
            Action::ScrollPageUp,
            Action::ScrollPageDown,
            Action::EditQueue,
            Action::HistoryPrevious,
            Action::HistoryNext,
            Action::ToggleLogs,
            Action::ToggleToolDisplayMode,
            Action::ToggleTaskPanel,
            Action::OpenTranscriptReview,
            Action::ToggleTranscriptRenderMode,
            Action::GeneratePromptSuggestion,
            Action::JumpToLastChange,
            Action::FocusInput,
        ]
    }

    /// Look up an action by its serialized name.
    fn from_name(name: &str) -> Option<Self> {
        Self::all().iter().find(|a| a.name() == name).copied()
    }
}

/// Parse a key binding spec like `"ctrl+c"`, `"alt+shift+enter"`, `"pageup"`.
///
/// Supported modifiers: `ctrl`, `shift`, `alt`, `meta`, `cmd`, `super`.
/// Key names: single characters (`a`, `?`), `enter`, `tab`, `backtab`, `esc`,
/// `backspace`, `delete`, `space`, `up`, `down`, `left`, `right`, `pageup`,
/// `pagedown`, `home`, `end`, `f1`…`f12`.
pub fn parse_key_binding(s: &str) -> Option<(KeyCode, KeyModifiers)> {
    let normalized = s.trim().to_ascii_lowercase();
    let s = normalized.as_str();
    if s.is_empty() {
        return None;
    }

    let parts: Vec<&str> = s.split('+').collect();
    let (modifiers, key_part) = if parts.len() == 1 {
        (KeyModifiers::empty(), parts[0])
    } else {
        let mut mods = KeyModifiers::empty();
        for part in &parts[..parts.len() - 1] {
            match *part {
                "ctrl" | "control" => mods.insert(KeyModifiers::CONTROL),
                "shift" => mods.insert(KeyModifiers::SHIFT),
                "alt" | "option" => mods.insert(KeyModifiers::ALT),
                "meta" => mods.insert(KeyModifiers::META),
                "cmd" | "command" | "super" | "gui" | "win" => {
                    mods.insert(KeyModifiers::SUPER);
                }
                _ => return None,
            }
        }
        (mods, parts[parts.len() - 1])
    };

    let code = match key_part {
        "enter" => KeyCode::Enter,
        "tab" => KeyCode::Tab,
        "backtab" => KeyCode::BackTab,
        "esc" | "escape" => KeyCode::Esc,
        "backspace" => KeyCode::Backspace,
        "delete" => KeyCode::Delete,
        "space" => KeyCode::Char(' '),
        "up" => KeyCode::Up,
        "down" => KeyCode::Down,
        "left" => KeyCode::Left,
        "right" => KeyCode::Right,
        "pageup" => KeyCode::PageUp,
        "pagedown" => KeyCode::PageDown,
        "home" => KeyCode::Home,
        "end" => KeyCode::End,
        "insert" => KeyCode::Insert,
        "null" => KeyCode::Null,
        "capslock" => KeyCode::CapsLock,
        "scrolllock" => KeyCode::ScrollLock,
        "numlock" => KeyCode::NumLock,
        "printscreen" => KeyCode::PrintScreen,
        "pause" => KeyCode::Pause,
        "menu" => KeyCode::Menu,
        name if name.starts_with('f') && name.len() > 1 => {
            let n: u8 = name[1..].parse().ok()?;
            match n {
                1 => KeyCode::F(1),
                2 => KeyCode::F(2),
                3 => KeyCode::F(3),
                4 => KeyCode::F(4),
                5 => KeyCode::F(5),
                6 => KeyCode::F(6),
                7 => KeyCode::F(7),
                8 => KeyCode::F(8),
                9 => KeyCode::F(9),
                10 => KeyCode::F(10),
                11 => KeyCode::F(11),
                12 => KeyCode::F(12),
                _ => return None,
            }
        }
        ch => {
            let chars: Vec<char> = ch.chars().collect();
            if chars.len() == 1 {
                KeyCode::Char(chars[0])
            } else {
                return None;
            }
        }
    };

    Some((code, modifiers))
}

/// Default key → action mappings matching the current hardcoded dispatch.
fn default_bindings() -> HashMap<Action, Vec<(KeyCode, KeyModifiers)>> {
    use Action::*;
    let mut m = HashMap::new();

    m.insert(
        Interrupt,
        vec![
            (KeyCode::Char('c'), KeyModifiers::CONTROL),
            (KeyCode::Char('C'), KeyModifiers::CONTROL),
            (KeyCode::Char('\u{3}'), KeyModifiers::empty()),
        ],
    );
    m.insert(
        Exit,
        vec![
            (KeyCode::Char('d'), KeyModifiers::CONTROL),
            (KeyCode::Char('D'), KeyModifiers::CONTROL),
        ],
    );
    m.insert(
        BackgroundOperation,
        vec![
            (KeyCode::Char('b'), KeyModifiers::CONTROL),
            (KeyCode::Char('B'), KeyModifiers::CONTROL),
        ],
    );
    m.insert(
        OpenModelPicker,
        vec![
            (KeyCode::Char('m'), KeyModifiers::CONTROL),
            (KeyCode::Char('M'), KeyModifiers::CONTROL),
        ],
    );
    m.insert(
        ClearScreen,
        vec![
            (KeyCode::Char('l'), KeyModifiers::CONTROL),
            (KeyCode::Char('L'), KeyModifiers::CONTROL),
        ],
    );
    m.insert(ScrollPageUp, vec![(KeyCode::PageUp, KeyModifiers::empty())]);
    m.insert(ScrollPageDown, vec![(KeyCode::PageDown, KeyModifiers::empty())]);

    m.insert(EditQueue, vec![(KeyCode::Up, KeyModifiers::ALT), (KeyCode::Up, KeyModifiers::META)]);

    m.insert(HistoryPrevious, vec![(KeyCode::Up, KeyModifiers::empty())]);
    m.insert(HistoryNext, vec![(KeyCode::Down, KeyModifiers::empty())]);

    m.insert(
        ToggleToolDisplayMode,
        vec![
            (KeyCode::Char('t'), KeyModifiers::ALT),
            (KeyCode::Char('T'), KeyModifiers::ALT),
        ],
    );
    m.insert(
        ToggleTaskPanel,
        vec![
            (KeyCode::Char('g'), KeyModifiers::ALT),
            (KeyCode::Char('G'), KeyModifiers::ALT),
        ],
    );
    m.insert(
        OpenTranscriptReview,
        vec![
            (KeyCode::Char('t'), KeyModifiers::CONTROL),
            (KeyCode::Char('T'), KeyModifiers::CONTROL),
        ],
    );
    m.insert(ToggleTranscriptRenderMode, vec![(KeyCode::Char('r'), KeyModifiers::empty())]);
    m.insert(
        GeneratePromptSuggestion,
        vec![
            (KeyCode::Char('p'), KeyModifiers::ALT),
            (KeyCode::Char('P'), KeyModifiers::ALT),
        ],
    );
    m.insert(JumpToLastChange, vec![(KeyCode::End, KeyModifiers::CONTROL)]);
    m.insert(
        FocusInput,
        vec![
            (KeyCode::Char('i'), KeyModifiers::ALT),
            (KeyCode::Char('I'), KeyModifiers::ALT),
        ],
    );

    m
}

/// Compiled store of key → action mappings, built from defaults + user overrides.
///
/// Lookup is O(number of mapped keys) — the total is small (<50 entries) so a
/// simple linear scan is faster than a nested hash map.
#[derive(Debug, Clone)]
pub struct BindingStore {
    /// Flat list of (key, modifiers) → action for O(n) scan.
    entries: Vec<(KeyCode, KeyModifiers, Action)>,
    /// First configured key for each action, used by UI affordances.
    primary_labels: HashMap<Action, String>,
    /// Actions whose defaults were explicitly replaced or unbound.
    overridden_actions: HashSet<Action>,
}

impl Default for BindingStore {
    fn default() -> Self {
        Self::defaults()
    }
}

impl BindingStore {
    /// Build from a user-provided overlay on top of the built-in defaults.
    ///
    /// `overlay` is a `HashMap<action_name, Vec<key_spec_string>>` — exactly
    /// the shape of `KeyBindingConfig::bindings` and
    /// `UserPreferences::keybindings`.
    pub(crate) fn new(overlay: HashMap<String, Vec<String>>) -> Self {
        let mut merged: HashMap<Action, Vec<(KeyCode, KeyModifiers)>> = default_bindings();
        let mut configured_actions = HashSet::new();

        for (action_name, key_specs) in overlay {
            let Some(action) = Action::from_name(&action_name) else {
                tracing::debug!(%action_name, "unknown action in keybinding overlay, skipping");
                continue;
            };

            let parsed: Vec<(KeyCode, KeyModifiers)> = key_specs.iter().filter_map(|s| parse_key_binding(s)).collect();

            if key_specs.is_empty() {
                // Empty list → unbind (remove defaults).
                merged.remove(&action);
                configured_actions.insert(action);
            } else if parsed.is_empty() {
                tracing::warn!(
                    action = action.name(),
                    "keybinding overlay contains no valid bindings; keeping defaults"
                );
            } else {
                merged.insert(action, parsed);
                configured_actions.insert(action);
            }
        }

        let mut entries = Vec::new();
        // HashMap iteration is intentionally randomized. Keep configured
        // actions ahead of defaults, but use the stable Action::all order for
        // both groups so a collision always has the same winner.
        for action in Action::all()
            .iter()
            .copied()
            .filter(|action| configured_actions.contains(action))
            .chain(
                Action::all()
                    .iter()
                    .copied()
                    .filter(|action| !configured_actions.contains(action)),
            )
        {
            let Some(keys) = merged.get(&action) else {
                continue;
            };
            for &(code, mods) in keys {
                entries.push((code, mods, action));
            }
        }

        // Derive labels from the effective entries rather than the requested
        // lists. If two actions claim the same key, the deterministic entry
        // order above decides the winner; the losing action must not advertise
        // a shortcut that can never reach it.
        let mut primary_labels = HashMap::new();
        let mut claimed_bindings = Vec::with_capacity(entries.len());
        for &(code, mods, action) in &entries {
            let is_reachable = !claimed_bindings
                .iter()
                .any(|&(claimed_code, claimed_mods)| binding_covers(claimed_code, claimed_mods, code, mods));
            if is_reachable {
                primary_labels.entry(action).or_insert_with(|| format_key_label(code, mods));
            }
            claimed_bindings.push((code, mods));
        }

        Self {
            entries,
            primary_labels,
            overridden_actions: configured_actions,
        }
    }

    /// Build with only the default bindings.
    fn defaults() -> Self {
        Self::new(HashMap::new())
    }

    /// Look up the action bound to a given key event.
    ///
    /// Returns `None` when the key has no binding (fall through to hardcoded
    /// dispatch).
    pub(crate) fn resolve(&self, key: &KeyEvent) -> Option<Action> {
        // Terminals may report Ctrl+letter as the corresponding C0 control
        // character without a CONTROL modifier. Normalize that form before
        // matching so configured bindings behave consistently across terminal
        // implementations (notably macOS terminal paths for Ctrl+T).
        let (key_code, key_modifiers) = normalize_terminal_control_key(key.code, key.modifiers);
        let mut best: Option<(usize, Action)> = None;

        // Iterate entries. For `Char` codes we also try a case-insensitive
        // match to handle terminal ambiguity (e.g. Ctrl+C vs Ctrl+Shift+C).
        for (i, &(code, mods, action)) in self.entries.iter().enumerate() {
            let code_match = match (code, key_code) {
                (KeyCode::Char(bc), KeyCode::Char(kc)) if bc.eq_ignore_ascii_case(&kc) => true,
                _ => code == key_code,
            };

            if !code_match {
                continue;
            }

            // All declared modifiers must be present.
            if !key_modifiers.contains(mods) {
                continue;
            }

            // For Char codes, SHIFT is already reflected in character case,
            // so allow it as an "extra" modifier without penalty.
            let char_shift_grace = if let KeyCode::Char(_) = key_code {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::empty()
            };
            let extra = key_modifiers.difference(mods);
            if extra.intersection(!char_shift_grace) != KeyModifiers::empty() {
                continue;
            }

            // Prefer earlier entries (user overrides come first, or we use
            // insertion order and give priority to the first binding).
            best = match best {
                None => Some((i, action)),
                Some((bi, _)) if i < bi => Some((i, action)),
                Some(other) => Some(other),
            };
        }

        best.map(|(_, action)| action)
    }

    pub(crate) fn primary_key_label(&self, action: Action) -> Option<&str> {
        self.primary_labels.get(&action).map(String::as_str)
    }

    pub(crate) fn action_is_overridden(&self, action: Action) -> bool {
        self.overridden_actions.contains(&action)
    }
}

fn binding_covers(
    existing_code: KeyCode,
    existing_modifiers: KeyModifiers,
    candidate_code: KeyCode,
    candidate_modifiers: KeyModifiers,
) -> bool {
    let code_matches = match (existing_code, candidate_code) {
        (KeyCode::Char(existing), KeyCode::Char(candidate)) => existing.eq_ignore_ascii_case(&candidate),
        _ => existing_code == candidate_code,
    };
    if !code_matches || !candidate_modifiers.contains(existing_modifiers) {
        return false;
    }

    let char_shift_grace = if matches!(candidate_code, KeyCode::Char(_)) {
        KeyModifiers::SHIFT
    } else {
        KeyModifiers::empty()
    };
    candidate_modifiers
        .difference(existing_modifiers)
        .intersection(!char_shift_grace)
        == KeyModifiers::empty()
}

pub(crate) fn normalize_terminal_control_event(mut key: KeyEvent) -> KeyEvent {
    let (code, modifiers) = normalize_terminal_control_key(key.code, key.modifiers);
    key.code = code;
    key.modifiers = modifiers;
    key
}

/// Window for consecutive double-Escape detection.
///
/// First Esc arms the composer timer; a second Esc within this window clears
/// the current line (multiline) or the entire input (single-line) when content
/// is present, or opens the rewind picker (`/rewind`) on an empty composer.
/// Matches the double-Ctrl+C exit window scale (1s) but slightly tighter.
pub(crate) const DOUBLE_ESCAPE_WINDOW: Duration = Duration::from_millis(800);

/// Whether `now` is a consecutive second Escape press.
///
/// Shared by the core and app session key handlers so the two entry points
/// cannot drift on the window or comparison.
pub(crate) fn is_double_escape_press(last_press: Option<Instant>, now: Instant) -> bool {
    last_press.is_some_and(|last| now.duration_since(last) <= DOUBLE_ESCAPE_WINDOW)
}

/// Return whether a key belongs to the composer shortcuts that intentionally
/// remain outside the configurable action dispatch.
pub(crate) fn is_readline_editing_key(key: &KeyEvent) -> bool {
    let modifiers = key.modifiers;
    let has_control = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);
    let has_command = modifiers.intersects(KeyModifiers::SUPER | KeyModifiers::META);

    if has_command {
        return false;
    }

    (has_control
        && !has_alt
        && matches!(
            key.code,
            KeyCode::Char('f')
                | KeyCode::Char('F')
                | KeyCode::Char('p')
                | KeyCode::Char('P')
                | KeyCode::Char('n')
                | KeyCode::Char('N')
                | KeyCode::Char('t')
                | KeyCode::Char('T')
        ))
        || (has_alt
            && !has_control
            && matches!(
                key.code,
                KeyCode::Char('d')
                    | KeyCode::Char('D')
                    | KeyCode::Char('u')
                    | KeyCode::Char('U')
                    | KeyCode::Char('l')
                    | KeyCode::Char('L')
                    | KeyCode::Char('c')
                    | KeyCode::Char('C')
                    | KeyCode::Char('\\')
            ))
}

fn normalize_terminal_control_key(code: KeyCode, modifiers: KeyModifiers) -> (KeyCode, KeyModifiers) {
    let Some(control) = (match code {
        KeyCode::Char(character) => Some(character as u32),
        _ => None,
    }) else {
        return (code, modifiers);
    };

    // Preserve the terminal's dedicated Tab and Enter paths. In particular,
    // treating C0 tab/return bytes as Ctrl+I/Ctrl+M would bypass multiline
    // input and submit handling on terminals that encode those keys directly.
    if !(1..=26).contains(&control) || matches!(control, 9 | 13) {
        return (code, modifiers);
    }

    let Some(letter) = char::from_u32(u32::from(b'a') + control - 1) else {
        return (code, modifiers);
    };
    (KeyCode::Char(letter), modifiers | KeyModifiers::CONTROL)
}

fn format_key_label(code: KeyCode, modifiers: KeyModifiers) -> String {
    let mut parts = Vec::with_capacity(5);
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("Ctrl");
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("Alt");
    }
    if modifiers.contains(KeyModifiers::SUPER) {
        parts.push("Cmd");
    } else if modifiers.contains(KeyModifiers::META) {
        parts.push("Meta");
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("Shift");
    }

    let key = match code {
        KeyCode::Char(' ') => "Space".to_string(),
        KeyCode::Char(character) => character.to_ascii_uppercase().to_string(),
        KeyCode::Enter => "Enter".to_string(),
        KeyCode::Tab => "Tab".to_string(),
        KeyCode::BackTab => "Backtab".to_string(),
        KeyCode::Esc => "Esc".to_string(),
        KeyCode::Backspace => "Backspace".to_string(),
        KeyCode::Delete => "Delete".to_string(),
        KeyCode::Insert => "Insert".to_string(),
        KeyCode::Up => "Up".to_string(),
        KeyCode::Down => "Down".to_string(),
        KeyCode::Left => "Left".to_string(),
        KeyCode::Right => "Right".to_string(),
        KeyCode::PageUp => "PageUp".to_string(),
        KeyCode::PageDown => "PageDown".to_string(),
        KeyCode::Home => "Home".to_string(),
        KeyCode::End => "End".to_string(),
        KeyCode::F(number) => format!("F{number}"),
        other => format!("{other:?}"),
    };
    parts.push(key.as_str());
    parts.join("+")
}

impl super::Session {
    pub(crate) fn resolve_rebindable_action(&self, key: &KeyEvent) -> Option<Action> {
        self.bindings.resolve(key)
    }

    pub(crate) fn dispatch_rebindable_action(&mut self, action: Action) -> Option<super::super::types::InlineEvent> {
        super::events::dispatch_rebindable_action(self, action)
    }

    pub(crate) fn primary_binding_label(&self, action: Action) -> Option<&str> {
        self.bindings.primary_key_label(action)
    }

    pub(crate) fn rebindable_action_is_overridden(&self, action: Action) -> bool {
        self.bindings.action_is_overridden(action)
    }

    pub(crate) fn set_bindings(&mut self, bindings: BindingStore) {
        self.bindings = bindings;
        self.mark_visual_dirty();
    }

    pub(crate) fn transcript_review_hints_visible(&self) -> bool {
        self.appearance.show_transcript_review_hints
    }

    pub(crate) fn transcript_review_shortcut_guide_visible(&self) -> bool {
        self.appearance.show_transcript_review_shortcut_guide
    }

    pub(crate) fn transcript_review_close_button_visible(&self) -> bool {
        self.appearance.show_transcript_review_close_button
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::crossterm::event::KeyEvent;

    #[test]
    fn test_parse_key_binding_simple() {
        let (code, mods) = parse_key_binding("ctrl+c").unwrap();
        assert_eq!(code, KeyCode::Char('c'));
        assert!(mods.contains(KeyModifiers::CONTROL));
        assert!(!mods.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn test_parse_key_binding_modifier_combos() {
        let (code, mods) = parse_key_binding("ctrl+shift+enter").unwrap();
        assert_eq!(code, KeyCode::Enter);
        assert!(mods.contains(KeyModifiers::CONTROL));
        assert!(mods.contains(KeyModifiers::SHIFT));
    }

    #[test]
    fn test_parse_key_binding_is_case_insensitive() {
        let (code, mods) = parse_key_binding("CTRL+SHIFT+X").unwrap();
        assert_eq!(code, KeyCode::Char('x'));
        assert!(mods.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT));
    }

    #[test]
    fn test_parse_key_binding_func_keys() {
        let (code, _) = parse_key_binding("f5").unwrap();
        assert_eq!(code, KeyCode::F(5));
    }

    #[test]
    fn test_parse_key_binding_special() {
        let (code, _) = parse_key_binding("pageup").unwrap();
        assert_eq!(code, KeyCode::PageUp);
        let (code, _) = parse_key_binding("backtab").unwrap();
        assert_eq!(code, KeyCode::BackTab);
    }

    #[test]
    fn test_default_bindings_resolve() {
        let store = BindingStore::defaults();

        // Ctrl+C → Interrupt
        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(store.resolve(&key), Some(Action::Interrupt));

        // PageUp → ScrollPageUp
        let key = KeyEvent::new(KeyCode::PageUp, KeyModifiers::empty());
        assert_eq!(store.resolve(&key), Some(Action::ScrollPageUp));

        // Alt+Up → EditQueue
        let key = KeyEvent::new(KeyCode::Up, KeyModifiers::ALT);
        assert_eq!(store.resolve(&key), Some(Action::EditQueue));

        // Alt+I → FocusInput (both cases, like the other Alt chords)
        let key = KeyEvent::new(KeyCode::Char('i'), KeyModifiers::ALT);
        assert_eq!(store.resolve(&key), Some(Action::FocusInput));
        let key = KeyEvent::new(KeyCode::Char('I'), KeyModifiers::ALT);
        assert_eq!(store.resolve(&key), Some(Action::FocusInput));

        // Unbound → None
        let key = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(store.resolve(&key), None);
    }

    #[test]
    fn test_default_bindings_case_insensitive() {
        let store = BindingStore::defaults();

        // Ctrl+C (uppercase C) → Interrupt
        let key = KeyEvent::new(KeyCode::Char('C'), KeyModifiers::CONTROL);
        assert_eq!(store.resolve(&key), Some(Action::Interrupt));
    }

    #[test]
    fn test_default_tool_display_binding_is_alt_t() {
        let store = BindingStore::defaults();
        let key = KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT);
        assert_eq!(store.resolve(&key), Some(Action::ToggleToolDisplayMode));
    }

    #[test]
    fn test_default_task_panel_binding_is_alt_g() {
        let store = BindingStore::defaults();
        let key = KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT);
        assert_eq!(store.resolve(&key), Some(Action::ToggleTaskPanel));
    }

    #[test]
    fn test_default_transcript_review_bindings() {
        let store = BindingStore::defaults();
        assert_eq!(
            store.resolve(&KeyEvent::new(KeyCode::Char('t'), KeyModifiers::CONTROL)),
            Some(Action::OpenTranscriptReview)
        );
        assert_eq!(
            store.resolve(&KeyEvent::new(KeyCode::Char('r'), KeyModifiers::empty())),
            Some(Action::ToggleTranscriptRenderMode)
        );
    }

    #[test]
    fn test_raw_control_character_matches_transcript_review_binding() {
        let store = BindingStore::defaults();

        assert_eq!(
            store.resolve(&KeyEvent::new(KeyCode::Char('\u{14}'), KeyModifiers::empty())),
            Some(Action::OpenTranscriptReview)
        );
    }

    #[test]
    fn test_transcript_review_bindings_can_be_rebound() {
        let mut overlay = HashMap::new();
        overlay.insert("open_transcript_review".to_string(), vec!["ctrl+x".to_string()]);
        overlay.insert("toggle_transcript_render_mode".to_string(), vec!["alt+x".to_string()]);
        let store = BindingStore::new(overlay);

        assert_eq!(
            store.resolve(&KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
            Some(Action::OpenTranscriptReview)
        );
        assert_eq!(
            store.resolve(&KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT)),
            Some(Action::ToggleTranscriptRenderMode)
        );
        assert_eq!(store.resolve(&KeyEvent::new(KeyCode::Char('r'), KeyModifiers::empty())), None);
    }

    #[test]
    fn test_task_panel_binding_can_be_rebound() {
        let mut overlay = HashMap::new();
        overlay.insert("toggle_task_panel".to_string(), vec!["ctrl+x".to_string()]);
        let store = BindingStore::new(overlay);

        assert_eq!(
            store.resolve(&KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
            Some(Action::ToggleTaskPanel)
        );
        assert_eq!(store.resolve(&KeyEvent::new(KeyCode::Char('g'), KeyModifiers::ALT)), None);
    }

    #[test]
    fn test_tool_display_binding_can_be_rebound() {
        let mut overlay = HashMap::new();
        overlay.insert("toggle_tool_display_mode".to_string(), vec!["ctrl+x".to_string()]);
        let store = BindingStore::new(overlay);

        assert_eq!(
            store.resolve(&KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)),
            Some(Action::ToggleToolDisplayMode)
        );
        assert_eq!(store.resolve(&KeyEvent::new(KeyCode::Char('t'), KeyModifiers::ALT)), None);
    }

    #[test]
    fn test_user_overlay_overrides_default() {
        let mut overlay = HashMap::new();
        overlay.insert("interrupt".to_string(), vec!["ctrl+x".to_string()]);
        let store = BindingStore::new(overlay);

        // Old binding no longer works
        let key_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(store.resolve(&key_c), None);

        // New binding works
        let key_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert_eq!(store.resolve(&key_x), Some(Action::Interrupt));
    }

    #[test]
    fn test_user_overlay_unbind() {
        let mut overlay = HashMap::new();
        overlay.insert("interrupt".to_string(), Vec::new());
        let store = BindingStore::new(overlay);

        let key = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert_eq!(store.resolve(&key), None);
    }

    #[test]
    fn test_invalid_overlay_keeps_defaults_and_does_not_override_action() {
        let mut overlay = HashMap::new();
        overlay.insert("interrupt".to_string(), vec!["not-a-key".to_string()]);
        let store = BindingStore::new(overlay);

        assert_eq!(store.resolve(&KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)), Some(Action::Interrupt));
        assert!(!store.action_is_overridden(Action::Interrupt));
    }

    #[test]
    fn test_configured_binding_collision_is_deterministic() {
        let mut overlay = HashMap::new();
        overlay.insert("interrupt".to_string(), vec!["ctrl+x".to_string()]);
        overlay.insert("open_model_picker".to_string(), vec!["ctrl+x".to_string()]);
        let store = BindingStore::new(overlay);

        assert_eq!(store.resolve(&KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL)), Some(Action::Interrupt));
        assert_eq!(store.primary_key_label(Action::Interrupt), Some("Ctrl+X"));
        assert_eq!(store.primary_key_label(Action::OpenModelPicker), None);
    }

    #[test]
    fn test_parse_invalid_key() {
        assert!(parse_key_binding("").is_none());
        assert!(parse_key_binding("invalid_key_name").is_none());
        assert!(parse_key_binding("ctrl+invalid").is_none());
        assert!(parse_key_binding("+ctrl+c").is_none());
    }

    #[test]
    fn test_action_name_roundtrip() {
        for action in Action::all() {
            let name = action.name();
            let parsed = Action::from_name(name);
            assert_eq!(parsed, Some(*action));
        }
    }

    #[test]
    fn test_action_from_name_unknown() {
        assert_eq!(Action::from_name("nonexistent"), None);
    }

    #[test]
    fn double_escape_press_requires_an_armed_consecutive_press() {
        let now = Instant::now();
        let exactly_at_window = now.checked_sub(DOUBLE_ESCAPE_WINDOW).expect("clock is past the window");
        let beyond_window = now
            .checked_sub(DOUBLE_ESCAPE_WINDOW + Duration::from_millis(1))
            .expect("clock is past the window");

        assert!(!is_double_escape_press(None, now));
        assert!(is_double_escape_press(Some(now), now));
        assert!(is_double_escape_press(Some(exactly_at_window), now));
        assert!(!is_double_escape_press(Some(beyond_window), now));
    }
}
