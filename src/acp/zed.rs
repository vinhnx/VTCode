use agent_client_protocol as acp;
use agent_client_protocol::Client;
use anyhow::{Context, Result};
use async_trait::async_trait;
use futures::StreamExt;
use serde_json::json;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot};
use tokio_util::compat::{TokioAsyncReadCompatExt, TokioAsyncWriteCompatExt};
use tracing::error;

use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::llm::factory::{create_provider_for_model, create_provider_with_config};
use vtcode_core::llm::provider::{FinishReason, LLMRequest, LLMStreamEvent, Message, ToolChoice};
use vtcode_core::prompts::read_system_prompt_from_md;

const SESSION_PREFIX: &str = "vtcode-zed-session";

#[derive(Clone)]
struct SessionHandle {
    data: Rc<RefCell<SessionData>>,
    cancel_flag: Rc<Cell<bool>>,
}

struct SessionData {
    messages: Vec<Message>,
}

struct NotificationEnvelope {
    notification: acp::SessionNotification,
    completion: oneshot::Sender<()>,
}

pub async fn run_zed_agent(config: &CoreAgentConfig) -> Result<()> {
    let outgoing = tokio::io::stdout().compat_write();
    let incoming = tokio::io::stdin().compat();
    let system_prompt = read_system_prompt_from_md().unwrap_or_else(|_| String::new());

    let local_set = tokio::task::LocalSet::new();
    let config_clone = config.clone();

    local_set
        .run_until(async move {
            let (tx, mut rx) = mpsc::unbounded_channel::<NotificationEnvelope>();
            let agent = ZedAgent::new(config_clone, system_prompt, tx);
            let (conn, io_task) = acp::AgentSideConnection::new(agent, outgoing, incoming, |fut| {
                tokio::task::spawn_local(fut);
            });

            let notifications = tokio::task::spawn_local(async move {
                while let Some(envelope) = rx.recv().await {
                    let result = conn.session_notification(envelope.notification).await;
                    if let Err(error) = result {
                        error!(%error, "Failed to forward ACP session notification");
                    }
                    let _ = envelope.completion.send(());
                }
            });

            let io_result = io_task.await;
            notifications.abort();
            io_result
        })
        .await
        .context("ACP stdio bridge task failed")?;

    Ok(())
}

struct ZedAgent {
    config: CoreAgentConfig,
    system_prompt: String,
    sessions: Rc<RefCell<HashMap<acp::SessionId, SessionHandle>>>,
    next_session_id: Cell<u64>,
    session_update_tx: mpsc::UnboundedSender<NotificationEnvelope>,
}

impl ZedAgent {
    fn new(
        config: CoreAgentConfig,
        system_prompt: String,
        session_update_tx: mpsc::UnboundedSender<NotificationEnvelope>,
    ) -> Self {
        Self {
            config,
            system_prompt,
            sessions: Rc::new(RefCell::new(HashMap::new())),
            next_session_id: Cell::new(0),
            session_update_tx,
        }
    }

    fn register_session(&self) -> acp::SessionId {
        let raw_id = self.next_session_id.get();
        self.next_session_id.set(raw_id + 1);
        let session_id = acp::SessionId(Arc::from(format!("{SESSION_PREFIX}-{raw_id}")));
        let handle = SessionHandle {
            data: Rc::new(RefCell::new(SessionData {
                messages: Vec::new(),
            })),
            cancel_flag: Rc::new(Cell::new(false)),
        };
        self.sessions
            .borrow_mut()
            .insert(session_id.clone(), handle);
        session_id
    }

    fn session_handle(&self, session_id: &acp::SessionId) -> Option<SessionHandle> {
        self.sessions.borrow().get(session_id).cloned()
    }

    fn push_message(&self, session: &SessionHandle, message: Message) {
        session.data.borrow_mut().messages.push(message);
    }

