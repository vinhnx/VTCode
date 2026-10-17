//! Integration tests for Agent Plugin discovery, validation, installation, and removal.

use std::path::PathBuf;
use vtcode_agent_plugins::*;

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures");

fn fixture(path: &str) -> PathBuf {
    PathBuf::from(FIXTURES).join(path)
}

#[test]
fn parse_minimal_manifest() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test"}"#;
    let (manifest, unknown) = PluginManifest::parse(content).unwrap();
    assert_eq!(manifest.name, "test");
    assert_eq!(manifest.schema, "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json");
    assert!(unknown.is_empty());
}

#[test]
fn parse_full_manifest() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json",
        "name": "example-plugin",
        "version": "1.0.0",
        "description": "An example plugin",
        "author": {"name": "Author", "email": "a@b.com", "url": "https://example.com"},
        "homepage": "https://docs.example.com",
        "repository": "https://github.com/example/plugin",
        "license": "MIT",
        "keywords": ["example", "test"],
        "extensions": {"com.vtcode": {"setting": true}}
    }"#;
    let (manifest, unknown) = PluginManifest::parse(content).unwrap();
    assert_eq!(manifest.name, "example-plugin");
    assert_eq!(manifest.version, Some("1.0.0".into()));
    assert_eq!(manifest.description, Some("An example plugin".into()));
    assert!(manifest.author.is_some());
    assert_eq!(manifest.author.unwrap().name, Some("Author".into()));
    assert!(unknown.is_empty());
}

#[test]
fn reject_invalid_name_uppercase() {
    assert!(PluginManifest::validate_name("My-Plugin").is_err());
}

#[test]
fn reject_invalid_name_leading_hyphen() {
    assert!(PluginManifest::validate_name("-start").is_err());
}

#[test]
fn reject_invalid_name_consecutive_hyphens() {
    assert!(PluginManifest::validate_name("has--double").is_err());
}

#[test]
fn reject_invalid_name_consecutive_periods() {
    assert!(PluginManifest::validate_name("too.many..dots").is_err());
}

#[test]
fn reject_empty_name() {
    assert!(PluginManifest::validate_name("").is_err());
}

#[test]
fn accept_valid_names() {
    assert!(PluginManifest::validate_name("a").is_ok());
    assert!(PluginManifest::validate_name("my-plugin").is_ok());
    assert!(PluginManifest::validate_name("a.b-c.2").is_ok());
}

#[test]
fn reject_missing_required_fields() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json"}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn reject_unsupported_schema() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/9.9.9/plugin.schema.json", "name": "test"}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn reject_wrong_typed_optional_field() {
    // A wrong-typed optional field must fail loudly rather than be silently dropped.
    let content =
        r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test", "version": 42}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn reject_keywords_not_array() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test", "keywords": "not-an-array"}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn reject_keywords_non_string_elements() {
    // Non-string elements in keywords must fail loudly, not be silently dropped.
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test", "keywords": [1, "two"]}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn accept_valid_keywords() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test", "keywords": ["one", "two"]}"#;
    let (manifest, _) = PluginManifest::parse(content).unwrap();
    assert_eq!(manifest.keywords, Some(vec!["one".into(), "two".into()]));
}

#[test]
fn ignore_non_object_extensions() {
    // Per the Agent Plugins spec, a non-object `extensions` value is reported
    // and ignored — the plugin continues loading. This includes null, strings,
    // arrays, numbers, and booleans.
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test", "extensions": "not-an-object"}"#;
    let (manifest, _) = PluginManifest::parse(content).unwrap();
    assert!(manifest.extensions.is_none(), "non-object extensions must be ignored, not fatal");

    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test", "extensions": null}"#;
    let (manifest, _) = PluginManifest::parse(content).unwrap();
    assert!(manifest.extensions.is_none(), "null extensions must be ignored, not fatal");
}

#[test]
fn accept_valid_extensions() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test", "extensions": {"foo": true}}"#;
    let (manifest, _) = PluginManifest::parse(content).unwrap();
    assert!(manifest.extensions.is_some());
    assert!(manifest.extensions.unwrap().contains_key("foo"));
}

#[test]
fn report_unknown_fields() {
    let content =
        r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "test", "unknown": true}"#;
    let (_, unknown) = PluginManifest::parse(content).unwrap();
    assert_eq!(unknown, vec!["unknown"]);
}

