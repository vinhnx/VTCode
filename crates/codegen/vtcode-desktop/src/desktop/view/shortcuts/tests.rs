//! Shortcut binding tests: id stability, conflict rules, and the
//! accessibility label for each key form.

use super::*;
use gpui_kit::TestAppContext;

#[test]
fn accessible_shortcut_labels_use_the_platform_formatted_key() {
    let keystroke = gpui_kit::Keystroke::parse("cmd-shift-p").unwrap();
    assert_eq!(shortcut_accessibility_value("cmd-shift-p"), Kbd::format(&keystroke));
    assert_eq!(shortcut_accessibility_value(""), "not set");
    assert_eq!(shortcut_accessibility_value("a-b"), "invalid shortcut");
}

#[gpui_kit::test]
fn rejects_conflicting_reserved_typing_and_unknown_shortcuts(cx: &mut TestAppContext) {
    cx.update(|cx| {
        assert_eq!(bindings(&BTreeMap::new(), cx).unwrap().len(), 38);
        assert!(bindings(&BTreeMap::from([("commands".into(), "ctrl-shift-k".into())]), cx).is_ok());
        assert!(bindings(&BTreeMap::from([("settings_search".into(), "ctrl-alt-f".into())]), cx).is_ok());
        assert!(bindings(&BTreeMap::from([("zen_mode".into(), "ctrl-alt-z".into())]), cx).is_ok());
        assert_eq!(bindings(&BTreeMap::from([("model".into(), String::new())]), cx).unwrap().len(), 37);
        assert_eq!(
            bindings(&BTreeMap::from([("settings_search".into(), String::new())]), cx)
                .unwrap()
                .len(),
            36
        );
        assert_eq!(
            bindings(&BTreeMap::from([("settings".into(), String::new())]), cx)
                .unwrap()
                .len(),
            36,
            "unbinding Settings removes both its desktop and in-settings bindings"
        );
        assert!(bindings(&BTreeMap::from([("dismiss".into(), "x".into())]), cx).is_err());
        for (action, key) in [
            ("commands", "cmd-n"),
            ("commands", "cmd-q"),
            ("commands", "k"),
            ("zen_mode", "cmd-shift-z"),
            ("unknown", "ctrl-k"),
        ] {
            assert!(bindings(&BTreeMap::from([(action.into(), key.into())]), cx).is_err(), "{action}: {key}");
        }
    });
}

#[gpui_kit::test]
fn rejects_overrides_colliding_with_fixed_chords(cx: &mut TestAppContext) {
    cx.update(|cx| {
        // Every modified fixed-table chord must be rejected: the deeper fixed
        // binding would shadow the rebind (e.g. `ctrl-m` opening the model
        // picker instead of the rebound command in the composer). CLI import
        // funnels through the same `bindings()` gate, so it is covered too.
        for key in bootstrap::FIXED_RESERVED_CHORDS {
            assert!(
                bindings(&BTreeMap::from([("model".into(), (*key).into())]), cx).is_err(),
                "{key} must be reserved"
            );
        }
        // Near-miss neighbors stay accepted (asymmetric sides).
        assert!(bindings(&BTreeMap::from([("model".into(), "ctrl-shift-m".into())]), cx).is_ok());
        assert!(bindings(&BTreeMap::from([("commands".into(), "ctrl-alt-p".into())]), cx).is_ok());
        assert!(bindings(&BTreeMap::from([("settings".into(), "cmd-shift-]".into())]), cx).is_ok());
    });
}

#[gpui_kit::test]
fn composer_context_chords_stay_with_their_commands(cx: &mut TestAppContext) {
    cx.update(|cx| {
        // The owner may keep its default or move away on its own.
        assert!(bindings(&BTreeMap::from([("cancel".into(), "cmd-.".into())]), cx).is_ok());
        assert!(bindings(&BTreeMap::from([("cancel".into(), "ctrl-alt-c".into())]), cx).is_ok());
        // Once the owner unbinds, the chord is still owned by the fixed
        // composer binding. Handing it to another command would shadow that
        // command whenever the composer has focus (deeper context wins).
        for (chord, owner) in EDITOR_CONTEXT_CHORDS {
            let overrides = BTreeMap::from([
                (owner.as_str().to_string(), String::new()),
                ("commands".to_string(), chord.to_string()),
            ]);
            assert!(bindings(&overrides, cx).is_err(), "{chord} must stay reserved for {:?}", owner);
        }
        // Near-miss neighbours stay assignable.
        assert!(bindings(&BTreeMap::from([("commands".into(), "ctrl-.".into())]), cx).is_ok());
        assert!(bindings(&BTreeMap::from([("commands".into(), "ctrl-[".into())]), cx).is_ok());
        assert!(bindings(&BTreeMap::from([("commands".into(), "ctrl-]".into())]), cx).is_ok());
    });
}