    fn resolved_messages(&self, session: &SessionHandle) -> Vec<Message> {
        let mut messages = Vec::new();
        if !self.system_prompt.trim().is_empty() {
            messages.push(Message::system(self.system_prompt.clone()));
        }

        let history = session.data.borrow();
        messages.extend(history.messages.iter().cloned());
        messages
    }

    fn stop_reason_from_finish(finish: FinishReason) -> acp::StopReason {
        match finish {
            FinishReason::Stop | FinishReason::ToolCalls => acp::StopReason::EndTurn,
            FinishReason::Length => acp::StopReason::MaxTokens,
            FinishReason::ContentFilter | FinishReason::Error(_) => acp::StopReason::Refusal,
        }
    }

    fn join_prompt(prompt: &[acp::ContentBlock]) -> Result<String, acp::Error> {
        let mut aggregated = String::new();

        for block in prompt {
            match block {
                acp::ContentBlock::Text(text) => aggregated.push_str(&text.text),
                acp::ContentBlock::ResourceLink(link) => {
                    if !aggregated.is_empty() {
                        aggregated.push('\n');
                    }
                    aggregated.push_str(&format!("Resource {} ({})", link.name, link.uri));
                }
                acp::ContentBlock::Resource(resource) => match &resource.resource {
                    acp::EmbeddedResourceResource::TextResourceContents(text) => {
                        if !aggregated.is_empty() {
                            aggregated.push('\n');
                        }
                        aggregated.push_str(&text.text);
                    }
                    _ => {
                        return Err(acp::Error::invalid_params()
                            .with_data(json!({ "reason": "unsupported_resource" })));
                    }
                },
                _ => {
                    return Err(acp::Error::invalid_params()
                        .with_data(json!({ "reason": "unsupported_content" })));
                }
            }
        }

        Ok(aggregated)
    }

    async fn send_update(
        &self,
        session_id: &acp::SessionId,
        update: acp::SessionUpdate,
    ) -> Result<(), acp::Error> {
        let (completion, completion_rx) = oneshot::channel();
        let notification = acp::SessionNotification {
            session_id: session_id.clone(),
            update,
            meta: None,
        };

        self.session_update_tx
            .send(NotificationEnvelope {
                notification,
                completion,
            })
            .map_err(|_| acp::Error::internal_error())?;

        completion_rx
            .await
            .map_err(|_| acp::Error::internal_error())
    }
}

#[async_trait(?Send)]
impl acp::Agent for ZedAgent {
    async fn initialize(
        &self,
        _args: acp::InitializeRequest,
    ) -> Result<acp::InitializeResponse, acp::Error> {
        Ok(acp::InitializeResponse {
            protocol_version: acp::V1,
            agent_capabilities: acp::AgentCapabilities::default(),
            auth_methods: Vec::new(),
            meta: None,
        })
    }

    async fn authenticate(
        &self,
        _args: acp::AuthenticateRequest,
    ) -> Result<acp::AuthenticateResponse, acp::Error> {
        Ok(acp::AuthenticateResponse::default())
    }

    async fn new_session(
        &self,
        _args: acp::NewSessionRequest,
    ) -> Result<acp::NewSessionResponse, acp::Error> {
        let session_id = self.register_session();
        Ok(acp::NewSessionResponse {
            session_id,
            modes: None,
            meta: None,
        })
    }