#[test]
fn reject_bad_author_field() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "bad", "author": {"name": 123}}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn reject_bad_author_unsupported_field() {
    let content = r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "bad", "author": {"name": "Test", "unsupported": true}}"#;
    assert!(PluginManifest::parse(content).is_err());
}

#[test]
fn parse_stdio_mcp_server() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "test": {
                "type": "stdio",
                "command": "npx",
                "args": ["-y", "@modelcontextprotocol/server-test"],
                "env": {"KEY": "value"},
                "cwd": "./data"
            }
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert_eq!(config.schema, "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json");
    assert!(config.servers.contains_key("test"));
}

#[test]
fn parse_http_mcp_server() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "remote": {
                "type": "streamable-http",
                "url": "https://example.com/mcp",
                "headers": {"X-Tenant": "public"}
            }
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(config.servers.contains_key("remote"));
}

#[test]
fn parse_sse_mcp_server() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "legacy": {
                "type": "sse",
                "url": "https://legacy.example.com/sse"
            }
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(config.servers.contains_key("legacy"));
}

#[test]
fn skip_invalid_server_entry() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "valid": {"type": "stdio", "command": "echo"},
            "invalid": {"type": "stdio"}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(config.servers.contains_key("valid"));
    assert!(!config.servers.contains_key("invalid"));
}

#[test]
fn skip_wrong_typed_required_fields() {
    // A present-but-wrong-typed required field (e.g. "type": 42) must skip
    // the server, not be misdiagnosed as "missing". The valid peer still loads.
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "bad_type": {"type": 42, "command": "echo"},
            "bad_command": {"type": "stdio", "command": 99},
            "good": {"type": "stdio", "command": "echo"}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(!config.servers.contains_key("bad_type"), "wrong-typed type field must skip server");
    assert!(!config.servers.contains_key("bad_command"), "wrong-typed command field must skip server");
    assert!(config.servers.contains_key("good"), "valid server must still load");
}

#[test]
fn report_unknown_mcp_top_level_fields() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {},
        "extra": true
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert_eq!(config.unknown_fields, vec!["extra"]);
}

#[test]
fn reject_unsupported_mcp_schema() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/9.9.9/mcp.schema.json",
        "mcpServers": {}
    }"#;
    assert!(McpConfig::parse(content).is_err());
}

#[test]
fn skip_wrong_typed_mcp_fields() {
    // Per the Agent Plugins spec, an invalid individual server entry is
    // skipped while valid peers continue loading. Wrong-typed fields cause
    // the server to be skipped (with a warning), not the whole MCP config
    // to fail — one bad server must not disable every server in the plugin.

    // args as string → server skipped, parse succeeds
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "bad": {"type": "stdio", "command": "echo", "args": "not-an-array"},
            "good": {"type": "stdio", "command": "echo"}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(!config.servers.contains_key("bad"), "wrong-typed server must be skipped");
    assert!(config.servers.contains_key("good"), "valid server must still load");

    // args with non-string elements → server skipped
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "bad": {"type": "stdio", "command": "echo", "args": [1, 2]}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(!config.servers.contains_key("bad"));

    // cwd as number → server skipped
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "bad": {"type": "stdio", "command": "echo", "cwd": 42}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(!config.servers.contains_key("bad"));
}

#[test]
fn load_example_plugin() {
    let plugin = LoadedPlugin::load_from_dir(&fixture("agent-plugins-example")).unwrap();
    assert_eq!(plugin.manifest.name, "agent-plugins-example");
    assert_eq!(plugin.manifest.version, Some("1.0.0".into()));
    assert_eq!(plugin.skills.len(), 1);
    assert_eq!(plugin.skills[0].name, "migrate-agent-plugin");
    assert_eq!(plugin.skills[0].dir_name, "migrate-agent-plugin");
    assert!(plugin.mcp.is_none());
}

#[test]
fn reject_invalid_name_plugin() {
    let result = LoadedPlugin::load_from_dir(&fixture("invalid-name"));
    assert!(result.is_err());
}

#[test]
fn reject_missing_name_plugin() {
    let result = LoadedPlugin::load_from_dir(&fixture("missing-name"));
    assert!(result.is_err());
}

