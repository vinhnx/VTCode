//! Internal auth service contracts used by VT Code.

use anyhow::{Result, anyhow};
use std::sync::Arc;

use crate::AuthCredentialsStoreMode;
use crate::codex_auth_import::{codex_auth_json_refresher, is_session_expired, try_load_codex_chatgpt_session};
use crate::config::{OpenAIAuthConfig, OpenAIPreferredMethod};
use crate::openai_chatgpt_oauth::{
    OpenAIChatGptAuthHandle, OpenAIChatGptSession, OpenAIChatGptSessionProvenance, OpenAIChatGptSessionRefresher,
    OpenAICredentialOverview, OpenAIResolvedAuth, OpenAIResolvedAuthSource, load_openai_chatgpt_session_with_mode,
};

/// Service contract for resolving VT Code's OpenAI account auth state.
#[derive(Debug, Clone)]
pub struct OpenAIAccountAuthService {
    auth_config: OpenAIAuthConfig,
    storage_mode: AuthCredentialsStoreMode,
}

impl OpenAIAccountAuthService {
    #[must_use]
    pub(crate) fn new(auth_config: OpenAIAuthConfig, storage_mode: AuthCredentialsStoreMode) -> Self {
        Self { auth_config, storage_mode }
    }

    /// Resolve the active OpenAI auth source for the current configuration.
    pub(crate) fn resolve_runtime_auth(&self, api_key: Option<String>) -> Result<OpenAIResolvedAuth> {
        // Load the stored ChatGPT session lazily: the ApiKey path never uses it,
        // so an unreadable session store must not break API-key auth.
        match self.auth_config.preferred_method {
            OpenAIPreferredMethod::Chatgpt => {
                let session = load_openai_chatgpt_session_with_mode(self.storage_mode)?;
                match session {
                    Some(native) => {
                        let handle = self.handle_from_session(native);
                        let api_key = handle.current_api_key()?;
                        Ok(OpenAIResolvedAuth::ChatGpt { api_key, handle })
                    }
                    None => match self.try_codex_fallback()? {
                        Some(codex_session) => {
                            // Reject expired Codex sessions so runtime selection
                            // matches status display (summarize_credentials also
                            // filters expired Codex out). This prevents status
                            // from reporting no active credential while runtime
                            // silently selects an expired one.
                            if is_session_expired(&codex_session) {
                                Err(anyhow!(
                                    "Codex's ChatGPT session is expired. \
                                 Run `codex login` to refresh it, or `vtcode login openai` for a VT Code session."
                                ))
                            } else {
                                // Codex sessions must use the external refresher —
                                // Codex-owned tokens are not rotated by VT Code
                                // (ownership/race-avoidance: rotating them could
                                // race Codex's refresh cycle or invalidate
                                // Codex-maintained credentials).
                                let handle = OpenAIChatGptAuthHandle::new_external(
                                    codex_session,
                                    self.auth_config.auto_refresh,
                                    codex_auth_json_refresher(),
                                );
                                let api_key = handle.current_api_key()?;
                                Ok(OpenAIResolvedAuth::ChatGpt { api_key, handle })
                            }
                        }
                        None => Err(anyhow!("Run vtcode login openai")),
                    },
                }
            }
            OpenAIPreferredMethod::ApiKey => {
                let api_key = require_api_key(api_key)?;
                Ok(OpenAIResolvedAuth::ApiKey { api_key })
            }
            OpenAIPreferredMethod::Auto => {
                let session = load_openai_chatgpt_session_with_mode(self.storage_mode)?;
                if let Some(session) = session {
                    let handle = self.handle_from_session(session);
                    let api_key = handle.current_api_key()?;
                    Ok(OpenAIResolvedAuth::ChatGpt { api_key, handle })
                } else if let Some(codex_session) = self.try_codex_fallback()? {
                    // Skip expired Codex sessions in auto mode — fall through to
                    // API key so the user isn't blocked by stale Codex tokens.
                    if is_session_expired(&codex_session) {
                        tracing::info!("codex auth.json session is expired; falling back to API key");
                        let api_key = require_api_key(api_key)?;
                        Ok(OpenAIResolvedAuth::ApiKey { api_key })
                    } else {
                        let handle = OpenAIChatGptAuthHandle::new_external(
                            codex_session,
                            self.auth_config.auto_refresh,
                            codex_auth_json_refresher(),
                        );
                        let api_key = handle.current_api_key()?;
                        Ok(OpenAIResolvedAuth::ChatGpt { api_key, handle })
                    }
                } else {
                    let api_key = require_api_key(api_key)?;
                    Ok(OpenAIResolvedAuth::ApiKey { api_key })
                }
            }
        }
    }

