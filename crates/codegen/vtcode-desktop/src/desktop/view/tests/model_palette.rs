use super::*;

fn catalog_model(provider: &str, id: &str) -> vtcode_desktop_protocol::ModelInfo {
    vtcode_desktop_protocol::ModelInfo {
        provider: provider.to_string(),
        id: id.to_string(),
        display_name: format!("{id} display"),
        context_window: None,
        current: false,
        recommended_rank: None,
        capabilities: None,
    }
}

#[gpui_kit::test]
fn model_palette_groups_show_labels_in_picker_order(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            // The active route sits on an open-source/local provider; the
            // frontier labs still lead the list and the active group is only
            // highlighted in place.
            config.provider = "ollama".to_string();
            config.model = "local-x".to_string();
            view.configuration = Some(config);
            // Scrambled arrival order: alphabetical-by-code would sort
            // anthropic < huggingface < mycorp < ollama < openai.
            view.models = vec![
                catalog_model("huggingface", "hf-a"),
                catalog_model("openai", "gpt-x"),
                catalog_model("mycorp", "corp-x"),
                catalog_model("ollama", "local-x"),
                catalog_model("anthropic", "claude-x"),
            ];
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_model_palette(window, cx));

        let labels: Vec<String> = view
            .read(cx)
            .palette_groups
            .iter()
            .map(|(label, _)| label.to_string())
            .collect();
        assert_eq!(labels, ["OpenAI", "Anthropic", "Hugging Face", "Ollama", "mycorp"]);

        let (description, keywords) = view
            .read(cx)
            .palette_groups
            .iter()
            .find(|(label, _)| label == "OpenAI")
            .and_then(|(_, entries)| entries.first())
            .map(|entry| (entry.description.clone().unwrap_or_default(), entry.keywords.clone()))
            .expect("openai group entry");
        assert!(description.starts_with("OpenAI"), "description leads with label: {description}");
        assert!(description.contains("gpt-x"), "description keeps model id: {description}");
        assert!(keywords.iter().any(|keyword| keyword == "openai"), "code stays searchable");
        assert!(keywords.iter().any(|keyword| keyword == "OpenAI"), "label is searchable");

        let current_suffix = rust_i18n::t!("app.palette.current_suffix").trim().to_string();
        let current_marked = view
            .read(cx)
            .palette_groups
            .iter()
            .flat_map(|(_, entries)| entries)
            .find(|entry| {
                matches!(
                    &entry.action,
                    super::super::PaletteAction::SelectModel { provider, model }
                        if provider.as_str() == "ollama" && model.as_str() == "local-x"
                )
            })
            .and_then(|entry| entry.description.as_deref())
            .is_some_and(|description| description.contains(&current_suffix));
        assert!(current_marked, "the active route is still marked in its ranked group");
    })
    .unwrap();
}

#[gpui_kit::test]
fn model_palette_merges_provider_alias_into_canonical_group(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            // Alias spelling (`google`) with an uncatalogued model triggers
            // the active-model fallback next to the canonical catalog group.
            config.provider = "google".to_string();
            config.model = "custom-xyz".to_string();
            view.configuration = Some(config);
            view.models = vec![catalog_model("gemini", "gemini-3.8-flash")];
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_model_palette(window, cx));

        let groups = &view.read(cx).palette_groups;
        assert_eq!(groups.len(), 1, "alias merges into one group");
        assert_eq!(groups[0].0, "Gemini");
        let mut providers: Vec<String> = groups[0]
            .1
            .iter()
            .filter_map(|entry| match &entry.action {
                super::super::PaletteAction::SelectModel { provider, .. } => Some(provider.clone()),
                _ => None,
            })
            .collect();
        providers.sort();
        assert_eq!(providers, ["gemini", "google"], "entries keep their original wire codes");
    })
    .unwrap();
}

