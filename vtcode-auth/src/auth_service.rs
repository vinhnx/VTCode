//! Internal auth service contracts used by VT Code.

use anyhow::{Result, anyhow};
use std::sync::Arc;

use crate::AuthCredentialsStoreMode;
use crate::config::{OpenAIAuthConfig, OpenAIPreferredMethod};
use crate::openai_chatgpt_oauth::{
    OpenAIChatGptAuthHandle, OpenAIChatGptSession, OpenAIChatGptSessionRefresher,
    OpenAICredentialOverview, OpenAIResolvedAuth, OpenAIResolvedAuthSource,
    load_openai_chatgpt_session_with_mode,
};

/// Service contract for resolving VT Code's OpenAI account auth state.
#[derive(Debug, Clone)]
pub struct OpenAIAccountAuthService {
    auth_config: OpenAIAuthConfig,
    storage_mode: AuthCredentialsStoreMode,
}

impl OpenAIAccountAuthService {
    #[must_use]
    pub fn new(auth_config: OpenAIAuthConfig, storage_mode: AuthCredentialsStoreMode) -> Self {
        Self {
            auth_config,
            storage_mode,
        }
    }

    /// Resolve the active OpenAI auth source for the current configuration.
    pub fn resolve_runtime_auth(&self, api_key: Option<String>) -> Result<OpenAIResolvedAuth> {
        let session = load_openai_chatgpt_session_with_mode(self.storage_mode)?;
        match self.auth_config.preferred_method {
            OpenAIPreferredMethod::Chatgpt => {
                let session = session.ok_or_else(|| anyhow!("Run vtcode login openai"))?;
                let handle = OpenAIChatGptAuthHandle::new(
                    session,
                    self.auth_config.clone(),
                    self.storage_mode,
                );
                let api_key = handle.current_api_key()?;
                Ok(OpenAIResolvedAuth::ChatGpt { api_key, handle })
            }
            OpenAIPreferredMethod::ApiKey => {
                let api_key = require_api_key(api_key)?;
                Ok(OpenAIResolvedAuth::ApiKey { api_key })
            }
            OpenAIPreferredMethod::Auto => {
                if let Some(session) = session {
                    let handle = OpenAIChatGptAuthHandle::new(
                        session,
                        self.auth_config.clone(),
                        self.storage_mode,
                    );
                    let api_key = handle.current_api_key()?;
                    Ok(OpenAIResolvedAuth::ChatGpt { api_key, handle })
                } else {
                    let api_key = require_api_key(api_key)?;
                    Ok(OpenAIResolvedAuth::ApiKey { api_key })
                }
            }
        }
    }

    /// Resolve a non-persistent OpenAI auth session backed by externally managed tokens.
    pub fn resolve_external_session_auth(
        &self,
        session: OpenAIChatGptSession,
        refresher: Arc<dyn OpenAIChatGptSessionRefresher>,
    ) -> Result<OpenAIResolvedAuth> {
        let handle = OpenAIChatGptAuthHandle::new_external(
            session,
            self.auth_config.auto_refresh,
            refresher,
        );
        let api_key = handle.current_api_key()?;
        Ok(OpenAIResolvedAuth::ChatGpt { api_key, handle })
    }

    /// Summarize the available OpenAI credentials without mutating storage.
    pub fn summarize_credentials(
        &self,
        api_key: Option<String>,
    ) -> Result<OpenAICredentialOverview> {
        let chatgpt_session = load_openai_chatgpt_session_with_mode(self.storage_mode)?;
        let api_key_available = api_key
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty());
        let active_source = match self.auth_config.preferred_method {
            OpenAIPreferredMethod::Chatgpt => chatgpt_session
                .as_ref()
                .map(|_| OpenAIResolvedAuthSource::ChatGpt),
            OpenAIPreferredMethod::ApiKey => {
                api_key_available.then_some(OpenAIResolvedAuthSource::ApiKey)
            }
            OpenAIPreferredMethod::Auto => {
                if chatgpt_session.is_some() {
                    Some(OpenAIResolvedAuthSource::ChatGpt)
                } else if api_key_available {
                    Some(OpenAIResolvedAuthSource::ApiKey)
                } else {
                    None
                }
            }
        };

        let (notice, recommendation) = if api_key_available && chatgpt_session.is_some() {
            let active_label = match active_source {
                Some(OpenAIResolvedAuthSource::ChatGpt) => "ChatGPT subscription",
                Some(OpenAIResolvedAuthSource::ApiKey) => "OPENAI_API_KEY",
                None => "neither credential",
            };
            let recommendation = match active_source {
                Some(OpenAIResolvedAuthSource::ChatGpt) => {
                    "Next step: keep the current priority, run /logout openai to rely on API-key auth only, or set [auth.openai].preferred_method = \"api_key\"."
                }
                Some(OpenAIResolvedAuthSource::ApiKey) => {
                    "Next step: keep the current priority, remove OPENAI_API_KEY if ChatGPT should win, or set [auth.openai].preferred_method = \"chatgpt\"."
                }
                None => {
                    "Next step: choose a single preferred source or set [auth.openai].preferred_method explicitly."
                }
            };
            (
                Some(format!(
                    "Both ChatGPT subscription auth and OPENAI_API_KEY are available. VT Code is using {active_label} because auth.openai.preferred_method = {}.",
                    self.auth_config.preferred_method.as_str()
                )),
                Some(recommendation.to_string()),
            )
        } else {
            (None, None)
        };

        Ok(OpenAICredentialOverview {
            api_key_available,
            chatgpt_session,
            active_source,
            preferred_method: self.auth_config.preferred_method,
            notice,
            recommendation,
        })
    }
}

fn require_api_key(api_key: Option<String>) -> Result<String> {
    api_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow!("OpenAI API key not found"))
}