#[test]
fn reject_bad_author_plugin() {
    let result = LoadedPlugin::load_from_dir(&fixture("bad-author"));
    assert!(result.is_err());
}

#[test]
fn partial_mcp_still_loads_skills() {
    let plugin = LoadedPlugin::load_from_dir(&fixture("partial-mcp")).unwrap();
    // mcp.json has an invalid server entry, but the plugin itself loads
    assert_eq!(plugin.manifest.name, "partial-mcp");
    assert!(plugin.mcp.is_some());
    assert!(plugin.mcp.unwrap().servers.contains_key("valid"));
}

#[test]
fn expand_placeholders_basic() {
    let root = PathBuf::from("/home/user/.agents/plugins/test");
    let data = PathBuf::from("/home/user/.agents/plugins/data/test");
    let expanded = expand_placeholders("${PLUGIN_ROOT}/config", &root, &data);
    assert_eq!(expanded, "/home/user/.agents/plugins/test/config");
}

#[test]
fn expand_placeholders_multiple() {
    let root = PathBuf::from("/home/user/.agents/plugins/test");
    let data = PathBuf::from("/home/user/.agents/plugins/data/test");
    let expanded = expand_placeholders("${PLUGIN_ROOT}/config:${PLUGIN_DATA}/state", &root, &data);
    assert_eq!(expanded, "/home/user/.agents/plugins/test/config:/home/user/.agents/plugins/data/test/state");
}

#[test]
fn validate_plugin_relative_accepts_dot_slash() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    assert!(validate_plugin_relative("./bin/server", &root).is_ok());
}

#[test]
fn validate_plugin_relative_rejects_bare_path() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    assert!(validate_plugin_relative("bin/server", &root).is_err());
}

#[test]
fn validate_plugin_relative_rejects_parent_escape() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    assert!(validate_plugin_relative("../bin/server", &root).is_err());
}

#[cfg(unix)]
#[test]
fn validate_plugin_relative_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("plugin");
    std::fs::create_dir_all(&root).unwrap();
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    symlink(&outside, root.join("link")).unwrap();
    assert!(validate_plugin_relative("./link", &root).is_err());
}

#[test]
fn validate_plugin_relative_accepts_nonexistent_leaf() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().to_path_buf();
    // A leaf that does not exist yet (e.g. a binary built at runtime) is fine
    // as long as its existing ancestors stay inside the root.
    std::fs::create_dir_all(root.join("bin")).unwrap();
    assert!(validate_plugin_relative("./bin/not-yet-built", &root).is_ok());
}

#[test]
fn install_refuses_duplicate_plugin_with_actionable_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let fixture_dir = fixture("agent-plugins-example");
    let installer = FileSystemPluginInstaller::with_base_dir(tmp.path().to_path_buf());
    let installed = installer
        .install(fixture_dir.to_str().unwrap(), Some("dup-plugin".into()))
        .unwrap();
    assert!(installed.path.join("plugin.json").is_file());

    let err = installer
        .install(fixture_dir.to_str().unwrap(), Some("dup-plugin".into()))
        .err()
        .expect("duplicate install must fail");
    match err {
        PluginError::AlreadyInstalled(message) => {
            assert!(message.contains("dup-plugin"), "message should name the plugin: {message}");
            assert!(message.contains("plugins remove"), "message should suggest removal: {message}");
            assert!(message.contains("--name"), "message should suggest --name: {message}");
        }
        other => panic!("expected AlreadyInstalled, got: {other}"),
    }

    installer.remove("dup-plugin").unwrap();
}

#[test]
fn remove_reports_unknown_plugin_with_actionable_error() {
    let tmp = tempfile::TempDir::new().unwrap();
    let installer = FileSystemPluginInstaller::with_base_dir(tmp.path().to_path_buf());
    let err = installer.remove("never-installed-plugin").expect_err("remove must fail");
    match err {
        PluginError::NotInstalled(message) => {
            assert!(message.contains("never-installed-plugin"), "message should name the plugin: {message}");
            assert!(message.contains("plugins add"), "message should suggest install: {message}");
        }
        other => panic!("expected NotInstalled, got: {other}"),
    }
}