#[gpui_kit::test]
fn editor_context_owner_table_pins_defaults_and_fixed_bindings(cx: &mut TestAppContext) {
    // Structural pin: if `init_keybindings` drops or changes one of its
    // `Input && mode == editor` rows, or a command default drifts, this fails
    // instead of leaving stale reservation rules in `bindings()`.
    for (chord, owner) in EDITOR_CONTEXT_CHORDS {
        let command = COMMANDS.iter().find(|command| command.id == owner).unwrap();
        assert_eq!(command.default_binding, chord, "{:?} default drifted", owner);
    }
    cx.update(|cx| {
        init_keybindings(cx, &Default::default());
        for (chord, owner) in EDITOR_CONTEXT_CHORDS {
            let expected = action(owner);
            assert_chord_resolves_to(cx, chord, "Input mode=editor", expected.as_ref());
        }
    });
}

#[gpui_kit::test]
fn every_modified_fixed_chord_is_reserved(cx: &mut TestAppContext) {
    use gpui_kit::AsKeystroke;

    cx.update(|cx| {
        let defaults = bindings(&BTreeMap::new(), cx).expect("defaults build");
        let configurable_count = defaults.len();
        cx.bind_keys(defaults);
        let baseline = cx.key_bindings().borrow().bindings().count();
        init_keybindings(cx, &Default::default());
        // `init_keybindings` rebinds the configurable table first; everything
        // after that prefix is the fixed table. Normalize both sides so the
        // oracle is independent of the spelling chosen in either table.
        let os = host_operating_system();
        let os_reserved = [
            "cmd-q",
            "cmd-w",
            "cmd-m",
            "cmd-h",
            "alt-cmd-h",
            "cmd-shift-n",
            "ctrl-cmd-f",
            "cmd-c",
            "cmd-x",
            "cmd-v",
            "cmd-a",
            "cmd-z",
            "cmd-shift-z",
        ];
        let reserved: HashSet<String> = bootstrap::FIXED_RESERVED_CHORDS
            .iter()
            .copied()
            .chain(EDITOR_CONTEXT_CHORDS.iter().map(|(chord, _)| *chord))
            .chain(os_reserved)
            .map(|chord| gpui_kit::Keystroke::parse_with_os(chord, os).unwrap().unparse(os))
            .collect();
        let keymap = cx.key_bindings();
        let fixed: Vec<String> = keymap
            .borrow()
            .bindings()
            .skip(baseline + configurable_count)
            .flat_map(|binding| {
                binding
                    .keystrokes()
                    .iter()
                    .map(|stroke| stroke.as_keystroke().unparse(os))
                    .collect::<Vec<_>>()
            })
            .collect();
        for chord in fixed {
            if !chord.contains('-') {
                continue; // bare keys are rejected by the modifier rule
            }
            assert!(
                reserved.contains(&chord),
                "fixed chord {chord} must be reserved before a configurable rebind can claim it"
            );
        }
    });
}

#[gpui_kit::test]
fn review_navigation_accepts_configurable_function_key_bindings(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let configured = BTreeMap::from([
            ("next_review_change".into(), "f8".into()),
            ("previous_review_change".into(), "shift-f8".into()),
            ("next_review_file".into(), "alt-f9".into()),
            ("previous_review_file".into(), "alt-shift-f9".into()),
        ]);
        assert!(bindings(&configured, cx).is_ok());
        assert_eq!(context_for(CommandId::NextReviewChange), "VTCodeDiffReview");
        assert_eq!(context_for(CommandId::PreviousReviewFile), "VTCodeDiffReview");
        assert_eq!(context_for(CommandId::NavigateBack), "VTCodeDesktop");
        assert!(bindings(&BTreeMap::from([("next_review_change".into(), "f25".into())]), cx).is_err());
        assert!(
            bindings(
                &BTreeMap::from([
                    ("next_review_change".into(), "f8".into()),
                    ("previous_review_change".into(), "f8".into()),
                ]),
                cx
            )
            .is_err()
        );
    });
}

