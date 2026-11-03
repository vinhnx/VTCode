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
fn model_palette_groups_show_labels_in_picker_order_with_current_first(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    let (commands, _requests) = tokio::sync::mpsc::channel(2);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.backend = Some(BackendHandle { commands });
            let mut config = runtime_options_config();
            // The current route sits on a mid-rank provider so the test proves
            // both current-first and rank order for the rest.
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
        assert_eq!(labels, ["Ollama", "OpenAI", "Anthropic", "Hugging Face", "mycorp"]);

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