#[test]
fn install_refuses_path_traversal_in_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let fixture_dir = fixture("agent-plugins-example");
    let installer = FileSystemPluginInstaller::with_base_dir(tmp.path().to_path_buf());

    for evil in ["../evil", "../../etc", "..", "foo/../../bar", "/etc/evil"] {
        match installer.install(fixture_dir.to_str().unwrap(), Some(evil.into())) {
            Err(PluginError::InvalidName(_)) => {}
            Ok(_) => panic!("name {evil:?} unexpectedly installed"),
            Err(e) => panic!("name {evil:?} expected InvalidName, got: {e}"),
        }
    }

    // Nothing may have been written outside the plugins root.
    let plugins_root = tmp.path().join(".agents/plugins");
    assert!(!plugins_root.exists() || std::fs::read_dir(&plugins_root).unwrap().next().is_none());
}

#[test]
fn remove_refuses_path_traversal_in_name() {
    let tmp = tempfile::TempDir::new().unwrap();
    let installer = FileSystemPluginInstaller::with_base_dir(tmp.path().to_path_buf());

    // A sentinel file that a traversal remove() must not be able to delete.
    let sentinel = tmp.path().join("sentinel.txt");
    std::fs::write(&sentinel, "keep me").unwrap();

    for evil in ["../sentinel.txt", "../../sentinel.txt", "..", ".", ""] {
        match installer.remove(evil) {
            Err(PluginError::InvalidName(_)) => {}
            Ok(_) => panic!("name {evil:?} unexpectedly removed"),
            Err(e) => panic!("name {evil:?} expected InvalidName, got: {e}"),
        }
    }

    assert!(sentinel.is_file(), "traversal remove() deleted a file outside the plugins root");
}

#[test]
fn reject_cross_variant_fields() {
    // Closed variants (§7.2.1): a stdio-only field on http, or vice versa,
    // invalidates only that entry.
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "stdio_with_url": {"type": "stdio", "command": "echo", "url": "https://example.com/mcp"},
            "http_with_command": {"type": "streamable-http", "url": "https://example.com/mcp", "command": "echo"},
            "http_with_cwd": {"type": "streamable-http", "url": "https://example.com/mcp", "cwd": "./data"},
            "good_stdio": {"type": "stdio", "command": "echo"},
            "good_http": {"type": "streamable-http", "url": "https://example.com/mcp"}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(!config.servers.contains_key("stdio_with_url"));
    assert!(!config.servers.contains_key("http_with_command"));
    assert!(!config.servers.contains_key("http_with_cwd"));
    assert!(config.servers.contains_key("good_stdio"));
    assert!(config.servers.contains_key("good_http"));
}

#[test]
fn reject_invalid_command_tokens() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "shell_string": {"type": "stdio", "command": "npx --yes"},
            "absolute_path": {"type": "stdio", "command": "/usr/bin/python3"},
            "slash_bare": {"type": "stdio", "command": "bin/server"},
            "good_bare": {"type": "stdio", "command": "npx"},
            "good_relative": {"type": "stdio", "command": "./bin/server"}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(!config.servers.contains_key("shell_string"), "shell string must be rejected");
    assert!(!config.servers.contains_key("absolute_path"), "absolute path must use ./");
    assert!(!config.servers.contains_key("slash_bare"), "bare with slash must use ./");
    assert!(config.servers.contains_key("good_bare"));
    assert!(config.servers.contains_key("good_relative"));
}

#[test]
fn reject_invalid_cwd_forms() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "bare": {"type": "stdio", "command": "echo", "cwd": "data"},
            "absolute": {"type": "stdio", "command": "echo", "cwd": "/tmp"},
            "other_var": {"type": "stdio", "command": "echo", "cwd": "${HOME}/x"},
            "good_dot": {"type": "stdio", "command": "echo", "cwd": "./data"},
            "good_root": {"type": "stdio", "command": "echo", "cwd": "${PLUGIN_ROOT}"},
            "good_root_sub": {"type": "stdio", "command": "echo", "cwd": "${PLUGIN_ROOT}/sub"},
            "good_data": {"type": "stdio", "command": "echo", "cwd": "${PLUGIN_DATA}"},
            "good_data_sub": {"type": "stdio", "command": "echo", "cwd": "${PLUGIN_DATA}/sub"}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(!config.servers.contains_key("bare"));
    assert!(!config.servers.contains_key("absolute"));
    assert!(!config.servers.contains_key("other_var"));
    assert!(config.servers.contains_key("good_dot"));
    assert!(config.servers.contains_key("good_root"));
    assert!(config.servers.contains_key("good_root_sub"));
    assert!(config.servers.contains_key("good_data"));
    assert!(config.servers.contains_key("good_data_sub"));
}

