use super::*;

#[test]
fn prompt_cache_options_capability_inherits_and_preserves_false() {
    let defaults = CustomProviderProfileConfig {
        supports_prompt_cache_options: Some(true),
        ..Default::default()
    };
    assert_eq!(
        ResolvedCustomProviderProfile::from_layers(&defaults, None).supports_prompt_cache_options,
        Some(true)
    );
    let profile = CustomProviderProfileConfig {
        supports_prompt_cache_options: Some(false),
        ..Default::default()
    };
    assert_eq!(
        ResolvedCustomProviderProfile::from_layers(&defaults, Some(&profile)).supports_prompt_cache_options,
        Some(false)
    );
    assert_eq!(
        ResolvedCustomProviderProfile::from_layers(&Default::default(), None).supports_prompt_cache_options,
        None
    );
}
