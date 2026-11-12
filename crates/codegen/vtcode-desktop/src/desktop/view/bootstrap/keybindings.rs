//! Fixed and configurable keybindings: the startup table, live shortcut
//! reinstall, and the experimental vim layer. Split from `bootstrap.rs`
//! to hold the 1000-line ceiling.

use super::super::*;

/// Modified chords from the fixed table in [`init_keybindings`].
/// `shortcuts::bindings()` rejects overrides colliding with these: the deeper
/// fixed binding would shadow the rebind (e.g. `ctrl-m` opening the model
/// picker instead of the rebound command while the composer is focused).
/// Bare fixed chords need no entry — the modifier rule rejects them already.
/// Keep in sync with the table below.
pub(crate) const FIXED_RESERVED_CHORDS: &[&str] = &[
    // Function keys pass the configurable table's modifier rule, so the
    // tree-scoped rename chord needs an explicit entry (bare delete and
    // backspace are rejected by that rule already).
    "f2",
    "ctrl-w",
    "alt-d",
    "ctrl-u",
    "ctrl-k",
    "ctrl-f",
    "ctrl-b",
    "ctrl-j",
    "alt-enter",
    "ctrl-m",
    "ctrl-p",
    "ctrl-n",
    "ctrl-s",
    "ctrl-g",
    "cmd-s",
    "cmd-j",
];

