use vtcode_config::{HooksConfig, VTCodeConfig};

#[test]
fn parses_ui_vim_mode_from_toml() {
    let config: VTCodeConfig = toml::from_str(
        r#"
[ui]
vim_mode = true
"#,
    )
    .expect("config should parse");

    assert!(config.ui.vim_mode);
}

#[test]
fn validates_notification_hook_groups() {
    let config: HooksConfig = toml::from_str(
        r#"
[lifecycle]

[[lifecycle.notification]]
matcher = "permission_prompt|idle_prompt"

[[lifecycle.notification.hooks]]
type = "command"
command = "echo notification"
"#,
    )
    .expect("hooks should parse");

    config.validate().expect("hooks should validate");
    assert_eq!(config.lifecycle.notification.len(), 1);
}
