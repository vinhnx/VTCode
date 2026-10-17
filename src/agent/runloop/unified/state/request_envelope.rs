//! Session request-envelope and segment tracking for [`super::SessionStats`].
//!
//! Extracted from `state.rs` so envelope identity, stable-segment reuse,
//! and compaction-parent reuse live beside the pre-existing
//! `preflight_recovery` submodule. No behavior change: Arc-sharing,
//! segment-id sequencing, and boundary-reason classification are verbatim,
//! and the public method surface on `SessionStats` is unchanged.

use std::sync::Arc;

use vtcode_core::core::agent::harness_kernel::hash_value;
use vtcode_core::core::agent::request_envelope::{SegmentBoundaryReason, SessionRequestEnvelope};
use vtcode_core::llm::provider::ToolDefinition;

use super::SessionStats;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RequestEnvelopeIdentity {
    model: String,
    provider: String,
    mode: String,
    /// Full prompt identity used to refresh dynamic suffix bytes in-place.
    /// This is deliberately excluded from the stable segment identity below.
    system_prompt_hash: u64,
    prefix_hash: u64,
    catalog_hash: Option<u64>,
    instruction_digest: u64,
    stable_prompt_hash: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ToolCatalogObservabilityIdentity {
    ordered_wire_tool_names: Vec<String>,
    catalog_tool_count: usize,
    wire_tool_count: usize,
    deferred_tool_count: usize,
    active_loaded_skill_names: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestSegmentTransition {
    pub boundary_reason: SegmentBoundaryReason,
    pub previous_segment_id: Option<String>,
    pub new_segment_id: String,
    pub previous_prefix_hash: Option<String>,
    pub previous_catalog_hash: Option<String>,
}

impl SessionStats {
    pub(crate) fn compaction_parent_context(
        &self,
        provider: &str,
        model: &str,
    ) -> Option<vtcode_core::compaction::CompactionParentContext> {
        let identity = self.request_envelope_identity.as_ref()?;
        if identity.provider != provider || identity.model != model {
            return None;
        }
        let envelope = self.request_envelope.as_ref()?;
        let tools = envelope.ordered_tools();
        Some(vtcode_core::compaction::CompactionParentContext {
            system_prompt: Some(envelope.system_prompt()),
            tools: (!tools.is_empty()).then_some(tools),
        })
    }

    pub(crate) fn record_compaction_boundary(&mut self) {
        self.prompt_cache_health.record_compaction_boundary();
    }

    pub(crate) fn begin_request_segment(&mut self, boundary_reason: SegmentBoundaryReason) -> RequestSegmentTransition {
        if boundary_reason == SegmentBoundaryReason::Compaction {
            self.record_compaction_boundary();
        }
        let previous_segment_id = self.request_envelope.as_ref().map(|envelope| envelope.segment_id().to_string());
        let previous_prefix_hash = self
            .request_envelope
            .as_ref()
            .map(|envelope| format!("{:016x}", envelope.prefix_hash()));
        let previous_catalog_hash = self
            .request_envelope
            .as_ref()
            .and_then(SessionRequestEnvelope::catalog_hash)
            .map(|hash| format!("{hash:016x}"));
        self.request_segment_sequence = self.request_segment_sequence.saturating_add(1);
        let new_segment_id = format!("segment-{:08}", self.request_segment_sequence);
        self.pending_request_segment_id = Some(new_segment_id.clone());
        self.request_envelope = None;
        self.request_envelope_identity = None;
        self.request_envelope_source_tools = None;
        RequestSegmentTransition {
            boundary_reason,
            previous_segment_id,
            new_segment_id,
            previous_prefix_hash,
            previous_catalog_hash,
        }
    }

    #[allow(dead_code, reason = "retained as a Vec-based compatibility wrapper for state tests")]
    pub(crate) fn request_envelope(
        &mut self,
        model: &str,
        provider: &str,
        mode: &str,
        system_prompt: String,
        tools: Vec<ToolDefinition>,
        instruction_digest: u64,
    ) -> SessionRequestEnvelope {
        self.request_envelope_shared(
            model,
            provider,
            mode,
            system_prompt,
            Some(Arc::new(tools)),
            instruction_digest,
            instruction_digest,
        )
    }

    pub(crate) fn request_envelope_shared(
        &mut self,
        model: &str,
        provider: &str,
        mode: &str,
        system_prompt: String,
        tools: Option<Arc<Vec<ToolDefinition>>>,
        instruction_digest: u64,
        stable_prompt_hash: u64,
    ) -> SessionRequestEnvelope {
        let system_prompt_hash = hash_value(&system_prompt);
        let source_matches = match (&self.request_envelope_source_tools, &tools) {
            (None, None) => true,
            (Some(previous), Some(current)) => Arc::ptr_eq(previous, current),
            _ => false,
        };
        let identity_matches_without_catalog = self.request_envelope_identity.as_ref().is_some_and(|identity| {
            identity.model == model
                && identity.provider == provider
                && identity.mode == mode
                && identity.system_prompt_hash == system_prompt_hash
                && identity.instruction_digest == instruction_digest
                && identity.stable_prompt_hash == stable_prompt_hash
        });
        if source_matches
            && identity_matches_without_catalog
            && let Some(envelope) = self.request_envelope.as_ref()
        {
            return envelope.clone();
        }

        let stable_prefix_hash = hash_value(&(instruction_digest, stable_prompt_hash));
        let candidate = SessionRequestEnvelope::with_prefix_hash(
            "candidate",
            system_prompt,
            tools.as_ref().map_or_else(Vec::new, |tools| tools.as_ref().clone()),
            instruction_digest,
            stable_prefix_hash,
        );
        let identity = RequestEnvelopeIdentity {
            model: model.to_string(),
            provider: provider.to_string(),
            mode: mode.to_string(),
            system_prompt_hash,
            prefix_hash: candidate.prefix_hash(),
            catalog_hash: candidate.catalog_hash(),
            instruction_digest,
            stable_prompt_hash,
        };
        if self.request_envelope_identity.as_ref() == Some(&identity)
            && let Some(envelope) = self.request_envelope.as_ref()
        {
            self.request_envelope_source_tools = tools;
            return envelope.clone();
        }

        let same_stable_segment = self.request_envelope_identity.as_ref().is_some_and(|previous| {
            previous.model == identity.model
                && previous.provider == identity.provider
                && previous.mode == identity.mode
                && previous.prefix_hash == identity.prefix_hash
                && previous.catalog_hash == identity.catalog_hash
                && previous.instruction_digest == identity.instruction_digest
                && previous.stable_prompt_hash == identity.stable_prompt_hash
        });
        if let Some(previous_identity) = self.request_envelope_identity.as_ref()
            && !same_stable_segment
        {
            let boundary_reason = request_identity_boundary_reason(previous_identity, &identity);
            self.begin_request_segment(boundary_reason);
        }

        let segment_id = if same_stable_segment {
            self.request_envelope
                .as_ref()
                .map(|envelope| envelope.segment_id().to_owned())
                .or_else(|| self.pending_request_segment_id.take())
                .unwrap_or_else(|| {
                    self.request_segment_sequence = self.request_segment_sequence.saturating_add(1);
                    format!("segment-{:08}", self.request_segment_sequence)
                })
        } else if let Some(pending_segment_id) = self.pending_request_segment_id.take() {
            pending_segment_id
        } else {
            self.request_segment_sequence = self.request_segment_sequence.saturating_add(1);
            format!("segment-{:08}", self.request_segment_sequence)
        };
        // Reuse the candidate's shared `Arc<str>`/`Arc<Vec<ToolDefinition>>`
        // owners and only swap the segment id. Rebuilding via
        // `with_prefix_hash` would deep-clone the whole tool catalog and
        // re-sort/re-hash it, even though `candidate` already holds the
        // canonicalized, hashed form. `begin_segment` keeps the frozen prompt,
        // catalog, and hashes byte-identical while cloning only Arcs.
        let envelope = candidate.begin_segment(segment_id);
        self.request_envelope_identity = Some(identity);
        self.request_envelope_source_tools = tools;
        self.request_envelope = Some(envelope.clone());
        envelope
    }

    pub(crate) fn note_tool_catalog_observability_change(
        &mut self,
        ordered_wire_tool_names: &[String],
        catalog_tool_count: usize,
        wire_tool_count: usize,
        deferred_tool_count: usize,
        active_loaded_skill_names: &[String],
    ) -> bool {
        let identity = ToolCatalogObservabilityIdentity {
            ordered_wire_tool_names: ordered_wire_tool_names.to_vec(),
            catalog_tool_count,
            wire_tool_count,
            deferred_tool_count,
            active_loaded_skill_names: active_loaded_skill_names.to_vec(),
        };
        if self.last_tool_catalog_observability.as_ref() == Some(&identity) {
            return false;
        }
        self.last_tool_catalog_observability = Some(identity);
        true
    }
}

fn request_identity_boundary_reason(
    previous: &RequestEnvelopeIdentity,
    current: &RequestEnvelopeIdentity,
) -> SegmentBoundaryReason {
    if previous.model != current.model {
        SegmentBoundaryReason::Model
    } else if previous.provider != current.provider {
        SegmentBoundaryReason::Provider
    } else if previous.mode != current.mode {
        SegmentBoundaryReason::Mode
    } else if previous.instruction_digest != current.instruction_digest
        || previous.stable_prompt_hash != current.stable_prompt_hash
        || previous.prefix_hash != current.prefix_hash
    {
        SegmentBoundaryReason::Instructions
    } else if previous.catalog_hash != current.catalog_hash {
        SegmentBoundaryReason::ToolCatalogEpoch
    } else {
        SegmentBoundaryReason::PrimaryAgent
    }
}
