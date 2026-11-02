//! Native preference projection over the CLI's authoritative key parser.
use hashbrown::HashMap;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use vtcode_desktop_protocol::SharedUiPreferences;
use vtcode_ui::tui::core_tui::session::parse_key_binding;

pub(super) fn snapshot(bindings: &HashMap<String, Vec<String>>, theme: &str) -> SharedUiPreferences {
    let mut result = SharedUiPreferences {
        // Report the effective theme the CLI would activate (unknown config
        // values fall back to the default), so desktop follow/import never
        // receives an id outside the shared registry.
        theme: vtcode_ui::theme::resolve_theme(Some(theme.to_string())),
        shortcuts: Default::default(),
        shortcut_warnings: Vec::new(),
    };
    for (cli_action, desktop_action) in [
        ("interrupt", "cancel"),
        ("open_model_picker", "model"),
        ("edit_queue", "edit_queue"),
    ] {
        let Some(specs) = bindings.get(cli_action) else {
            continue;
        };
        let mut normalized = Vec::new();
        for spec in specs.iter().take(8) {
            match native_key(spec) {
                Some(key) if !normalized.contains(&key) => normalized.push(key),
                Some(_) => {}
                None => result
                    .shortcut_warnings
                    .push(format!("{cli_action}: unsupported key combination")),
            }
        }
        if specs.len() > 8 {
            result
                .shortcut_warnings
                .push(format!("{cli_action}: only the first eight bindings were inspected"));
        }
        // Preserve an explicit empty list, but never turn invalid input into an unbind.
        if specs.is_empty() || !normalized.is_empty() {
            let _previous = result.shortcuts.insert(desktop_action.into(), normalized);
        }
    }
    result
}

fn native_key(spec: &str) -> Option<String> {
    let (code, modifiers) = parse_key_binding(spec)?;
    if modifiers.contains(KeyModifiers::HYPER) {
        return None;
    }
    let key = match code {
        KeyCode::Char(' ') => "space".into(),
        KeyCode::Char(character) if !character.is_control() => character.to_string(),
        KeyCode::Esc => "escape".into(),
        KeyCode::Enter => "enter".into(),
        KeyCode::Tab => "tab".into(),
        KeyCode::Backspace => "backspace".into(),
        KeyCode::Delete => "delete".into(),
        KeyCode::Up => "up".into(),
        KeyCode::Down => "down".into(),
        KeyCode::Left => "left".into(),
        KeyCode::Right => "right".into(),
        KeyCode::Home => "home".into(),
        KeyCode::End => "end".into(),
        KeyCode::PageUp => "pageup".into(),
        KeyCode::PageDown => "pagedown".into(),
        KeyCode::F(number) => format!("f{number}"),
        _ => return None,
    };
    let mut parts = Vec::new();
    if modifiers.contains(KeyModifiers::CONTROL) {
        parts.push("ctrl".to_string());
    }
    if modifiers.contains(KeyModifiers::ALT) {
        parts.push("alt".to_string());
    }
    if modifiers.contains(KeyModifiers::SHIFT) {
        parts.push("shift".to_string());
    }
    if modifiers.intersects(KeyModifiers::SUPER | KeyModifiers::META) {
        parts.push("cmd".to_string());
    }
    parts.push(key);
    Some(parts.join("-"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn snapshot_reports_the_effective_cli_theme() {
        let bindings = HashMap::new();
        assert_eq!(snapshot(&bindings, "ciapre-blue").theme, "ciapre-blue");
        assert_eq!(snapshot(&bindings, "Ciapre-Blue").theme, "ciapre-blue");
        assert_eq!(snapshot(&bindings, "not-a-theme").theme, vtcode_ui::theme::DEFAULT_THEME_ID);
        assert_eq!(snapshot(&bindings, "").theme, vtcode_ui::theme::DEFAULT_THEME_ID);
    }

    #[test]
    fn shared_ui_preferences_reuse_cli_aliases_and_preserve_explicit_unbinds() {
        let projected = snapshot(
            &HashMap::from([
                ("interrupt".into(), vec!["Control+SHIFT+x".into(), "ctrl+shift+x".into()]),
                ("open_model_picker".into(), Vec::new()),
                ("edit_queue".into(), vec!["alt+up".into()]),
                ("exit".into(), vec!["ctrl+d".into()]),
            ]),
            "ciapre-blue",
        );
        assert_eq!(projected.theme, "ciapre-blue");
        assert_eq!(projected.shortcuts["cancel"], vec!["ctrl-shift-x"]);
        assert!(projected.shortcuts["model"].is_empty());
        assert_eq!(projected.shortcuts["edit_queue"], vec!["alt-up"]);
        assert_eq!(projected.shortcuts.len(), 3, "terminal-only exit is not imported into native window handling");
        assert!(projected.shortcut_warnings.is_empty());
        let invalid = snapshot(&HashMap::from([("interrupt".into(), vec!["not a key".into()])]), "ciapre");
        assert!(!invalid.shortcuts.contains_key("cancel"), "invalid bindings must not disable cancellation");
        assert_eq!(invalid.shortcut_warnings.len(), 1);
    }
}