#[gpui_kit::test]
fn model_palette_route_availability_resolves_provider_alias(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.models = vec![catalog_model("gemini", "gemini-3.8-flash")];
        });
        window.render_frame(cx);

        let route = |provider: &str, model: &str| super::super::PaletteAction::SelectModel {
            provider: provider.into(),
            model: model.into(),
        };
        // Deeplink-style codes must resolve through the catalog alias and
        // model-id casing instead of presenting a disabled row.
        assert!(view.read(cx).palette_action_available(&route("google", "GEMINI-3.8-FLASH"), cx));
        assert!(
            !view.read(cx).palette_action_available(&route("google", "gemini-9-none"), cx),
            "an uncatalogued route stays unavailable"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn model_palette_marks_catalog_route_current_through_provider_alias(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            // The active route uses the `google` alias and different model-id
            // casing from the catalog, so identity must resolve canonically.
            config.provider = "google".to_string();
            config.model = "GEMINI-3.8-FLASH".to_string();
            view.configuration = Some(config);
            view.models = vec![catalog_model("gemini", "gemini-3.8-flash")];
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_model_palette(window, cx));

        let groups = &view.read(cx).palette_groups;
        assert_eq!(groups.len(), 1, "alias merges into one group");
        assert_eq!(groups[0].0, "Gemini");
        assert_eq!(groups[0].1.len(), 1, "the catalog route is not duplicated by the fallback");

        let current_suffix = rust_i18n::t!("app.palette.current_suffix").trim().to_string();
        let description = groups[0].1[0].description.as_deref().unwrap_or_default();
        assert!(
            description.contains(&current_suffix),
            "catalog route is marked current through the alias: {description}"
        );
        assert_eq!(
            view.read(cx).model_display_name("google", "gemini-3.8-flash"),
            Some("gemini-3.8-flash display"),
            "display name resolves through the alias"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn model_palette_preserves_catalog_generation_order_within_group(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = None;
            // The catalog arrives newest-first within a provider; grouping
            // must keep that arrival order (no alphabetical re-sort).
            view.models = vec![
                catalog_model("openai", "gpt-6.2-astra"),
                catalog_model("openai", "gpt-6.1-sol"),
                catalog_model("openai", "gpt-6-sol"),
            ];
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_model_palette(window, cx));

        let openai = view
            .read(cx)
            .palette_groups
            .iter()
            .find(|(label, _)| label == "OpenAI")
            .expect("openai group");
        let order: Vec<&str> = openai
            .1
            .iter()
            .filter_map(|entry| match &entry.action {
                super::super::PaletteAction::SelectModel { model, .. } => Some(model.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(order, ["gpt-6.2-astra", "gpt-6.1-sol", "gpt-6-sol"]);
    })
    .unwrap();
}

/// Opening the full "Choose a model" panel must leave the active route
/// selected and scrolled into view. With a long catalog the current row can
/// sit well below the fold, so a bare list with no selection leaves the user
/// guessing which model is active (the original bug).
#[gpui_kit::test]
fn opening_the_full_model_palette_selects_and_reveals_the_current_model(cx: &mut TestAppContext) {
    let (window, view) = open_desktop_sized(cx, 520., 700.);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    settle_fade_animations(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            // Frontier labs lead the picker; the active route lives in a
            // later group, past the first screenful of rows.
            config.provider = "mycorp".into();
            config.model = "corp-current".into();
            view.configuration = Some(config);
            view.models = (0..40)
                .map(|rank| catalog_model("openai", &format!("gpt-catalog-{rank}")))
                .chain(std::iter::once(catalog_model("mycorp", "corp-current")))
                .collect();
            view.sidebar_open = false;
            view.inspector_open = false;
        });
        window.render_frame(cx);
        view.update(cx, |view, cx| view.open_model_palette(window, cx));
        // Headless windows never run `on_next_frame`. GPUI runs those callbacks
        // before the frame's draw, so call the focus hook before the first
        // render to reproduce the production ordering (the CommandState has
        // no model yet at that point).
        view.update(cx, |view, cx| view.focus_palette_state(window, cx));
        window.render_frame(cx);
        window.render_frame(cx);

        let current = super::super::PaletteAction::SelectModel {
            provider: "mycorp".into(),
            model: "corp-current".into(),
        };
        let expected = view
            .read(cx)
            .palette_groups
            .iter()
            .enumerate()
            .find_map(|(section, (_, entries))| {
                entries
                    .iter()
                    .position(|entry| entry.action == current)
                    .map(|row| gpui_kit::component::IndexPath::new(row).section(section))
            });
        let state = view.read(cx).palette.clone().expect("model palette is open");
        assert_eq!(
            state.read_with(cx, |state, _| state.selected_index()),
            expected,
            "the active model must be the highlighted row"
        );
        assert!(
            window.try_find(palette_action_id(&current)).is_some(),
            "the active model row must be scrolled into view"
        );
    })
    .unwrap();
}

/// Clicking a composer CTA while its own popup is up must dismiss that popup
/// instead of opening it again. The palette's click-away handler runs first
/// (capture phase), so it has to leave presses inside the owning trigger's
/// recorded bounds to the trigger's toggle action.
#[gpui_kit::test]
fn clicking_an_open_composer_cta_dismisses_its_panel(cx: &mut TestAppContext) {
    use super::super::PaletteKind;

    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(4);
    settle_fade_animations(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            view.configuration = Some(runtime_options_config());
            view.sidebar_open = false;
            view.inspector_open = false;
        });
        window.render_frame(cx);

        for (trigger, kind) in [
            ("pick-model", PaletteKind::Models),
            ("composer-effort", PaletteKind::ModelOptions),
            ("composer-policy", PaletteKind::Permissions),
        ] {
            window.click(trigger, cx);
            window.render_frame(cx);
            assert!(view.read(cx).palette.is_some(), "{trigger} must open its popup");
            assert!(view.read(cx).palette_open_for(&[kind]), "{trigger} must own the open popup");

            window.click(trigger, cx);
            window.render_frame(cx);
            assert!(
                view.read(cx).palette.is_none(),
                "{trigger} must dismiss its popup on a second click, not reopen it"
            );
        }

        // The attach picker is a separate popover with its own click-away
        // handler; its trigger must toggle it the same way.
        view.update(cx, |view, _| {
            view.workspace = Some(PathBuf::from("/workspace"));
        });
        window.render_frame(cx);
        window.click("attach-files", cx);
        window.render_frame(cx);
        assert!(view.read(cx).attachment_picker.is_some(), "attach opens the picker");
        window.click("attach-files", cx);
        window.render_frame(cx);
        assert!(
            view.read(cx).attachment_picker.is_none(),
            "a second attach click must dismiss the picker, not reopen it"
        );
    })
    .unwrap();
}