    async fn prompt(&self, args: acp::PromptRequest) -> Result<acp::PromptResponse, acp::Error> {
        let Some(session) = self.session_handle(&args.session_id) else {
            return Err(
                acp::Error::invalid_params().with_data(json!({ "reason": "unknown_session" }))
            );
        };

        session.cancel_flag.set(false);

        let user_message = Self::join_prompt(&args.prompt)?;
        self.push_message(&session, Message::user(user_message.clone()));

        let provider = match create_provider_for_model(
            &self.config.model,
            self.config.api_key.clone(),
            Some(self.config.prompt_cache.clone()),
        ) {
            Ok(provider) => provider,
            Err(_) => create_provider_with_config(
                &self.config.provider,
                Some(self.config.api_key.clone()),
                None,
                Some(self.config.model.clone()),
                Some(self.config.prompt_cache.clone()),
            )
            .map_err(acp::Error::into_internal_error)?,
        };

        let supports_streaming = provider.supports_streaming();
        let reasoning_effort = if provider.supports_reasoning_effort(&self.config.model) {
            Some(self.config.reasoning_effort.as_str().to_string())
        } else {
            None
        };

        let messages = self.resolved_messages(&session);
        let request = LLMRequest {
            messages,
            system_prompt: None,
            tools: None,
            model: self.config.model.clone(),
            max_tokens: None,
            temperature: None,
            stream: supports_streaming,
            tool_choice: Some(ToolChoice::none()),
            parallel_tool_calls: None,
            parallel_tool_config: None,
            reasoning_effort,
        };

        let mut stop_reason = acp::StopReason::EndTurn;
        let mut assistant_message = String::new();

        if supports_streaming {
            let mut stream = provider
                .stream(request)
                .await
                .map_err(acp::Error::into_internal_error)?;

            while let Some(event) = stream.next().await {
                let event = event.map_err(acp::Error::into_internal_error)?;

                if session.cancel_flag.get() {
                    stop_reason = acp::StopReason::Cancelled;
                    break;
                }

                match event {
                    LLMStreamEvent::Token { delta } => {
                        if !delta.is_empty() {
                            assistant_message.push_str(&delta);
                            self.send_update(
                                &args.session_id,
                                acp::SessionUpdate::AgentMessageChunk {
                                    content: delta.into(),
                                },
                            )
                            .await?;
                        }
                    }
                    LLMStreamEvent::Reasoning { delta } => {
                        if !delta.is_empty() {
                            self.send_update(
                                &args.session_id,
                                acp::SessionUpdate::AgentThoughtChunk {
                                    content: delta.into(),
                                },
                            )
                            .await?;
                        }
                    }
                    LLMStreamEvent::Completed { response } => {
                        if let Some(content) = response.content {
                            if assistant_message.is_empty() {
                                assistant_message.push_str(&content);
                                if !content.is_empty() {
                                    self.send_update(
                                        &args.session_id,
                                        acp::SessionUpdate::AgentMessageChunk {
                                            content: content.into(),
                                        },
                                    )
                                    .await?;
                                }
                            }
                        }

                        if let Some(reasoning) = response.reasoning {
                            if !reasoning.is_empty() {
                                self.send_update(
                                    &args.session_id,
                                    acp::SessionUpdate::AgentThoughtChunk {
                                        content: reasoning.into(),
                                    },
                                )
                                .await?;
                            }
                        }

                        stop_reason = Self::stop_reason_from_finish(response.finish_reason);
                        break;
                    }
                }
            }
        } else {
            let response = provider
                .generate(request)
                .await
                .map_err(acp::Error::into_internal_error)?;

            if let Some(content) = response.content.clone() {
                if !content.is_empty() {
                    self.send_update(
                        &args.session_id,
                        acp::SessionUpdate::AgentMessageChunk {
                            content: content.clone().into(),
                        },
                    )
                    .await?;
                }
                assistant_message = content;
            }

            if let Some(reasoning) = response.reasoning {
                if !reasoning.is_empty() {
                    self.send_update(
                        &args.session_id,
                        acp::SessionUpdate::AgentThoughtChunk {
                            content: reasoning.into(),
                        },
                    )
                    .await?;
                }
            }

            stop_reason = Self::stop_reason_from_finish(response.finish_reason);
        }

        if stop_reason != acp::StopReason::Cancelled && !assistant_message.is_empty() {
            self.push_message(&session, Message::assistant(assistant_message));
        }

        Ok(acp::PromptResponse {
            stop_reason,
            meta: None,
        })
    }

    async fn cancel(&self, args: acp::CancelNotification) -> Result<(), acp::Error> {
        if let Some(session) = self.session_handle(&args.session_id) {
            session.cancel_flag.set(true);
        }
        Ok(())
    }
}
