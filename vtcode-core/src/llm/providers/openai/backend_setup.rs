//! Backend/auth setup boundary for OpenAI-compatible request construction.
//!
//! Rig 0.38 exposes a ChatGPT subscription provider with access-token auth,
//! account headers, and the Codex backend URL. VT Code still owns stored
//! session refresh and the Responses request/stream pipeline in this slice, so
//! the default ChatGPT setup uses Rig's public auth primitive while retaining
//! VT Code's session snapshot/refresh boundary here.

use crate::config::constants::{env_vars, urls};
use crate::llm::providers::common::override_base_url;
use reqwest::RequestBuilder;
use rig::providers::chatgpt::ChatGPTAuth as RigChatGptAuth;
use std::env;
use std::sync::Arc;
use vtcode_config::auth::OpenAIChatGptSession;

pub(crate) const CHATGPT_CODEX_BASE: &str = "https://chatgpt.com/backend-api/codex";

const CHATGPT_ACCOUNT_HEADER: &str = "ChatGPT-Account-Id";
const CHATGPT_ORIGINATOR_HEADER: &str = "originator";
const CHATGPT_ORIGINATOR_VALUE: &str = "codex_cli_rs";
const CHATGPT_SESSION_HEADER: &str = "session_id";
const CHATGPT_USER_AGENT: &str = "VT Code/1.0";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum OpenAIBackendKind {
    ApiKey,
    ChatGptSubscription(ChatGptSubscriptionAuthSource),
    CustomCommand,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ChatGptSubscriptionAuthSource {
    /// Default ChatGPT subscription path. VT Code snapshots/refreshed stored
    /// sessions, then hands the access token/account pair to Rig's public
    /// ChatGPT auth primitive for request authorisation.
    RigChatGpt,
    /// Temporary bridge for VT Code's existing Codex app-server-derived request
    /// auth shape. Kept only for an explicit fallback while the remaining
    /// Responses request/stream parity is moved behind Rig in later slices.
    #[allow(dead_code)]
    CodexAppServerCompatibility,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OpenAIBackendRefreshBehaviour {
    StaticBearer,
    RefreshableCommandToken,
    RefreshableChatGptSession,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OpenAIBackendTransportCapabilities {
    /// VTCode transport gate retained until Rig transport parity is proven.
    /// API-key/custom endpoints may use the custom Responses WebSocket path;
    /// ChatGPT Codex keeps SSE only because the custom stream path preserves
    /// `store=false` and encrypted reasoning includes. Protected by
    /// `provider_from_config_respects_prompt_cache_and_websocket_gating` and
    /// the API-key/ChatGPT streaming metadata tests. Remove once Rig can expose
    /// matching per-backend transport capabilities.
    pub websocket: bool,
    pub chat_completions_fallback: bool,
    pub responses_compaction_endpoint: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct OpenAIBackendResponsesDefaults {
    pub force_store_false: bool,
    pub include_output_types: bool,
    pub include_sampling_parameters: bool,
    pub include_prompt_cache_retention: bool,
    pub include_encrypted_reasoning: bool,
    pub include_structured_history_in_input: bool,
    pub preserve_structured_history_on_replay: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct OpenAIBackendSetup {
    kind: OpenAIBackendKind,
    base_url: Arc<str>,
    refresh_behaviour: OpenAIBackendRefreshBehaviour,
    transport: OpenAIBackendTransportCapabilities,
    responses_defaults: OpenAIBackendResponsesDefaults,
}

#[derive(Clone, Debug)]
pub(crate) struct OpenAIRequestAuth {
    pub bearer_token: String,
    pub chatgpt_account_id: Option<String>,
    pub rig_chatgpt_auth: Option<RigChatGptAuth>,
}

impl OpenAIRequestAuth {
    pub(crate) fn bearer_token(bearer_token: String) -> Self {
        Self {
            bearer_token,
            chatgpt_account_id: None,
            rig_chatgpt_auth: None,
        }
    }

    fn from_rig_chatgpt_auth(auth: RigChatGptAuth) -> Self {
        let RigChatGptAuth::AccessToken {
            access_token,
            account_id,
        } = auth.clone()
        else {
            return Self::bearer_token(String::new());
        };

        Self {
            bearer_token: access_token,
            chatgpt_account_id: account_id,
            rig_chatgpt_auth: Some(auth),
        }
    }

    fn chatgpt_bearer_and_account(&self) -> (&str, Option<&str>) {
        if let Some(RigChatGptAuth::AccessToken {
            access_token,
            account_id,
        }) = &self.rig_chatgpt_auth
        {
            return (access_token, account_id.as_deref());
        }

        (&self.bearer_token, self.chatgpt_account_id.as_deref())
    }
}

impl OpenAIBackendSetup {
    pub(crate) fn from_api_key_config(base_url: Option<String>) -> Self {
        let base_url = override_base_url(
            urls::OPENAI_API_BASE,
            base_url,
            Some(env_vars::OPENAI_BASE_URL),
        );
        Self::api_key(base_url)
    }

    pub(crate) fn from_chatgpt_subscription_config(base_url: Option<String>) -> Self {
        let base_url = override_base_url(
            CHATGPT_CODEX_BASE,
            base_url,
            Some(env_vars::OPENAI_BASE_URL),
        );
        Self::chatgpt_subscription_rig(base_url)
    }

    pub(crate) fn api_key(base_url: String) -> Self {
        Self::new(
            OpenAIBackendKind::ApiKey,
            base_url,
            OpenAIBackendRefreshBehaviour::StaticBearer,
        )
    }

    pub(crate) fn chatgpt_subscription_rig(base_url: String) -> Self {
        Self::new(
            OpenAIBackendKind::ChatGptSubscription(ChatGptSubscriptionAuthSource::RigChatGpt),
            base_url,
            OpenAIBackendRefreshBehaviour::RefreshableChatGptSession,
        )
    }

    #[allow(dead_code)]
    pub(crate) fn chatgpt_subscription_compatibility(base_url: String) -> Self {
        Self::new(
            OpenAIBackendKind::ChatGptSubscription(
                ChatGptSubscriptionAuthSource::CodexAppServerCompatibility,
            ),
            base_url,
            OpenAIBackendRefreshBehaviour::RefreshableChatGptSession,
        )
    }

    pub(crate) fn with_custom_command_auth(mut self) -> Self {
        self.kind = OpenAIBackendKind::CustomCommand;
        self.refresh_behaviour = OpenAIBackendRefreshBehaviour::RefreshableCommandToken;
        self
    }

    fn new(
        kind: OpenAIBackendKind,
        base_url: String,
        refresh_behaviour: OpenAIBackendRefreshBehaviour,
    ) -> Self {
        let is_chatgpt_codex_backend = matches!(kind, OpenAIBackendKind::ChatGptSubscription(_))
            && base_url.contains("chatgpt.com");
        Self {
            kind,
            base_url: Arc::from(base_url.as_str()),
            refresh_behaviour,
            transport: OpenAIBackendTransportCapabilities {
                websocket: !is_chatgpt_codex_backend,
                chat_completions_fallback: !is_chatgpt_codex_backend,
                responses_compaction_endpoint: !is_chatgpt_codex_backend,
            },
            responses_defaults: OpenAIBackendResponsesDefaults {
                force_store_false: is_chatgpt_codex_backend,
                include_output_types: !is_chatgpt_codex_backend,
                include_sampling_parameters: !is_chatgpt_codex_backend,
                include_prompt_cache_retention: !is_chatgpt_codex_backend,
                include_encrypted_reasoning: is_chatgpt_codex_backend,
                include_structured_history_in_input: !is_chatgpt_codex_backend,
                preserve_structured_history_on_replay: is_chatgpt_codex_backend,
            },
        }
    }

    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    pub(crate) fn kind(&self) -> &OpenAIBackendKind {
        &self.kind
    }

    pub(crate) fn refresh_behaviour(&self) -> OpenAIBackendRefreshBehaviour {
        self.refresh_behaviour
    }

    pub(crate) fn transport(&self) -> OpenAIBackendTransportCapabilities {
        self.transport
    }

    pub(crate) fn responses_defaults(&self) -> OpenAIBackendResponsesDefaults {
        self.responses_defaults
    }

    pub(crate) fn is_native_openai_api(&self) -> bool {
        matches!(self.kind(), OpenAIBackendKind::ApiKey) && self.base_url.contains("api.openai.com")
    }

    pub(crate) fn uses_chatgpt_subscription_auth(&self) -> bool {
        matches!(self.kind(), OpenAIBackendKind::ChatGptSubscription(_))
    }

    pub(crate) fn is_chatgpt_codex_backend(&self) -> bool {
        self.uses_chatgpt_subscription_auth() && self.base_url.contains("chatgpt.com")
    }

    pub(crate) fn uses_refreshable_auth(&self) -> bool {
        !matches!(
            self.refresh_behaviour(),
            OpenAIBackendRefreshBehaviour::StaticBearer
        )
    }

    pub(crate) fn request_auth_from_session(
        &self,
        session: OpenAIChatGptSession,
    ) -> OpenAIRequestAuth {
        if matches!(
            self.kind(),
            OpenAIBackendKind::ChatGptSubscription(ChatGptSubscriptionAuthSource::RigChatGpt)
        ) {
            return OpenAIRequestAuth::from_rig_chatgpt_auth(RigChatGptAuth::AccessToken {
                access_token: session.access_token,
                account_id: session.account_id,
            });
        }

        let bearer_token =
            if self.is_chatgpt_codex_backend() || session.openai_api_key.trim().is_empty() {
                session.access_token
            } else {
                session.openai_api_key
            };

        OpenAIRequestAuth {
            bearer_token,
            chatgpt_account_id: session.account_id,
            rig_chatgpt_auth: None,
        }
    }

    pub(crate) fn authorize_request(
        &self,
        builder: RequestBuilder,
        auth: &OpenAIRequestAuth,
    ) -> RequestBuilder {
        let (bearer_token, chatgpt_account_id) = if self.is_chatgpt_codex_backend() {
            auth.chatgpt_bearer_and_account()
        } else {
            (
                auth.bearer_token.as_str(),
                auth.chatgpt_account_id.as_deref(),
            )
        };

        let mut builder = if bearer_token.trim().is_empty() {
            builder
        } else {
            builder.bearer_auth(bearer_token)
        };

        if self.is_chatgpt_codex_backend() {
            if let Some(account_id) = chatgpt_account_id
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                builder = builder.header(CHATGPT_ACCOUNT_HEADER, account_id);
            }
            builder = builder
                .header(CHATGPT_ORIGINATOR_HEADER, CHATGPT_ORIGINATOR_VALUE)
                .header("User-Agent", CHATGPT_USER_AGENT);
            if let Ok(session_id) = env::var("VT_SESSION_ID")
                && !session_id.trim().is_empty()
            {
                builder = builder.header(CHATGPT_SESSION_HEADER, session_id);
            }
        }

        builder
    }
}
