//! Palette search/model helpers extracted from the command catalog.

use super::*;

pub(in crate::desktop::view) fn transcript_search_snippet(text: &str) -> String {
    let line = text.lines().find(|line| !line.trim().is_empty()).unwrap_or_default().trim();
    let mut chars = line.chars();
    let snippet = chars.by_ref().take(96).collect::<String>();
    if chars.next().is_some() {
        format!("{snippet}…")
    } else {
        snippet
    }
}

pub(in crate::desktop::view) fn is_searchable_conversation_role(role: &str) -> bool {
    TranscriptRole::from_str(role).is_some_and(TranscriptRole::is_searchable_conversation)
}

pub(in crate::desktop::view) fn model_palette_command(model: &protocol::ModelInfo, current: bool) -> PaletteCommand {
    let context = model
        .context_window
        .map(|tokens| rust_i18n::t!("app.palette.context_suffix", window = format_context_window(tokens)).to_string())
        .unwrap_or_default();
    let current = if current {
        rust_i18n::t!("app.palette.current_suffix").to_string()
    } else {
        String::new()
    };
    let capabilities = model.capabilities.as_ref().map(model_capability_badges).unwrap_or_default();
    let mut entry = command(
        model.display_name.clone(),
        PaletteAction::SelectModel {
            provider: model.provider.clone(),
            model: model.id.clone(),
        },
    );
    let provider_label = provider_display_label(&model.provider);
    let mut details = vec![provider_label.clone()];
    details.extend(capabilities.iter().map(|(_, display)| display.clone()));
    if !context.is_empty() {
        details.push(context.trim_start_matches(display::MIDDOT).to_string());
    }
    details.push(model.id.clone());
    if !current.is_empty() {
        details.push(current.trim_start_matches(display::MIDDOT).to_string());
    }
    entry.description = Some(details.join(display::MIDDOT).into());
    entry.keywords = vec![model.provider.clone(), provider_label, model.id.clone()];
    entry
        .keywords
        .extend(capabilities.into_iter().map(|(keyword, _)| keyword.to_string()));
    entry
}

/// Capability badges as `(search_keyword, display_label)` pairs: keywords stay
/// English so palette search is locale-independent, while display labels are
/// localized. Tier and format names (`Pro`, `PDF`) are proper nouns/symbols
/// and stay English in both positions.
pub(in crate::desktop::view) fn model_capability_badges(
    capabilities: &protocol::ModelCapabilities,
) -> Vec<(&'static str, String)> {
    let mut badges = Vec::new();
    if capabilities.reasoning {
        badges.push(("Reasoning", rust_i18n::t!("app.palette.cap_reasoning").to_string()));
    }
    if capabilities.vision {
        badges.push(("Vision", rust_i18n::t!("app.palette.cap_vision").to_string()));
    }
    if capabilities.tool_call {
        badges.push(("Tools", rust_i18n::t!("app.palette.cap_tools").to_string()));
    }
    if capabilities.structured_output {
        badges.push(("Structured output", rust_i18n::t!("app.palette.cap_structured").to_string()));
    }
    if capabilities.is_pro {
        badges.push(("Pro", "Pro".to_string()));
    }
    for (modality, keyword) in [("audio", "Audio"), ("video", "Video"), ("pdf", "PDF")] {
        if capabilities
            .input_modalities
            .iter()
            .any(|value| value.eq_ignore_ascii_case(modality))
        {
            let display = match modality {
                "audio" => rust_i18n::t!("app.palette.cap_audio").to_string(),
                "video" => rust_i18n::t!("app.palette.cap_video").to_string(),
                _ => keyword.to_string(),
            };
            badges.push((keyword, display));
        }
    }
    badges
}