#[test]
fn reject_reserved_env_keys() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "bad_root": {"type": "stdio", "command": "echo", "env": {"PLUGIN_ROOT": "/tmp"}},
            "bad_data": {"type": "stdio", "command": "echo", "env": {"PLUGIN_DATA": "/tmp"}},
            "good": {"type": "stdio", "command": "echo", "env": {"DATA_DIR": "${PLUGIN_DATA}/db"}}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(!config.servers.contains_key("bad_root"));
    assert!(!config.servers.contains_key("bad_data"));
    assert!(config.servers.contains_key("good"));
}

#[test]
fn reject_invalid_remote_urls() {
    // Asymmetric: https non-loopback loads, http non-loopback does not;
    // http loopback (localhost, 127.x, ::1) loads.
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "https_ok": {"type": "streamable-http", "url": "https://example.com/mcp"},
            "http_public": {"type": "streamable-http", "url": "http://example.com/mcp"},
            "http_localhost": {"type": "streamable-http", "url": "http://localhost:4317/mcp"},
            "http_127": {"type": "streamable-http", "url": "http://127.0.0.1:8000/mcp"},
            "http_127_range": {"type": "streamable-http", "url": "http://127.12.34.56/mcp"},
            "http_128": {"type": "streamable-http", "url": "http://128.0.0.1/mcp"},
            "http_v6": {"type": "streamable-http", "url": "http://[::1]/mcp"},
            "userinfo": {"type": "streamable-http", "url": "https://user@example.com/mcp"},
            "fragment": {"type": "streamable-http", "url": "https://example.com/mcp#frag"},
            "relative": {"type": "streamable-http", "url": "/mcp"}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(config.servers.contains_key("https_ok"));
    assert!(!config.servers.contains_key("http_public"), "non-loopback http must require https");
    assert!(config.servers.contains_key("http_localhost"));
    assert!(config.servers.contains_key("http_127"));
    assert!(config.servers.contains_key("http_127_range"), "127/8 is loopback");
    assert!(!config.servers.contains_key("http_128"), "128.x is not loopback");
    assert!(config.servers.contains_key("http_v6"));
    assert!(!config.servers.contains_key("userinfo"), "userinfo must be rejected");
    assert!(!config.servers.contains_key("fragment"), "fragment must be rejected");
    assert!(!config.servers.contains_key("relative"), "relative url must be rejected");
}

#[test]
fn reject_invalid_headers() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "bad_name": {"type": "streamable-http", "url": "https://example.com/mcp", "headers": {"Bad Name": "v"}},
            "bad_value": {"type": "streamable-http", "url": "https://example.com/mcp", "headers": {"X-Ok": "a\nb"}},
            "good": {"type": "streamable-http", "url": "https://example.com/mcp", "headers": {"X-Tenant": "public"}}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(!config.servers.contains_key("bad_name"), "header name with space must be rejected");
    assert!(!config.servers.contains_key("bad_value"), "header value with newline must be rejected");
    assert!(config.servers.contains_key("good"));
}

#[test]
fn expand_placeholders_is_single_pass() {
    // Text introduced by replacement must not be rescanned (§9.2).
    let root = PathBuf::from("/r/${PLUGIN_DATA}");
    let data = PathBuf::from("/d");
    let expanded = expand_placeholders("${PLUGIN_ROOT}/a", &root, &data);
    assert_eq!(expanded, "/r/${PLUGIN_DATA}/a", "replacement text must stay literal");
    let expanded = expand_placeholders("${PLUGIN_DATA}/${PLUGIN_ROOT}", &root, &data);
    assert_eq!(expanded, "/d//r/${PLUGIN_DATA}");
}

#[test]
fn default_plugin_data_lives_outside_package() {
    let dir = default_plugin_data_dir("my-plugin");
    let mut parts: Vec<_> = dir.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    assert!(parts.len() >= 2, "data dir must have parent components: {}", dir.display());
    let tail = parts.split_off(parts.len() - 2);
    assert_eq!(tail, vec!["plugin-data".to_string(), "my-plugin".to_string()]);
    // Must not be inside a hypothetical package root.
    let package = PathBuf::from("/tmp/pkg/my-plugin");
    assert!(!dir.starts_with(&package));
}

