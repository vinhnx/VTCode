use super::*;

#[test]
fn native_routes_keep_openai_and_anthropic_supported() {
    assert_eq!(NATIVE_ROUTES.len(), 2, "only the two implemented native routes may be admitted");
    assert!(native_route_supported(Provider::OpenAI, OPENAI_COMPUTER_PROTOCOL));
    assert!(native_route_supported(Provider::Anthropic, ANTHROPIC_COMPUTER_PROTOCOL));
    for (provider, protocol) in NATIVE_ROUTES {
        assert!(native_route_supported(*provider, protocol), "{provider}/{protocol}");
    }
}

#[test]
fn native_route_admission_denies_unknown_routes_before_io() {
    // Admission does not depend on model or endpoint, but it does depend on a
    // declared provider/protocol pair. Unknown routes stay fail-closed.
    for (provider, protocol) in [
        (Provider::Gemini, "computer"),
        (Provider::OpenAI, "computer_20251124"),
        (Provider::Anthropic, "computer"),
        (Provider::OpenAI, ANTHROPIC_COMPUTER_PROTOCOL),
    ] {
        assert!(!native_route_supported(provider, protocol), "admitted {provider}/{protocol}");
    }
}

const MATRIX: &[VerifiedTarget] = &[VerifiedTarget {
    provider: "anthropic",
    model: "claude-sonnet-5",
    endpoint: "https://api.anthropic.com",
    protocol: "computer_toolset_20260801",
}];

#[test]
fn verified_probe_record_denies_tuples_without_a_probe() {
    assert!(!verified_target_default(
        "anthropic",
        "claude-sonnet-5",
        "https://api.anthropic.com",
        "computer_toolset_20260801"
    ));
    assert!(!verified_target_default("openai", "gpt-6.2-sol", "https://api.openai.com/v1", "computer"));
}

#[test]
fn verified_probe_record_matches_only_the_recorded_tuple() {
    assert!(verified_target_default("openai", "gpt-6-luna", "https://api.openai.com/v1", "computer"));
    for (provider, model, endpoint, protocol) in [
        ("openai", "gpt-6-luna-code", "https://api.openai.com/v1", "computer"),
        ("openai", "gpt-6-luna", "https://proxy.invalid/v1", "computer"),
        ("openai", "gpt-6-luna", "https://api.openai.com/v1", "computer_20251124"),
        ("openai", "GPT-6-LUNA", "https://api.openai.com/v1", "computer"),
    ] {
        assert!(
            !verified_target_default(provider, model, endpoint, protocol),
            "admitted {provider}/{model}/{endpoint}/{protocol}"
        );
    }
}

#[test]
fn verified_probe_record_requires_an_exact_match_in_every_dimension() {
    assert!(verified_target(
        MATRIX,
        "anthropic",
        "claude-sonnet-5",
        "https://api.anthropic.com",
        "computer_toolset_20260801"
    ));
    for (provider, model, endpoint, protocol) in [
        ("openai", "claude-sonnet-5", "https://api.anthropic.com", "computer_toolset_20260801"),
        ("anthropic", "claude-opus-5", "https://api.anthropic.com", "computer_toolset_20260801"),
        ("anthropic", "claude-sonnet-5", "https://proxy.invalid", "computer_toolset_20260801"),
        ("anthropic", "claude-sonnet-5", "https://api.anthropic.com", "computer_20251124"),
        // Matching is exact and case-sensitive: probes record the precise strings used.
        ("ANTHROPIC", "claude-sonnet-5", "https://api.anthropic.com", "computer_toolset_20260801"),
        ("", "", "", ""),
    ] {
        assert!(!verified_target(MATRIX, provider, model, endpoint, protocol), "admitted {provider}/{model}");
    }
}
