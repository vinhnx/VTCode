use hashbrown::HashMap;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Rebindable user-facing actions.
///
/// Each variant corresponds to a command-level action that a user may want to
/// remap.  Fine-grained editing shortcuts (character insertion, cursor movement,
/// text selection, Backspace, Delete, Home/End, Ctrl+A/E/W/U/K, Enter/Tab/Esc
/// with their context-sensitive logic) remain hardcoded in `events.rs`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Interrupt,
    Exit,
    BackgroundOperation,
    OpenModelPicker,
    ClearScreen,
    ScrollPageUp,
    ScrollPageDown,
    EditQueue,
    HistoryPrevious,
    HistoryNext,
    ToggleLogs,
    GeneratePromptSuggestion,
}

impl Action {
    /// Human-readable name for config file serialization.
    pub fn name(self) -> &'static str {
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
            Action::GeneratePromptSuggestion => "generate_prompt_suggestion",
        }
    }

    /// All defined actions.
    pub fn all() -> &'static [Action] {
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
            Action::GeneratePromptSuggestion,
        ]
    }

    /// Look up an action by its serialized name.
    pub fn from_name(name: &str) -> Option<Self> {
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
    let s = s.trim();
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
    m.insert(
        ScrollPageDown,
        vec![(KeyCode::PageDown, KeyModifiers::empty())],
    );

    m.insert(
        EditQueue,
        vec![
            (KeyCode::Up, KeyModifiers::ALT),
            (KeyCode::Up, KeyModifiers::META),
        ],
    );

    m.insert(HistoryPrevious, vec![(KeyCode::Up, KeyModifiers::empty())]);
    m.insert(HistoryNext, vec![(KeyCode::Down, KeyModifiers::empty())]);

    m.insert(
        ToggleLogs,
        vec![
            (KeyCode::Char('t'), KeyModifiers::CONTROL),
            (KeyCode::Char('T'), KeyModifiers::CONTROL),
        ],
    );
    m.insert(
        GeneratePromptSuggestion,
        vec![
            (KeyCode::Char('p'), KeyModifiers::ALT),
            (KeyCode::Char('P'), KeyModifiers::ALT),
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
    pub fn new(overlay: HashMap<String, Vec<String>>) -> Self {
        let mut merged: HashMap<Action, Vec<(KeyCode, KeyModifiers)>> = default_bindings();

        for (action_name, key_specs) in overlay {
            let Some(action) = Action::from_name(&action_name) else {
                tracing::debug!(%action_name, "unknown action in keybinding overlay, skipping");
                continue;
            };

            let parsed: Vec<(KeyCode, KeyModifiers)> = key_specs
                .iter()
                .filter_map(|s| parse_key_binding(s))
                .collect();

            if parsed.is_empty() {
                // Empty list → unbind (remove defaults).
                merged.remove(&action);
            } else {
                merged.insert(action, parsed);
            }
        }

        let mut entries = Vec::new();
        for (action, keys) in &merged {
            for &(code, mods) in keys {
                entries.push((code, mods, *action));
            }
        }

        Self { entries }
    }

    /// Build with only the default bindings.
    pub fn defaults() -> Self {
        Self::new(HashMap::new())
    }

    /// Look up the action bound to a given key event.
    ///
    /// Returns `None` when the key has no binding (fall through to hardcoded
    /// dispatch).
    pub fn resolve(&self, key: &KeyEvent) -> Option<Action> {
        let mut best: Option<(usize, Action)> = None;

        // Iterate entries. For `Char` codes we also try a case-insensitive
        // match to handle terminal ambiguity (e.g. Ctrl+C vs Ctrl+Shift+C).
        for (i, &(code, mods, action)) in self.entries.iter().enumerate() {
            let code_match = match (code, key.code) {
                (KeyCode::Char(bc), KeyCode::Char(kc)) if bc.eq_ignore_ascii_case(&kc) => true,
                _ => code == key.code,
            };

            if !code_match {
                continue;
            }

            // All declared modifiers must be present.
            if !key.modifiers.contains(mods) {
                continue;
            }

            // For Char codes, SHIFT is already reflected in character case,
            // so allow it as an "extra" modifier without penalty.
            let char_shift_grace = if let KeyCode::Char(_) = key.code {
                KeyModifiers::SHIFT
            } else {
                KeyModifiers::empty()
            };
            let extra = key.modifiers.difference(mods);
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
}
