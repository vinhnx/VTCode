//! Deterministic, evidence-backed execution views of canonical retained events.
//! No model calls or additional explanation database are involved.
mod projection;
mod render;
#[cfg(test)]
mod tests;

use crate::{SessionEventLog, SessionStoreError};
pub use render::{render_details, render_diagram, render_html, render_html_with_workspace, render_summary};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use vtcode_exec_events::{ThreadEvent, TokenBreakdown, Usage, VersionedThreadEvent};

/// Latest recorded task, or all retained tasks in the session.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExplanationScope {
    /// Latest recorded task.
    #[default]
    Task,
    /// All retained tasks in the session.
    Session,
}

/// Content-addressed reference: rewrites cannot retarget it to unrelated bytes.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub struct EvidenceRef {
    /// Canonical session identity.
    pub session_id: String,
    /// Byte offset in the retained event file or evidence page.
    pub offset: u64,
    /// Length of the canonical event record in bytes.
    pub length: u64,
    /// SHA-256 of the complete event record.
    pub digest: String,
    /// Stable item identity, when the event contains an item.
    pub item_id: Option<String>,
}

/// One bounded public fact with its canonical source.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplanationEntry {
    /// Bounded, redacted public description.
    pub label: String,
    /// Recorded lifecycle result; unavailable facts remain explicit.
    pub status: String,
    /// Canonical source of this fact.
    pub evidence: EvidenceRef,
    /// Recorded root task identity, if available.
    pub task_id: Option<String>,
    /// Recorded actor identity, if available.
    pub actor_id: Option<String>,
    /// Recorded parent actor identity, if available.
    pub parent_actor_id: Option<String>,
    /// Recorded RFC3339 time, if available.
    pub timestamp: Option<String>,
    /// Captured changed path, if available.
    pub path: Option<String>,
    /// Captured new-side hunk location; deletions use the diff.
    pub line: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Evidence-backed VerificationEntry data for shared renderers.
pub struct VerificationEntry {
    /// Public fact and its evidence.
    pub fact: ExplanationEntry,
    /// Recorded process exit code; absent is unconfirmed.
    pub exit_code: Option<i32>,
    /// Successful check's earliest recorded lifecycle follows all recorded mutations.
    pub fresh: bool,
}

/// Recorded request-prefix token attribution with canonical turn evidence.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenBreakdownEntry {
    /// Turn context and evidence for the recorded request prefix.
    pub fact: ExplanationEntry,
    /// Producer-recorded counts; never estimated by this projection.
    pub breakdown: TokenBreakdown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Evidence-backed DecisionEntry data for shared renderers.
