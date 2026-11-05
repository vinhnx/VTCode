use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use vtcode_commons::VtCodePaths;
use vtcode_exec_events::{ThreadEvent, ThreadItemDetails};

use super::{EvictionSummaryHook, now_rfc3339};
use crate::error::SessionStoreError;

const MAX_EVICTION_GROUNDED_FACTS: usize = 32;
const MAX_EVICTION_GROUNDED_FACT_BYTES: usize = 512;

#[derive(Debug, Serialize)]
struct EvictionSummary {
    session_id: String,
    evicted_event_count: usize,
    event_types: BTreeMap<String, u64>,
    grounded_facts: Vec<String>,
    created_at: String,
}

/// Extract a bounded, deterministic set of facts from canonical event
/// payloads. This is intentionally structural rather than model-generated:
/// eviction must remain synchronous, reproducible, and safe when the model is
/// unavailable. Only completed item snapshots and terminal thread errors are
/// considered, so streaming deltas and raw tool output cannot flood the
/// derived summary.
fn extract_grounded_facts(events: &[ThreadEvent]) -> Vec<String> {
    let mut facts = Vec::new();
    for event in events {
        let candidates: Vec<String> = match event {
            ThreadEvent::ItemCompleted(completed) => item_facts(&completed.item.details),
            ThreadEvent::TurnFailed(failed) => vec![failed.message.clone()],
            ThreadEvent::TurnBlocked(blocked) => vec![blocked.message.clone()],
            ThreadEvent::Error(error) => vec![error.message.clone()],
            _ => Vec::new(),
        };
        for candidate in candidates {
            let fact = normalize_eviction_fact(&candidate);
            if fact.is_empty() || facts.iter().any(|existing| existing == &fact) {
                continue;
            }
            facts.push(fact);
            if facts.len() == MAX_EVICTION_GROUNDED_FACTS {
                return facts;
            }
        }
    }
    facts
}

fn item_facts(details: &ThreadItemDetails) -> Vec<String> {
    match details {
        ThreadItemDetails::AgentMessage(item) => vec![item.text.clone()],
        ThreadItemDetails::Plan(item) => vec![item.text.clone()],
        ThreadItemDetails::FileChange(item) => item
            .changes
            .iter()
            .map(|change| {
                let kind = match change.kind {
                    vtcode_exec_events::PatchChangeKind::Add => "add",
                    vtcode_exec_events::PatchChangeKind::Delete => "delete",
                    vtcode_exec_events::PatchChangeKind::Update => "update",
                };
                format!("file {kind}: {}", change.path)
            })
            .collect(),
        ThreadItemDetails::Harness(item) => item.message.clone().into_iter().collect(),
        _ => Vec::new(),
    }
}

fn normalize_eviction_fact(value: &str) -> String {
    let normalized = vtcode_commons::formatting::collapse_whitespace(value);
    if normalized.len() <= MAX_EVICTION_GROUNDED_FACT_BYTES {
        return normalized;
    }
    let end = normalized.floor_char_boundary(MAX_EVICTION_GROUNDED_FACT_BYTES - '…'.len_utf8());
    format!("{}…", &normalized[..end])
}

pub(super) fn default_eviction_summary_hook(derived_dir: PathBuf, session_id: String) -> EvictionSummaryHook {
    Arc::new(move |events| {
        let mut event_types = BTreeMap::new();
        for event in events {
            let kind = serde_json::to_value(event)
                .ok()
                .and_then(|value| value.get("type").and_then(|value| value.as_str()).map(str::to_owned))
                .unwrap_or_else(|| "unknown".to_owned());
            *event_types.entry(kind).or_insert(0) += 1;
        }
        let summary = EvictionSummary {
            session_id: session_id.clone(),
            evicted_event_count: events.len(),
            event_types,
            grounded_facts: extract_grounded_facts(events),
            created_at: now_rfc3339(),
        };
        let path = derived_dir.join(format!("eviction-summary-{}.json", uuid::Uuid::new_v4().simple()));
        let bytes = serde_json::to_vec(&summary)?;
        VtCodePaths::write_private_file_atomic(&path, &bytes)
            .map_err(|error| SessionStoreError::io(path, std::io::Error::other(error)))
    })
}