    /// Build a stored-session auth handle from a VT Code-managed session.
    fn handle_from_session(&self, session: OpenAIChatGptSession) -> OpenAIChatGptAuthHandle {
        OpenAIChatGptAuthHandle::new(session, self.auth_config.clone(), self.storage_mode)
    }

    /// Try to load a ChatGPT session from Codex's `~/.codex/auth.json` as a
    /// fallback when VT Code has no stored session of its own.
    ///
    /// Errors are logged and swallowed so a malformed Codex auth file never
    /// breaks the normal auth resolution path.
    fn try_codex_fallback(&self) -> Result<Option<OpenAIChatGptSession>> {
        match try_load_codex_chatgpt_session() {
            Ok(session) => Ok(session),
            Err(err) => {
                tracing::warn!("failed to load codex auth.json fallback: {err}");
                Ok(None)
            }
        }
    }

    /// Resolve a non-persistent OpenAI auth session backed by externally managed tokens.
    pub fn resolve_external_session_auth(
        &self,
        session: OpenAIChatGptSession,
        refresher: Arc<dyn OpenAIChatGptSessionRefresher>,
    ) -> Result<OpenAIResolvedAuth> {
        let handle = OpenAIChatGptAuthHandle::new_external(session, self.auth_config.auto_refresh, refresher);
        let api_key = handle.current_api_key()?;
        Ok(OpenAIResolvedAuth::ChatGpt { api_key, handle })
    }

    /// Summarize the available OpenAI credentials without mutating storage.
    pub(crate) fn summarize_credentials(&self, api_key: Option<String>) -> Result<OpenAICredentialOverview> {
        let vtcode_session = load_openai_chatgpt_session_with_mode(self.storage_mode)?;
        // Try Codex fallback independently so we can report its availability
        // even when a VT Code-managed session exists (for status display purposes).
        // Expired Codex sessions are not reported as available — they would
        // produce 401s at runtime.
        let codex_session = self.try_codex_fallback()?.filter(|session| !is_session_expired(session));
        let codex_fallback_available = codex_session.is_some();

        // Prefer the VT Code-managed session; fall back to Codex.
        let (chatgpt_session, chatgpt_session_provenance) = match vtcode_session {
            Some(session) => (Some(session), Some(OpenAIChatGptSessionProvenance::Native)),
            None => match codex_session {
                Some(session) => (Some(session), Some(OpenAIChatGptSessionProvenance::CodexFallback)),
                None => (None, None),
            },
        };

        // Extract redacted metadata for display — never expose the full session.
        let (chatgpt_email, chatgpt_plan, chatgpt_session_present) = match &chatgpt_session {
            Some(session) => (session.email.clone(), session.plan.clone(), true),
            None => (None, None, false),
        };

        let api_key_available = api_key.as_ref().is_some_and(|value| !value.trim().is_empty());
        let active_source = match self.auth_config.preferred_method {
            OpenAIPreferredMethod::Chatgpt => chatgpt_session_present.then_some(OpenAIResolvedAuthSource::ChatGpt),
            OpenAIPreferredMethod::ApiKey => api_key_available.then_some(OpenAIResolvedAuthSource::ApiKey),
            OpenAIPreferredMethod::Auto => {
                if chatgpt_session_present {
                    Some(OpenAIResolvedAuthSource::ChatGpt)
                } else if api_key_available {
                    Some(OpenAIResolvedAuthSource::ApiKey)
                } else {
                    None
                }
            }
        };

        let (notice, recommendation) = if api_key_available && chatgpt_session_present {
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
                None => "Next step: choose a single preferred source or set [auth.openai].preferred_method explicitly.",
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
            chatgpt_email,
            chatgpt_plan,
            chatgpt_session_present,
            chatgpt_session_provenance,
            codex_fallback_available,
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