pub struct DecisionEntry {
    /// Public fact and its evidence.
    pub fact: ExplanationEntry,
    /// Agent-reported public rationale, never reconstructed from reasoning.
    pub rationale: String,
    /// Agent-reported rejected alternatives.
    pub alternatives: Vec<String>,
    /// Current-task item references reported by the agent.
    pub evidence_ids: Vec<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
/// Evidence-backed review ordering; does not assert a bug.
pub enum ReviewPriority {
    /// Security, persistence, schema, or dependency boundary.
    High,
    /// Compatibility, change size, recovery, or verification signal.
    Medium,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Evidence-backed ReviewSignal data for shared renderers.
pub struct ReviewSignal {
    /// Deterministic review order, not a bug severity.
    pub priority: ReviewPriority,
    /// Concrete review signal supported by the fact.
    pub reason: String,
    /// Public fact and its evidence.
    pub fact: ExplanationEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
/// Evidence-backed GraphEdge data for shared renderers.
pub struct GraphEdge {
    /// Content-addressed source event identity with task and actor context.
    pub from: String,
    /// Content-addressed target event identity or file path.
    pub to: String,
    /// Recorded relationship label.
    pub relation: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
/// Evidence-backed EvidenceCompleteness data for shared renderers.
pub struct EvidenceCompleteness {
    /// Retained records that could not be decoded.
    pub malformed_records: usize,
    /// Unsupported canonical event types.
    pub unknown_records: usize,
    /// Events missing task context.
    pub legacy_records: usize,
    /// Turns preceding the retained range.
    pub evicted_turns: u64,
    /// Explicit historical gaps and unavailable information.
    pub warnings: Vec<String>,
}

/// Shared facts consumed by terminal, diagrams, browser, and offline report.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplanationModel {
    /// Canonical session identity.
    pub session_id: String,
    /// Digest of the complete retained snapshot.
    pub revision: String,
    /// Requested task or session scope.
    pub scope: ExplanationScope,
    /// Recorded root task identity, if available.
    pub task_id: Option<String>,
    /// Recorded lifecycle result; unavailable facts remain explicit.
    pub status: String,
    /// Original recorded task requests.
    pub goals: Vec<ExplanationEntry>,
    /// Lifecycle-reduced actions in recorded order.
    pub actions: Vec<ExplanationEntry>,
    /// Distinct paths with successful recorded changes.
    pub changes: Vec<ExplanationEntry>,
    /// Number of distinct successful file-change operations.
    pub edit_operations: usize,
    /// Explicit public decisions, labeled agent-reported.
    pub decisions: Vec<DecisionEntry>,
    /// Verification commands with honest exit status and freshness.
    pub verification: Vec<VerificationEntry>,
    /// Recorded request-prefix token attribution for each retained turn.
    pub token_breakdowns: Vec<TokenBreakdownEntry>,
    /// Failed, denied, blocked, and unconfirmed operations.
    pub failures: Vec<ExplanationEntry>,
    /// Sorted evidence-backed review signals.
    pub review_priorities: Vec<ReviewSignal>,
    /// Recorded plans, corrections, and approvals.
    pub plan_evolution: Vec<ExplanationEntry>,
    /// Recorded order and action-to-file edges.
    pub graph: Vec<GraphEdge>,
    /// Canonical token usage, only when recorded.
    pub usage: Option<Usage>,
    /// Recorded total session cost, absent for task scope.
    pub cost_usd: Option<serde_json::Number>,
    /// Visible uncertainty about retained evidence.
    pub completeness: EvidenceCompleteness,
}

/// Paged evidence, bounded independently of the projection size.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidencePage {
    /// Canonical content-addressed source.
    pub reference: EvidenceRef,
    /// Redacted event evidence for this page.
    pub text: String,
    /// Byte offset in the retained event file or evidence page.
    pub offset: usize,
    /// Next UTF-8 page boundary, or None at EOF.
    pub next_offset: Option<usize>,
    /// Total redacted evidence length.
    pub total_bytes: usize,
}

/// Bounded page of each projected collection, with one authoritative revision.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExplanationPage {
    /// Current Git state, independent of canonical execution and ownership.
    /// Runtime adapters may attach it on page zero; canonical queries leave it absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace_diff: Option<WorkspaceDiffSnapshot>,
    /// Projected facts for this page; scalar metadata is repeated unchanged.
    pub model: ExplanationModel,
    /// Start index applied to each collection.
    pub offset: usize,
    /// Next index, or None when every collection is exhausted.
    pub next_offset: Option<usize>,
}

/// Bounded current workspace state; never evidence of agent attribution.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceDiffSnapshot {
    /// Runtime capture time, independent of historical event timestamps.
    pub captured_at: String,
    /// Redacted tracked-file diff against HEAD, absent when unavailable.
    pub text: Option<String>,
    /// Whether the bounded snapshot omits diff content.
    pub truncated: bool,
    /// Scope and availability explanation; never a claim of agent ownership.
    pub note: String,
}

/// Page large projections within bridge byte limits.
pub fn page_explanation(model: &ExplanationModel, offset: usize) -> ExplanationPage {
    fn page<T: Clone>(items: &[T], offset: usize, count: usize) -> Vec<T> {
        items.iter().skip(offset).take(count).cloned().collect()
    }
    let maximum = [
        model.goals.len(),
        model.actions.len(),
        model.changes.len(),
        model.decisions.len(),
        model.verification.len(),
        model.token_breakdowns.len(),
        model.failures.len(),
        model.review_priorities.len(),
        model.plan_evolution.len(),
        model.graph.len(),
    ]
    .into_iter()
    .max()
    .unwrap_or(0);
    let mut count = 32;
    loop {
        let result = ExplanationPage {
            workspace_diff: None,
            model: ExplanationModel {
                session_id: model.session_id.clone(),
                revision: model.revision.clone(),
                scope: model.scope,
                task_id: model.task_id.clone(),
                status: model.status.clone(),
                goals: page(&model.goals, offset, count),
                actions: page(&model.actions, offset, count),
                changes: page(&model.changes, offset, count),
                edit_operations: model.edit_operations,
                decisions: page(&model.decisions, offset, count),
                verification: page(&model.verification, offset, count),
                token_breakdowns: page(&model.token_breakdowns, offset, count),
                failures: page(&model.failures, offset, count),
                review_priorities: page(&model.review_priorities, offset, count),
                plan_evolution: page(&model.plan_evolution, offset, count),
                graph: page(&model.graph, offset, count),
                usage: model.usage.clone(),
                cost_usd: model.cost_usd.clone(),
                completeness: model.completeness.clone(),
            },
            offset,
            next_offset: (offset.saturating_add(count) < maximum).then_some(offset.saturating_add(count)),
        };
        if count == 1 || serde_json::to_vec(&result).is_ok_and(|bytes| bytes.len() <= 64 * 1024) {
            return result;
        }
        count /= 2;
    }
}

impl ExplanationModel {
    /// Unique substantive sources in this projection, sorted by recorded order.
    pub fn evidence_references(&self) -> Vec<EvidenceRef> {
        let mut refs: Vec<_> = self
            .goals
            .iter()
            .chain(&self.actions)
            .chain(&self.changes)
            .chain(&self.failures)
            .chain(&self.plan_evolution)
            .chain(self.decisions.iter().map(|d| &d.fact))
            .chain(self.verification.iter().map(|v| &v.fact))
            .chain(self.token_breakdowns.iter().map(|entry| &entry.fact))
            .chain(self.review_priorities.iter().map(|r| &r.fact))
            .map(|e| e.evidence.clone())
            .collect();
        refs.sort();
        refs.dedup();
        refs
    }
}

