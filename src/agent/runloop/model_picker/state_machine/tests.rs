use super::credential_for_openai_auth;
use vtcode_config::OpenAIAuthConfig;
use vtcode_config::api_keys::{CredentialSource, ResolvedCredential, credential_identity};
use vtcode_config::auth::{
    AuthCredentialsStoreMode, OpenAIChatGptAuthHandle, OpenAIChatGptSession, OpenAIResolvedAuth,
};

fn discovered_api_key() -> ResolvedCredential {
    ResolvedCredential {
        identity: credential_identity("openai", "OPENAI_API_KEY").unwrap().unwrap(),
        source: CredentialSource::Env,
        secret: Some("discovered-api-key".into()),
        env_var: Some("OPENAI_API_KEY".into()),
    }
}

#[test]
fn picker_credential_matches_runtime_subscription_instead_of_discovered_api_key() {
    let handle = OpenAIChatGptAuthHandle::new(
        OpenAIChatGptSession {
            openai_api_key: String::new(),
            id_token: String::new(),
            access_token: "subscription-token".into(),
            refresh_token: "refresh-token".into(),
            account_id: Some("fixture-account".into()),
            email: None,
            plan: None,
            obtained_at: 1,
            refreshed_at: 1,
            expires_at: None,
        },
        OpenAIAuthConfig::default(),
        AuthCredentialsStoreMode::File,
    );
    let credential = credential_for_openai_auth(
        discovered_api_key(),
        OpenAIResolvedAuth::ChatGpt { api_key: "subscription-token".into(), handle },
    );
    assert_eq!(credential.source, CredentialSource::OAuth);
    assert!(credential.secret.is_none());
    assert!(credential.env_var.is_none());
    assert_eq!(credential.identity.key_name(), "OPENAI_API_KEY");
}

#[test]
fn picker_api_key_credential_keeps_source_and_runtime_key() {
    let credential = credential_for_openai_auth(
        discovered_api_key(),
        OpenAIResolvedAuth::ApiKey { api_key: "runtime-api-key".into() },
    );
    assert_eq!(credential.source, CredentialSource::Env);
    assert_eq!(credential.secret.as_deref(), Some("runtime-api-key"));
    assert_eq!(credential.env_var.as_deref(), Some("OPENAI_API_KEY"));
}
