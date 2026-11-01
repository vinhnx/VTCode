//! Cross-process peer-session messaging executors (`ask_session`, `answer_session`, `list_sessions`).
//!
//! Thin bridge over the [`vtcode_memory::mailbox`] file-mailbox contract: the
//! executor parses tool args, resolves the calling session from the harness
//! context, and shapes mailbox results as tool JSON. Delivery into the peer's
//! next turn happens in the runloop turn-prep drain, not here.

use super::ToolRegistry;
use anyhow::{Context, Result, anyhow, bail};
use futures::future::BoxFuture;
use serde_json::{Value, json};

/// Extract a required non-empty trimmed string arg.
fn required_text(args: &Value, field: &str, what: &str) -> Result<String> {
    args.get(field)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .ok_or_else(|| anyhow!("{what}"))
}

impl ToolRegistry {
    pub(crate) fn ask_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let session =
                required_text(&args, "session", "ask_session requires a target session id (see list_sessions)")?;
            let question = required_text(&args, "question", "ask_session requires a non-empty question")?;
            let ttl_secs = args.get("ttl_secs").and_then(Value::as_i64);
            let from_session = self.harness_context.session_id();
            if session == from_session {
                bail!("ask_session cannot target the calling session itself");
            }
            let mail = vtcode_memory::NewMail::question(
                &session,
                vec![vtcode_memory::MailQuestion { id: "q1".to_owned(), question, options: Vec::new() }],
                "",
                ttl_secs,
            )
            .context("ask_session: invalid message")?;
            let receipt = vtcode_memory::send_mail(self.workspace_root(), &from_session, &mail)
                .context("ask_session: delivery failed")?;
            Ok(json!({
                "message_id": receipt.message_id,
                "to_session": session,
            }))
        })
    }

    pub(crate) fn answer_session_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let message_id = required_text(&args, "message_id", "answer_session requires the question message_id")?;
            let answer = required_text(&args, "answer", "answer_session requires a non-empty answer")?;
            let ttl_secs = args.get("ttl_secs").and_then(Value::as_i64);
            let own_session = self.harness_context.session_id();
            // The question lives in our inbox, or in the read archive when an
            // earlier turn already drained it; its sender is the reply target.
            let question = vtcode_memory::find_mail(self.workspace_root(), &own_session, &message_id)
                .context("answer_session: inbox read failed")?
                .ok_or_else(|| {
                    anyhow!("unknown question {message_id:?}; it may have expired or been answered already")
                })?;
            let mail = vtcode_memory::NewMail::answer(&question.from_session, &message_id, &answer, ttl_secs)
                .context("answer_session: invalid message")?;
            let receipt = vtcode_memory::send_mail(self.workspace_root(), &own_session, &mail)
                .context("answer_session: delivery failed")?;
            Ok(json!({
                "message_id": receipt.message_id,
                "to_session": question.from_session,
                "in_reply_to": message_id,
            }))
        })
    }

    pub(crate) fn list_sessions_executor(&self, args: Value) -> BoxFuture<'_, Result<Value>> {
        Box::pin(async move {
            let include_self = args.get("include_self").and_then(Value::as_bool).unwrap_or(false);
            let own_session = self.harness_context.session_id();
            let sessions: Vec<vtcode_memory::SessionSummary> = vtcode_memory::live_sessions(self.workspace_root())
                .into_iter()
                .filter(|summary| include_self || summary.session_id != own_session)
                .collect();
            Ok(json!({ "sessions": sessions }))
        })
    }
}