#[test]
fn resolve_cwd_rejects_dotdot_escape() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("plugin");
    std::fs::create_dir_all(&root).unwrap();
    let data = tmp.path().join("plugin-data");
    std::fs::create_dir_all(&data).unwrap();
    // `${PLUGIN_ROOT}/../evil` passes the syntactic form check but must fail
    // containment after expansion.
    assert!(resolve_cwd(Some("${PLUGIN_ROOT}/../evil"), &root, &data).is_err());
    assert!(resolve_cwd(Some("${PLUGIN_DATA}/../sibling"), &root, &data).is_err());
    assert!(resolve_cwd(Some("./ok-subdir"), &root, &data).is_ok());
}

#[test]
fn uppercase_scheme_is_accepted() {
    let content = r#"{
        "$schema": "https://agent-plugins.org/schemas/1.0.0/mcp.schema.json",
        "mcpServers": {
            "upper": {"type": "streamable-http", "url": "HTTPS://example.com/mcp"}
        }
    }"#;
    let config = McpConfig::parse(content).unwrap();
    assert!(config.servers.contains_key("upper"), "scheme must be case-insensitive");
}

#[test]
fn reject_bare_directory_command() {
    assert!(validate_command_token("./").is_err());
    assert!(validate_command_token("./.").is_err());
    assert!(validate_command_token("npx").is_ok());
    assert!(validate_command_token("./bin/server").is_ok());
}

#[test]
fn duplicate_skill_names_keep_first() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("plugin");
    std::fs::create_dir_all(root.join("skills/a")).unwrap();
    std::fs::create_dir_all(root.join("skills/b")).unwrap();
    std::fs::write(
        root.join("plugin.json"),
        r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "dup-test"}"#,
    )
    .unwrap();
    for dir in ["a", "b"] {
        std::fs::write(
            root.join(format!("skills/{dir}/SKILL.md")),
            "---\nname: same-name\ndescription: dup skill\n---\n# Body\n",
        )
        .unwrap();
    }
    let plugin = LoadedPlugin::load_from_dir(&root).unwrap();
    assert_eq!(plugin.skills.len(), 1, "duplicate manifest names must dedup to first");
    assert_eq!(plugin.skills[0].name, "same-name");
}

#[test]
fn skill_dir_mismatch_warns_but_loads() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("plugin");
    std::fs::create_dir_all(root.join("skills/renamed-dir")).unwrap();
    std::fs::write(
        root.join("plugin.json"),
        r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "mismatch-test"}"#,
    )
    .unwrap();
    std::fs::write(
        root.join("skills/renamed-dir/SKILL.md"),
        "---\nname: original-name\ndescription: renamed on install\n---\n# Body\n",
    )
    .unwrap();
    let plugin = LoadedPlugin::load_from_dir(&root).unwrap();
    assert_eq!(plugin.skills.len(), 1);
    assert_eq!(plugin.skills[0].name, "original-name");
    assert_eq!(plugin.skills[0].dir_name, "renamed-dir");
}

#[cfg(unix)]
#[test]
fn skills_symlink_escape_is_isolated() {
    use std::os::unix::fs::symlink;
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path().join("plugin");
    std::fs::create_dir_all(root.join("skills/good")).unwrap();
    std::fs::write(
        root.join("plugin.json"),
        r#"{"$schema": "https://agent-plugins.org/schemas/1.0.0/plugin.schema.json", "name": "escape-test"}"#,
    )
    .unwrap();
    std::fs::write(root.join("skills/good/SKILL.md"), "---\nname: good\ndescription: good skill\n---\n# Body\n")
        .unwrap();
    let outside = tmp.path().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("SKILL.md"), "---\nname: evil\ndescription: evil\n---\n# Body\n").unwrap();
    symlink(&outside, root.join("skills/evil")).unwrap();
    let plugin = LoadedPlugin::load_from_dir(&root).unwrap();
    assert!(plugin.skills.iter().any(|s| s.name == "good"));
    assert!(!plugin.skills.iter().any(|s| s.name == "evil"), "escaping skill must be skipped");
}
