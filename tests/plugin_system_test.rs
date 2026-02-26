use vtcode_core::config::PluginRuntimeConfig;
use vtcode_core::plugins::{PluginManager, PluginManifest, PluginValidator};

#[tokio::test]
async fn test_plugin_system_compilation() {
    // This test just verifies that the plugin system compiles and basic functionality works
    let plugins_dir = std::path::PathBuf::from("./test_plugins");
    let config = PluginRuntimeConfig::default();

    // Create a basic plugin manager - this should compile
    let _manager = PluginManager::new(config, plugins_dir).unwrap();

    // Create a basic manifest - this should compile
    let manifest = PluginManifest {
        name: "test-plugin".to_string(),
        version: Some("1.0.0".to_string()),
        description: Some("A test plugin".to_string()),
        author: None,
        homepage: None,
        repository: None,
        license: Some("MIT".to_string()),
        keywords: Some(vec!["test".to_string()]),
        commands: None,
        agents: None,
        skills: None,
        hooks: None,
        mcp_servers: None,
        output_styles: None,
        lsp_servers: None,
    };

    // Validate the manifest - this should compile
    assert!(PluginValidator::validate_manifest(&manifest).is_ok());
}