#[test]
fn command_ids_round_trip_and_pin_the_persisted_table() {
    // Ids persist in preferences: this literal table pins the contract so
    // a rename breaks loudly instead of orphaning saved shortcuts.
    let pinned = [
        "commands",
        "conversation_search",
        "new_session",
        "close_session",
        "workspace",
        "composer",
        "sidebar",
        "inspector",
        "zen_mode",
        "cancel",
        "background_operation",
        "dismiss",
        "settings",
        "settings_search",
        "model",
        "permissions",
        "agent_mode",
        "add_context",
        "status_bar",
        "reload_cli_settings",
        "sync_cli_theme",
        "utilities",
        "workspace_files",
        "workspace_review",
        "refresh_workspace",
        "navigate_back",
        "navigate_forward",
        "next_review_change",
        "previous_review_change",
        "next_review_file",
        "previous_review_file",
        "queue_batched",
        "prompt_history",
        "edit_queue",
        "browse_threads",
        "go_home",
    ];
    assert_eq!(CommandId::ALL.map(CommandId::as_str), pinned);
    assert_eq!(COMMANDS.map(|command| command.id), CommandId::ALL);
    for id in CommandId::ALL {
        assert_eq!(CommandId::from_id(id.as_str()), Some(id));
    }
    assert_eq!(CommandId::from_id("unknown"), None);
    assert_eq!(CommandId::from_id(""), None);
    assert_eq!(CommandId::from_id("Commands"), None);
}

#[test]
fn commands_carry_unique_labels_descriptions_and_parsable_defaults() {
    use std::collections::HashSet;

    let mut labels = HashSet::new();
    for command in COMMANDS {
        let label = command.label_in("en");
        let description = command.description_in("en");
        assert!(!label.is_empty(), "{:?} needs a label", command.id);
        assert!(!description.is_empty(), "{:?} needs a description", command.id);
        assert!(labels.insert(label.clone()), "duplicate label: {label}");
        assert!(
            gpui_kit::Keystroke::parse(command.default_binding).is_ok(),
            "{:?} default must parse: {}",
            command.id,
            command.default_binding
        );
    }
}

#[test]
fn draft_from_fills_defaults_and_keeps_overrides() {
    let draft = draft_from(&BTreeMap::new());
    assert_eq!(draft.len(), COMMANDS.len());
    assert_eq!(draft.get("commands").map(String::as_str), Some("cmd-k"));
    assert_eq!(draft.get("previous_review_file").map(String::as_str), Some("alt-shift-f7"));

    let draft = draft_from(&BTreeMap::from([
        ("commands".into(), "ctrl-shift-k".into()),
        ("model".into(), String::new()),
    ]));
    assert_eq!(draft.len(), COMMANDS.len());
    assert_eq!(draft.get("commands").map(String::as_str), Some("ctrl-shift-k"));
    assert_eq!(draft.get("model").map(String::as_str), Some(""));
    assert_eq!(draft.get("zen_mode").map(String::as_str), Some("ctrl-cmd-z"));

    let zen = COMMANDS.iter().find(|command| command.id == CommandId::ZenMode).unwrap();
    assert_eq!(staged_value(&BTreeMap::new(), zen), "ctrl-cmd-z");
    let overrides = BTreeMap::from([("zen_mode".into(), "ctrl-alt-z".into())]);
    assert_eq!(staged_value(&overrides, zen), "ctrl-alt-z");
}