pub(crate) fn init_keybindings(cx: &mut App, preferences: &preferences::Preferences) {
    bind_configurable_shortcuts(cx, &preferences.shortcuts);
    cx.bind_keys([
        KeyBinding::new("cmd-w", CloseWindow, Some(key_context::DESKTOP)),
        KeyBinding::new("cmd-m", MinimizeWindow, Some(key_context::DESKTOP)),
        KeyBinding::new("ctrl-cmd-f", ToggleFullScreen, Some(key_context::DESKTOP)),
        KeyBinding::new("cmd-j", ToggleTerminal, Some(key_context::DESKTOP)),
        KeyBinding::new("cmd-shift-n", ShowMainWindow, None),
        KeyBinding::new("cmd-q", QuitApplication, None),
        KeyBinding::new("cmd-h", HideApplication, None),
        KeyBinding::new("alt-cmd-h", HideOtherApplications, None),
        KeyBinding::new("right", NextInspectorTab, Some(key_context::INSPECTOR)),
        KeyBinding::new("left", PreviousInspectorTab, Some(key_context::INSPECTOR)),
        KeyBinding::new("home", FirstInspectorTab, Some(key_context::INSPECTOR)),
        KeyBinding::new("end", LastInspectorTab, Some(key_context::INSPECTOR)),
        KeyBinding::new("right", NextSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("left", PreviousSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("down", NextSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("up", PreviousSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("home", FirstSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("end", LastSettingsTab, Some(key_context::SETTINGS)),
        KeyBinding::new("enter", OpenSelectedFile, Some(gpui_kit::base::tree_key_context())),
        KeyBinding::new("f2", RenameSelectedFile, Some(gpui_kit::base::tree_key_context())),
        KeyBinding::new("delete", DeleteSelectedFile, Some(gpui_kit::base::tree_key_context())),
        KeyBinding::new("backspace", DeleteSelectedFile, Some(gpui_kit::base::tree_key_context())),
        // No fixed `cmd-[`: the configurable NavigateBack default owns that
        // chord with superset behavior (route-back, then session history),
        // so a fixed twin would be dead weight — the old twin never
        // double-fired (bubble listeners stop by default) and NavigateBack
        // already runs the route-back-first logic.
        // CLI readline parity, composer-scoped and fixed like the CLI (these
        // reuse Kit's own editing actions, so undo/redo keep working).
        KeyBinding::new("ctrl-w", DeleteToPreviousWordStart, Some(key_context::COMPOSER)),
        KeyBinding::new("alt-d", DeleteToNextWordEnd, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-u", DeleteToBeginningOfLine, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-k", DeleteToEndOfLine, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-f", MoveRight, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-b", MoveLeft, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-j", Enter { secondary: false, shift: true }, Some(key_context::COMPOSER)),
        KeyBinding::new("alt-enter", Enter { secondary: false, shift: true }, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-m", OpenModelPicker, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-p", HistoryPrevious, Some(key_context::COMPOSER)),
        KeyBinding::new("ctrl-n", HistoryNext, Some(key_context::COMPOSER)),
        // CLI scope-cycle twin: opens history when closed, cycles the search
        // scope while open. Ctrl+R keeps the palette toggle contract.
        KeyBinding::new("ctrl-s", CycleHistoryScope, Some(key_context::DESKTOP)),
        // CLI picker parity: Ctrl+G cancels the history search and restores the
        // draft (Ctrl+C stays Kit's Copy in the input context, so it cannot be
        // claimed here without also hijacking Cmd+C).
        KeyBinding::new("ctrl-g", CancelPromptHistory, Some(key_context::DESKTOP)),
        // Fixed like window chords: the app has exactly one saveable
        // surface, so Cmd+S always means "save the preview draft" and the
        // root handler no-ops unless an edit session is armed.
        KeyBinding::new("cmd-s", SavePreviewSource, Some(key_context::DESKTOP)),
        KeyBinding::new("up", TranscriptPrevious, Some(key_context::TRANSCRIPT)),
        KeyBinding::new("down", TranscriptNext, Some(key_context::TRANSCRIPT)),
        KeyBinding::new("home", TranscriptFirst, Some(key_context::TRANSCRIPT)),
        KeyBinding::new("end", TranscriptLast, Some(key_context::TRANSCRIPT)),
        KeyBinding::new("up", SessionPrevious, Some(key_context::SESSIONS)),
        KeyBinding::new("down", SessionNext, Some(key_context::SESSIONS)),
        KeyBinding::new("home", SessionFirst, Some(key_context::SESSIONS)),
        KeyBinding::new("end", SessionLast, Some(key_context::SESSIONS)),
        KeyBinding::new("enter", OpenSelectedSession, Some(key_context::SESSIONS)),
        KeyBinding::new("backspace", DeleteHighlightedSession, Some(key_context::SESSIONS)),
        KeyBinding::new("delete", DeleteHighlightedSession, Some(key_context::SESSIONS)),
        KeyBinding::new("left", PreviousThreadTab, Some(key_context::THREAD_TABS)),
        KeyBinding::new("right", NextThreadTab, Some(key_context::THREAD_TABS)),
        KeyBinding::new("home", FirstThreadTab, Some(key_context::THREAD_TABS)),
        KeyBinding::new("end", LastThreadTab, Some(key_context::THREAD_TABS)),
        KeyBinding::new("enter", OpenSelectedThreadTab, Some(key_context::THREAD_TABS)),
        KeyBinding::new("up", ActivityPrevious, Some(key_context::ACTIVITY)),
        KeyBinding::new("down", ActivityNext, Some(key_context::ACTIVITY)),
        KeyBinding::new("home", ActivityFirst, Some(key_context::ACTIVITY)),
        KeyBinding::new("end", ActivityLast, Some(key_context::ACTIVITY)),
    ]);
    if preferences.vim_navigation {
        apply_vim_navigation(cx, true);
    }
}

/// Bind the configurable shortcut table, falling back to defaults when a
/// hand-edited table fails validation. Startup binds once; shortcut saves
/// reinstall through [`reinstall_shortcuts`] so retired chords die with them.
fn bind_configurable_shortcuts(cx: &mut App, shortcuts: &std::collections::BTreeMap<String, String>) {
    match shortcuts::bindings(shortcuts, cx) {
        Ok(bindings) => cx.bind_keys(bindings),
        Err(error) => {
            eprintln!("Desktop shortcuts: {error}; using defaults");
            if let Ok(bindings) = shortcuts::bindings(&Default::default(), cx) {
                cx.bind_keys(bindings);
            }
        }
    }
}

/// Live-apply a saved shortcut table: shadow retired chords with `NoAction`,
/// then bind the fresh table. Same-chord conflicts resolve toward the latest
/// binding, so shadows must precede the table. The fixed table never changes
/// and stays bound once from [`init_keybindings`]. Reinstalling appends
/// identical duplicates for unchanged chords; dispatch stops at the first
/// hit, so they are harmless, and saves are rare user gestures.
pub(crate) fn reinstall_shortcuts(
    cx: &mut App,
    old_shortcuts: &std::collections::BTreeMap<String, String>,
    new_shortcuts: &std::collections::BTreeMap<String, String>,
) {
    cx.bind_keys(shortcuts::stale_shadows(old_shortcuts, new_shortcuts, cx));
    bind_configurable_shortcuts(cx, new_shortcuts);
}

/// (chord, key context) rows of the experimental vim layer. Vim chords reuse
/// the plain navigation actions, so toggling is pure keymap surgery: live
/// bindings while the toggle holds, `NoAction` shadows when it drops (keymaps
/// only append — see [`reinstall_shortcuts`]). Scoped to the navigation
/// containers on purpose: a bare letter bound under `DESKTOP` would hijack
/// typing in every text input, and a fixed-then-gated handler would still
/// swallow the keystroke when off (bubble listeners stop by default). Bare
/// chords need no `FIXED_RESERVED_CHORDS` entry — the configurable table's
/// modifier rule rejects them already.
const VIM_CHORDS: &[(&str, &str)] = &[
    ("j", key_context::SESSIONS),
    ("k", key_context::SESSIONS),
    ("g", key_context::SESSIONS),
    ("G", key_context::SESSIONS),
    ("i", key_context::SESSIONS),
    ("/", key_context::SESSIONS),
    ("j", key_context::TRANSCRIPT),
    ("k", key_context::TRANSCRIPT),
    ("g", key_context::TRANSCRIPT),
    ("G", key_context::TRANSCRIPT),
    ("i", key_context::TRANSCRIPT),
    ("/", key_context::TRANSCRIPT),
    ("h", key_context::INSPECTOR),
    ("l", key_context::INSPECTOR),
    ("i", key_context::INSPECTOR),
    ("/", key_context::INSPECTOR),
    ("h", key_context::SETTINGS),
    ("l", key_context::SETTINGS),
    ("i", key_context::SETTINGS),
    ("/", key_context::SETTINGS),
    ("h", key_context::THREAD_TABS),
    ("l", key_context::THREAD_TABS),
    ("i", key_context::THREAD_TABS),
    ("/", key_context::THREAD_TABS),
    ("j", key_context::ACTIVITY),
    ("k", key_context::ACTIVITY),
    ("g", key_context::ACTIVITY),
    ("shift-g", key_context::ACTIVITY),
    ("i", key_context::ACTIVITY),
    ("/", key_context::ACTIVITY),
];

/// Live out or retire the vim layer. Same-chord conflicts resolve toward the
/// latest binding, so enabling re-binds over stale shadows and disabling
/// shadows over stale live rows; toggles are rare user gestures, so the
/// appended duplicates are harmless (same tradeoff as [`reinstall_shortcuts`]).
pub(crate) fn apply_vim_navigation(cx: &mut App, enabled: bool) {
    if enabled {
        cx.bind_keys([
            KeyBinding::new("j", SessionNext, Some(key_context::SESSIONS)),
            KeyBinding::new("k", SessionPrevious, Some(key_context::SESSIONS)),
            KeyBinding::new("g", SessionFirst, Some(key_context::SESSIONS)),
            KeyBinding::new("G", SessionLast, Some(key_context::SESSIONS)),
            KeyBinding::new("i", FocusComposer, Some(key_context::SESSIONS)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::SESSIONS)),
            KeyBinding::new("j", TranscriptNext, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("k", TranscriptPrevious, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("g", TranscriptFirst, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("G", TranscriptLast, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("i", FocusComposer, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::TRANSCRIPT)),
            KeyBinding::new("h", PreviousInspectorTab, Some(key_context::INSPECTOR)),
            KeyBinding::new("l", NextInspectorTab, Some(key_context::INSPECTOR)),
            KeyBinding::new("i", FocusComposer, Some(key_context::INSPECTOR)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::INSPECTOR)),
            KeyBinding::new("h", PreviousSettingsTab, Some(key_context::SETTINGS)),
            KeyBinding::new("l", NextSettingsTab, Some(key_context::SETTINGS)),
            KeyBinding::new("i", FocusComposer, Some(key_context::SETTINGS)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::SETTINGS)),
            KeyBinding::new("h", PreviousThreadTab, Some(key_context::THREAD_TABS)),
            KeyBinding::new("l", NextThreadTab, Some(key_context::THREAD_TABS)),
            KeyBinding::new("i", FocusComposer, Some(key_context::THREAD_TABS)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::THREAD_TABS)),
            KeyBinding::new("j", ActivityNext, Some(key_context::ACTIVITY)),
            KeyBinding::new("k", ActivityPrevious, Some(key_context::ACTIVITY)),
            KeyBinding::new("g", ActivityFirst, Some(key_context::ACTIVITY)),
            KeyBinding::new("shift-g", ActivityLast, Some(key_context::ACTIVITY)),
            KeyBinding::new("i", FocusComposer, Some(key_context::ACTIVITY)),
            KeyBinding::new("/", OpenConversationSearch, Some(key_context::ACTIVITY)),
        ]);
    } else {
        cx.bind_keys(
            VIM_CHORDS
                .iter()
                .map(|(chord, context)| KeyBinding::new(chord, NoAction {}, Some(*context))),
        );
    }
}
