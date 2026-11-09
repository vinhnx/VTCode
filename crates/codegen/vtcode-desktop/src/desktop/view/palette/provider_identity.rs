//! Provider/model identity helpers for the model palette: canonical grouping,
//! display labels, picker rank, and active-route matching.

use std::str::FromStr;
use vtcode_commons::provider::Provider;

/// Canonical grouping key for a provider code: known aliases (`google`,
/// `meta-ai`, `llama.cpp`, …) collapse to the canonical code so the active
/// fallback never opens a second group next to the catalog group. Unknown
/// (custom) provider codes pass through unchanged.
pub(super) fn provider_group_key(code: &str) -> String {
    Provider::from_str(code).map_or_else(|_| code.to_string(), |provider| provider.as_ref().to_string())
}

/// Whether two provider codes name the same provider. Known aliases resolve
/// through [`Provider::name_matches`] (the canonical matcher); unknown custom
/// codes fall back to a case-insensitive comparison.
pub(super) fn provider_codes_match(left: &str, right: &str) -> bool {
    let right = right.trim();
    Provider::from_str(left)
        .map_or_else(|_| left.trim().eq_ignore_ascii_case(right), |provider| provider.name_matches(right))
}

/// Whether a catalog/action route is the active provider + model route.
/// Provider aliases and model-id casing do not change route identity.
pub(super) fn model_route_matches(provider: &str, model: &str, current_provider: &str, current_model: &str) -> bool {
    provider_codes_match(provider, current_provider) && model.trim().eq_ignore_ascii_case(current_model.trim())
}

/// Human-readable provider name for palette group headers and entry details.
/// Unknown (custom) provider codes pass through unchanged.
pub(super) fn provider_display_label(code: &str) -> String {
    Provider::from_str(code).map_or_else(|_| code.to_string(), |provider| provider.label().to_string())
}

/// Rank of a provider code in the CLI picker's display order. Unknown codes
/// sort after every known provider.
pub(super) fn provider_picker_rank(code: &str, display_order: &[Provider]) -> usize {
    Provider::from_str(code)
        .ok()
        .and_then(|provider| display_order.iter().position(|ordered| *ordered == provider))
        .unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_alias_matches_canonical_code_in_both_directions() {
        assert!(provider_codes_match("google", "gemini"));
        assert!(provider_codes_match("gemini", "Google"));
        assert!(!provider_codes_match("google", "openai"));
    }

    #[test]
    fn unknown_custom_codes_match_case_insensitively_without_aliasing() {
        assert!(provider_codes_match("My-Provider", "my-provider"));
        assert!(!provider_codes_match("My-Provider", "other-provider"));
        assert!(
            !provider_codes_match("my-provider", "google"),
            "an unknown code must not borrow a known provider's aliases"
        );
    }

    #[test]
    fn route_identity_ignores_provider_alias_and_model_casing() {
        assert!(model_route_matches("google", "GEMINI-3-FLASH", "gemini", "gemini-3-flash"));
        assert!(!model_route_matches("google", "gemini-3-flash", "gemini", "gemini-3-pro"));
        assert!(!model_route_matches("openai", "gpt-5", "google", "gpt-5"));
    }

    #[test]
    fn unknown_codes_group_by_their_own_code_and_rank_last() {
        assert_eq!(provider_group_key("ollama-cloud"), "ollama-cloud");
        assert_eq!(provider_group_key("custom-lab"), "custom-lab");
        assert_eq!(provider_display_label("custom-lab"), "custom-lab");
        let order = Provider::picker_display_order();
        assert_eq!(provider_picker_rank("custom-lab", &order), usize::MAX);
        assert!(provider_picker_rank("openai", &order) < provider_picker_rank("gemini", &order));
    }
}