#[test]
fn shortcut_search_matches_label_description_and_binding() {
    use super::super::config_search::config_search_terms;

    let zen = COMMANDS.iter().find(|command| command.id == CommandId::ZenMode).unwrap();
    let commands = COMMANDS.iter().find(|command| command.id == CommandId::Commands).unwrap();
    let model = COMMANDS.iter().find(|command| command.id == CommandId::Model).unwrap();

    assert!(shortcut_matches_query(zen, "ctrl-cmd-z", &[]));
    // Label-only term: "zen" hits Zen mode, not the palette command.
    let terms = config_search_terms("zen");
    assert!(shortcut_matches_query(zen, "ctrl-cmd-z", &terms));
    assert!(!shortcut_matches_query(commands, "cmd-k", &terms));
    // Description-only term: "palette" hits Search commands, not Choose model.
    let terms = config_search_terms("palette");
    assert!(shortcut_matches_query(commands, "cmd-k", &terms));
    assert!(!shortcut_matches_query(model, "cmd-shift-m", &terms));
    // Every term must match: "toggle zen" hits Zen mode, "toggle palette" hits nothing.
    let terms = config_search_terms("toggle zen");
    assert!(shortcut_matches_query(zen, "ctrl-cmd-z", &terms));
    assert!(!shortcut_matches_query(commands, "cmd-k", &terms));
    let terms = config_search_terms("toggle palette");
    assert!(!shortcut_matches_query(zen, "ctrl-cmd-z", &terms));
    assert!(!shortcut_matches_query(commands, "cmd-k", &terms));
    // Case-insensitive, and staged bindings match by their stored text.
    let terms = config_search_terms("ZEN");
    assert!(shortcut_matches_query(zen, "ctrl-cmd-z", &terms));
    let terms = config_search_terms("ctrl-shift-k");
    assert!(shortcut_matches_query(commands, "ctrl-shift-k", &terms));
    assert!(!shortcut_matches_query(commands, "cmd-k", &terms));
}

#[test]
fn capture_ignores_bare_modifiers_but_keeps_real_keys() {
    for key in [
        "shift", "control", "ctrl", "alt", "option", "cmd", "command", "meta", "function", "fn", "platform",
    ] {
        assert!(is_modifier_key(key), "{key} must not commit a capture");
    }
    for key in ["a", ";", "escape", "enter", "space", "f7", "f12", "[", ",", "."] {
        assert!(!is_modifier_key(key), "{key} must commit a capture attempt");
    }
}

#[test]
fn captured_chords_round_trip_through_parse() {
    let os = host_operating_system();
    for chord in ["cmd-shift-k", "ctrl-cmd-z", "escape", "f7", "alt-shift-f7", "cmd-["] {
        let once = gpui_kit::Keystroke::parse_with_os(chord, os).unwrap().unparse(os);
        let twice = gpui_kit::Keystroke::parse_with_os(&once, os).unwrap().unparse(os);
        assert_eq!(twice, once, "{chord} must survive a capture round-trip");
    }
}

#[gpui_kit::test]
fn secondary_alias_resolves_to_the_host_modifier(cx: &mut TestAppContext) {
    cx.update(|cx| {
        // `y` is arbitrary but load-bearing: the letter must avoid every
        // reserved chord on both hosts (`s` collides with the fixed
        // Cmd+S preview save, `j` with the fixed Ctrl+J composer chord).
        let loaded = bindings(&BTreeMap::from([("model".into(), "secondary-y".into())]), cx)
            .expect("secondary- resolves on the host OS");
        assert_eq!(loaded.len(), 38, "full table builds with the secondary override");
        #[cfg(target_os = "macos")]
        let expected = "cmd-y";
        #[cfg(not(target_os = "macos"))]
        let expected = "ctrl-y";
        assert_eq!(
            gpui_kit::Keystroke::parse_with_os("secondary-y", host_operating_system())
                .unwrap()
                .unparse(host_operating_system()),
            expected
        );
    });
}

use super::super::tests::{assert_chord_dead, assert_chord_resolves_once_to, assert_chord_resolves_to};

/// Mirror a save reinstall through the live keymap: old table, shadows,
/// then the fresh table, exactly as `bootstrap::reinstall_shortcuts` binds.
fn reinstall(old: &BTreeMap<String, String>, new: &BTreeMap<String, String>, cx: &mut App) {
    cx.bind_keys(bindings(old, cx).expect("old table builds"));
    cx.bind_keys(stale_shadows(old, new, cx));
    cx.bind_keys(bindings(new, cx).expect("new table builds"));
}

#[gpui_kit::test]
fn reinstall_retires_rebound_chord_and_keeps_new_live(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let old = BTreeMap::from([("model".into(), "ctrl-shift-m".into())]);
        let new = BTreeMap::from([("model".into(), "ctrl-alt-m".into())]);
        reinstall(&old, &new, cx);
        assert_chord_dead(cx, "ctrl-shift-m", key_context::DESKTOP);
        assert_chord_resolves_once_to(cx, "ctrl-alt-m", key_context::DESKTOP, &OpenModelPicker);
        assert_chord_resolves_to(cx, "cmd-k", key_context::DESKTOP, &OpenCommandPalette);
    });
}

