use super::*;

const MATRIX: &[VerifiedTarget] = &[VerifiedTarget {
    provider: "anthropic",
    model: "claude-sonnet-5",
    endpoint: "https://api.anthropic.com",
    protocol: "computer_toolset_20260801",
}];

#[test]
fn computer_use_gate_denies_everything_until_a_probe_lands() {
    assert!(!verified_target_default(
        "anthropic",
        "claude-sonnet-5",
        "https://api.anthropic.com",
        "computer_toolset_20260801"
    ));
    assert!(!verified_target_default("openai", "gpt-6.2-sol", "https://api.openai.com/v1", "computer"));
}

#[test]
fn computer_use_gate_requires_an_exact_match_in_every_dimension() {
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
