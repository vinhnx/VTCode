use super::SecurityConfig;

#[test]
fn test_default_security_config() {
    let config = SecurityConfig::default();
    assert!(config.human_in_the_loop);
    assert!(config.require_write_tool_for_claims);
    assert!(config.hitl_notification_bell);
}

#[test]
fn test_security_config_with_custom_bell_setting() {
    let config = SecurityConfig {
        hitl_notification_bell: false,
        ..Default::default()
    };
    assert!(!config.hitl_notification_bell);
}

#[test]
fn test_serialize_deserialize_security_config() {
    let original = SecurityConfig {
        human_in_the_loop: true,
        require_write_tool_for_claims: true,
        auto_apply_detected_patches: false,
        zero_trust_mode: false,
        encrypt_payloads: false,
        integrity_checks: true,
        hitl_notification_bell: true,
        gatekeeper: super::GatekeeperConfig::default(),
    };

    let serialized = serde_json::to_string(&original).unwrap();
    let deserialized: SecurityConfig = serde_json::from_str(&serialized).unwrap();

    assert_eq!(
        original.hitl_notification_bell,
        deserialized.hitl_notification_bell
    );
    assert_eq!(original.human_in_the_loop, deserialized.human_in_the_loop);
}
