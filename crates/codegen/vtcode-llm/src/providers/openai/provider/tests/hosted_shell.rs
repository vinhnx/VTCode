use super::*;

#[test]
fn responses_payload_uses_hosted_shell_when_enabled() {
    let provider = OpenAIProvider::from_config(
        Some(String::new()),
        None,
        Some(models::openai::GPT_5.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        Some(hosted_shell_openai_config()),
        None,
    );
    let request = shell_request(models::openai::GPT_5);
    let payload = provider
        .convert_to_openai_responses_format(&request)
        .expect("conversion should succeed");
    let tool = payload["tools"][0].as_object().expect("tool entry should be object");
    assert_str_field_obj(tool, "type", "shell");
    assert_eq!(tool["environment"]["type"].as_str(), Some("container_auto"));
    assert_eq!(tool["environment"]["network_policy"]["type"].as_str(), Some("disabled"));
    assert_eq!(tool["environment"]["file_ids"][0].as_str(), Some("file_123"));
    assert_eq!(tool["environment"]["skills"][0]["type"].as_str(), Some("skill_reference"));
    assert!(tool["environment"]["skills"][0].get("version").is_none());
    assert_absent(&payload, "output_types");
}

#[test]
fn responses_payload_serializes_hosted_shell_allowlist_and_domain_secrets() {
    let provider = OpenAIProvider::from_config(
        Some(String::new()),
        None,
        Some(models::openai::GPT_5.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        Some(OpenAIConfig {
            hosted_shell: OpenAIHostedShellConfig {
                enabled: true,
                environment: OpenAIHostedShellEnvironment::ContainerAuto,
                container_id: None,
                file_ids: Vec::new(),
                skills: Vec::new(),
                network_policy: OpenAIHostedShellNetworkPolicy {
                    policy_type: OpenAIHostedShellNetworkPolicyType::Allowlist,
                    allowed_domains: vec!["httpbin.org".to_string()],
                    domain_secrets: vec![OpenAIHostedShellDomainSecret {
                        domain: "httpbin.org".to_string(),
                        name: "API_KEY".to_string(),
                        value: "debug-secret-123".to_string(),
                    }],
                },
            },
            ..Default::default()
        }),
        None,
    );
    let request = shell_request(models::openai::GPT_5);
    let payload = provider.convert_to_openai_responses_format(&request).expect("should succeed");
    let np = &payload["tools"][0]["environment"]["network_policy"];
    assert_eq!(np["type"].as_str(), Some("allowlist"));
    assert_eq!(np["allowed_domains"][0].as_str(), Some("httpbin.org"));
    assert_eq!(np["domain_secrets"][0]["domain"].as_str(), Some("httpbin.org"));
    assert_eq!(np["domain_secrets"][0]["name"].as_str(), Some("API_KEY"));
    assert_eq!(np["domain_secrets"][0]["value"].as_str(), Some("debug-secret-123"));
}

#[test]
fn responses_payload_omits_explicit_latest_version_and_uses_container_reference() {
    let provider = OpenAIProvider::from_config(
        Some(String::new()),
        None,
        Some(models::openai::GPT_5.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        Some(OpenAIConfig {
            hosted_shell: OpenAIHostedShellConfig {
                enabled: true,
                environment: OpenAIHostedShellEnvironment::ContainerAuto,
                container_id: None,
                file_ids: Vec::new(),
                skills: vec![OpenAIHostedSkill::SkillReference {
                    skill_id: "skill_123".to_string(),
                    version: OpenAIHostedSkillVersion::String(" latest ".to_string()),
                }],
                network_policy: OpenAIHostedShellNetworkPolicy::default(),
            },
            ..Default::default()
        }),
        None,
    );
    let request = shell_request(models::openai::GPT_5);
    let payload = provider.convert_to_openai_responses_format(&request).expect("should succeed");
    assert!(payload["tools"][0]["environment"]["skills"][0].get("version").is_none());

    // Container reference should use container_id, omit file_ids/skills
    let provider2 = OpenAIProvider::from_config(
        Some(String::new()),
        None,
        Some(models::openai::GPT_5.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        Some(OpenAIConfig {
            hosted_shell: OpenAIHostedShellConfig {
                enabled: true,
                environment: OpenAIHostedShellEnvironment::ContainerReference,
                container_id: Some("cntr_123".to_string()),
                file_ids: vec!["file_ignored".to_string()],
                skills: vec![OpenAIHostedSkill::SkillReference {
                    skill_id: "skill_ignored".to_string(),
                    version: OpenAIHostedSkillVersion::default(),
                }],
                network_policy: OpenAIHostedShellNetworkPolicy::default(),
            },
            ..Default::default()
        }),
        None,
    );
    let request2 = shell_request(models::openai::GPT_5);
    let payload2 = provider2.convert_to_openai_responses_format(&request2).expect("should succeed");
    let env = &payload2["tools"][0]["environment"];
    assert_eq!(env["type"].as_str(), Some("container_reference"));
    assert_eq!(env["container_id"].as_str(), Some("cntr_123"));
    assert!(env.get("file_ids").is_none() && env.get("skills").is_none());
}

#[test]
fn hosted_shell_keeps_local_tool_when_conditions_not_met() {
    // Non-native URL
    let p1 = OpenAIProvider::from_config(
        Some(String::new()),
        None,
        Some(models::openai::GPT_5.to_string()),
        Some("https://example.com/v1".to_string()),
        None,
        None,
        None,
        Some(hosted_shell_openai_config()),
        None,
    );
    let payload1 = p1
        .convert_to_openai_responses_format(&shell_request(models::openai::GPT_5))
        .expect("should succeed");
    let t1 = payload1["tools"][0].as_object().expect("tool");
    assert_str_field_obj(t1, "type", "function");
    assert_str_field_obj(t1, "name", "shell");

    // Blank container reference
    let p2 = OpenAIProvider::from_config(
        Some(String::new()),
        None,
        Some(models::openai::GPT_5.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        Some(OpenAIConfig {
            hosted_shell: OpenAIHostedShellConfig {
                enabled: true,
                environment: OpenAIHostedShellEnvironment::ContainerReference,
                container_id: Some("   ".to_string()),
                file_ids: Vec::new(),
                skills: Vec::new(),
                network_policy: OpenAIHostedShellNetworkPolicy::default(),
            },
            ..Default::default()
        }),
        None,
    );
    let payload2 = p2
        .convert_to_openai_responses_format(&shell_request(models::openai::GPT_5))
        .expect("should succeed");
    let t2 = payload2["tools"][0].as_object().expect("tool");
    assert_str_field_obj(t2, "type", "function");
    assert_str_field_obj(t2, "name", "shell");

    // Blank skill ID
    let p3 = OpenAIProvider::from_config(
        Some(String::new()),
        None,
        Some(models::openai::GPT_5.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        Some(OpenAIConfig {
            hosted_shell: OpenAIHostedShellConfig {
                enabled: true,
                environment: OpenAIHostedShellEnvironment::ContainerAuto,
                container_id: None,
                file_ids: Vec::new(),
                skills: vec![OpenAIHostedSkill::SkillReference {
                    skill_id: "   ".to_string(),
                    version: OpenAIHostedSkillVersion::default(),
                }],
                network_policy: OpenAIHostedShellNetworkPolicy::default(),
            },
            ..Default::default()
        }),
        None,
    );
    let payload3 = p3
        .convert_to_openai_responses_format(&shell_request(models::openai::GPT_5))
        .expect("should succeed");
    assert_str_field_obj(payload3["tools"][0].as_object().expect("tool"), "type", "function");

    // Empty allowlist
    let p4 = OpenAIProvider::from_config(
        Some(String::new()),
        None,
        Some(models::openai::GPT_5.to_string()),
        Some("https://api.openai.com/v1".to_string()),
        None,
        None,
        None,
        Some(OpenAIConfig {
            hosted_shell: OpenAIHostedShellConfig {
                enabled: true,
                environment: OpenAIHostedShellEnvironment::ContainerAuto,
                container_id: None,
                file_ids: Vec::new(),
                skills: Vec::new(),
                network_policy: OpenAIHostedShellNetworkPolicy {
                    policy_type: OpenAIHostedShellNetworkPolicyType::Allowlist,
                    allowed_domains: Vec::new(),
                    domain_secrets: Vec::new(),
                },
            },
            ..Default::default()
        }),
        None,
    );
    let payload4 = p4
        .convert_to_openai_responses_format(&shell_request(models::openai::GPT_5))
        .expect("should succeed");
    assert_str_field_obj(payload4["tools"][0].as_object().expect("tool"), "type", "function");
}