#[gpui_kit::test]
fn reinstall_retires_unbound_chord_without_touching_neighbors(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let old = BTreeMap::from([("model".into(), "ctrl-shift-m".into())]);
        let new = BTreeMap::from([("model".into(), String::new())]);
        reinstall(&old, &new, cx);
        assert_chord_dead(cx, "ctrl-shift-m", key_context::DESKTOP);
        assert_chord_resolves_to(cx, "cmd-k", key_context::DESKTOP, &OpenCommandPalette);
        assert_chord_resolves_to(cx, "cmd-,", key_context::DESKTOP, &OpenSettings);
    });
}

#[gpui_kit::test]
fn unchanged_tables_emit_no_shadows(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let sparse = BTreeMap::from([("model".into(), "ctrl-shift-m".into())]);
        assert!(stale_shadows(&sparse, &sparse, cx).is_empty());
        // The staged draft is a full table while disk holds sparse
        // overrides: identical effective chords still mean no shadows.
        let full = draft_from(&sparse);
        assert!(stale_shadows(&sparse, &full, cx).is_empty());
        assert!(stale_shadows(&BTreeMap::new(), &BTreeMap::new(), cx).is_empty());
    });
}

#[gpui_kit::test]
fn invalid_old_table_retires_against_startup_defaults(cx: &mut TestAppContext) {
    cx.update(|cx| {
        // Startup rejects the garbage table and binds defaults instead,
        // so the reinstall must retire the default chord, not the garbage.
        let old = BTreeMap::from([("model".into(), "a-b".into())]);
        let new = BTreeMap::from([("model".into(), "ctrl-shift-m".into())]);
        assert!(bindings(&old, cx).is_err(), "fixture must be invalid");
        cx.bind_keys(bindings(&BTreeMap::new(), cx).expect("defaults build"));
        cx.bind_keys(stale_shadows(&old, &new, cx));
        cx.bind_keys(bindings(&new, cx).expect("new table builds"));
        assert_chord_dead(cx, "cmd-shift-m", key_context::DESKTOP);
        assert_chord_resolves_once_to(cx, "ctrl-shift-m", key_context::DESKTOP, &OpenModelPicker);
    });
}

#[gpui_kit::test]
fn settings_rebind_shadows_both_contexts(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let old = BTreeMap::new();
        let new = BTreeMap::from([("settings".into(), "ctrl-,".into())]);
        let shadows = stale_shadows(&old, &new, cx);
        assert_eq!(shadows.len(), 2, "the dual-context binding retires twice");
        reinstall(&old, &new, cx);
        for context in [key_context::DESKTOP, key_context::SETTINGS] {
            assert_chord_dead(cx, "cmd-,", context);
            assert_chord_resolves_once_to(cx, "ctrl-,", context, &OpenSettings);
        }
        // The untouched twin keeps its own chord live in both contexts.
        for context in [key_context::DESKTOP, key_context::SETTINGS] {
            assert_chord_resolves_to(cx, "cmd-f", context, &OpenSettingsSearch);
        }
    });
}

#[gpui_kit::test]
fn retiring_the_full_table_shadows_every_binding(cx: &mut TestAppContext) {
    cx.update(|cx| {
        // Structural pin: retiring everything must shadow every binding,
        // including dual-context commands. If `bindings()` gains a
        // context, this fails until `stale_shadows` mirrors it.
        let unbind_all: BTreeMap<String, String> = COMMANDS
            .iter()
            .map(|command| (command.id.as_str().to_string(), String::new()))
            .collect();
        let defaults = bindings(&BTreeMap::new(), cx).expect("defaults build");
        let shadows = stale_shadows(&BTreeMap::new(), &unbind_all, cx);
        assert_eq!(shadows.len(), defaults.len(), "one shadow per bound chord");
        cx.bind_keys(defaults);
        cx.bind_keys(shadows);
        for command in COMMANDS.iter() {
            assert_chord_dead(cx, command.default_binding, context_for(command.id));
        }
    });
}

#[gpui_kit::test]
fn navigation_chords_reach_browse_threads_and_home(cx: &mut TestAppContext) {
    cx.update(|cx| {
        reinstall(&BTreeMap::new(), &BTreeMap::new(), cx);
        assert_chord_resolves_to(cx, "cmd-t", key_context::DESKTOP, &OpenThreadBrowser);
        assert_chord_resolves_to(cx, "cmd-shift-h", key_context::DESKTOP, &GoHome);
    });
}
