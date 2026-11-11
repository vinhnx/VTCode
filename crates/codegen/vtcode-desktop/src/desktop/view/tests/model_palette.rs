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

        let labels: Vec<String> = view.read(cx).palette_groups.iter().map(|(label, _)| label.clone()).collect();
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