/// Query one ordered retained snapshot. Full payloads are discarded as scanned.
pub fn query_explanation(
    log: &SessionEventLog,
    scope: ExplanationScope,
) -> Result<ExplanationModel, SessionStoreError> {
    let session_id = log.manifest().session_id;
    let mut reducer = projection::Reducer::new(session_id.clone(), scope);
    let mut revision = Sha256::new();
    let manifest = log.visit_snapshot(|offset, bytes| {
        revision.update(bytes);
        let reference = EvidenceRef {
            session_id: session_id.clone(),
            offset,
            length: bytes.len() as u64,
            digest: hex(&Sha256::digest(bytes)),
            item_id: None,
        };
        match serde_json::from_slice::<VersionedThreadEvent>(bytes) {
            Ok(v) => reducer.push(v.into_event(), reference),
            Err(_) => reducer.malformed(),
        }
    })?;
    let evicted_turns = manifest.evicted_turn_count();
    Ok(reducer.finish(hex(&revision.finalize()), evicted_turns))
}

/// Resolve evidence only if its session, offset, length, and digest still match.
/// An expired/evicted reference is an error, never an empty successful result.
pub fn query_evidence(
    log: &SessionEventLog,
    reference: &EvidenceRef,
    offset: usize,
    limit: usize,
) -> Result<EvidencePage, SessionStoreError> {
    if reference.session_id != log.manifest().session_id {
        return Err(evidence_error("evidence belongs to another session"));
    }
    let mut found = None;
    log.visit_snapshot(|position, bytes| {
        if position == reference.offset
            && bytes.len() as u64 == reference.length
            && hex(&Sha256::digest(bytes)) == reference.digest
        {
            // Parse then redact individual values so JSON remains valid.
            if let Ok(mut value) = serde_json::from_slice::<serde_json::Value>(bytes) {
                redact_value(&mut value);
                found = serde_json::to_string_pretty(&value).ok();
            }
        }
    })?;
    let text = found.ok_or_else(|| evidence_error("evidence is unavailable, malformed, or expired"))?;
    if offset > text.len() || !text.is_char_boundary(offset) {
        return Err(evidence_error("invalid evidence page offset"));
    }
    let mut end = offset.saturating_add(limit.clamp(1, 32 * 1024)).min(text.len());
    while !text.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    // Always progress, including a page limit smaller than the next UTF-8 scalar.
    if end == offset && offset < text.len() {
        end += text[offset..].chars().next().map_or(0, char::len_utf8);
    }
    Ok(EvidencePage {
        reference: reference.clone(),
        text: text[offset..end].to_owned(),
        offset,
        next_offset: (end < text.len()).then_some(end),
        total_bytes: text.len(),
    })
}

fn evidence_error(message: &str) -> SessionStoreError {
    SessionStoreError::io("events.jsonl", std::io::Error::new(std::io::ErrorKind::InvalidData, message))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn public_text(text: &str, limit: usize) -> String {
    let clean: String = text.chars().filter(|c| !c.is_control() || *c == '\n' || *c == '\t').collect();
    let redacted = vtcode_commons::sanitizer::redact_secrets(clean);
    let mut end = redacted.len().min(limit);
    while !redacted.is_char_boundary(end) {
        end -= 1;
    }
    let mut result = redacted[..end].to_owned();
    if redacted.len() > limit {
        result.push_str(" [truncated; inspect evidence]");
    }
    result
}

fn public_identity(text: &str) -> String {
    if text
        .strip_prefix("task-")
        .or_else(|| text.strip_prefix("turn-"))
        .is_some_and(|id| id.len() == 36 && uuid::Uuid::parse_str(id).is_ok())
    {
        return text.to_owned();
    }
    let redacted = public_text(text, 256);
    if redacted == text {
        redacted
    } else {
        format!("redacted-id-{}", hex(&Sha256::digest(text.as_bytes())))
    }
}

fn public_path(text: &str) -> String {
    let label = public_text(text, 900);
    if label == text {
        label
    } else {
        format!("{label} [path-{}]", hex(&Sha256::digest(text.as_bytes())))
    }
}

fn redact_value(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::String(s) => *s = public_text(s, usize::MAX),
        serde_json::Value::Array(a) => {
            for value in a {
                redact_value(value);
            }
        }
        serde_json::Value::Object(o) => {
            for (key, v) in o {
                if matches!(
                    key.to_ascii_lowercase().replace(['_', '-'], "").as_str(),
                    "token"
                        | "password"
                        | "passwd"
                        | "authorization"
                        | "apikey"
                        | "secret"
                        | "accesstoken"
                        | "refreshtoken"
                        | "clientsecret"
                        | "privatekey"
                        | "credential"
                        | "credentials"
                ) {
                    *v = serde_json::Value::String("[REDACTED]".into());
                } else {
                    redact_value(v);
                }
            }
        }
        _ => {}
    }
}
