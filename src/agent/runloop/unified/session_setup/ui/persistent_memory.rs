use anyhow::Result;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::config::types::AgentConfig as CoreAgentConfig;
use vtcode_core::persistent_memory::{PersistentMemoryStatus, persistent_memory_status};
use vtcode_tui::app::{
    InlineHeaderContext, InlineHeaderHighlight, InlineHeaderStatusBadge, InlineHeaderStatusTone,
};

pub(super) fn persistent_memory_guide_lines(memory_status: &PersistentMemoryStatus) -> Vec<String> {
    let mut lines = Vec::with_capacity(3);
    if memory_status.cleanup_status.needed {
        lines.push(
            "Memory is enabled, but one-time cleanup is required before updates.".to_string(),
        );
    } else {
        lines.push("Memory is enabled for this workspace.".to_string());
    }
    lines.push("Use `remember ...` to save a note or `forget ...` to remove one.".to_string());
    lines.push(if memory_status.auto_write {
        "Auto-write is on: VT Code may consolidate durable notes after a session.".to_string()
    } else {
        "Auto-write is off: VT Code will only change memory through explicit actions.".to_string()
    });
    lines
}

pub(super) fn load_persistent_memory_status(
    config: &CoreAgentConfig,
    vt_cfg: Option<&VTCodeConfig>,
) -> Result<Option<PersistentMemoryStatus>> {
    let Some(vt_cfg) = vt_cfg else {
        return Ok(None);
    };
    let memory_config = &vt_cfg.agent.persistent_memory;
    if !vt_cfg.persistent_memory_enabled() {
        return Ok(None);
    }

    persistent_memory_status(memory_config, &config.workspace).map(Some)
}

pub(super) fn persistent_memory_header_badge(
    memory_status: &PersistentMemoryStatus,
) -> InlineHeaderStatusBadge {
    if memory_status.cleanup_status.needed {
        return InlineHeaderStatusBadge {
            text: "Memory: Needs cleanup".to_string(),
            tone: InlineHeaderStatusTone::Warning,
        };
    }

    let text = if memory_status.pending_rollout_summaries > 0 {
        format!(
            "Memory: On ({} pending)",
            memory_status.pending_rollout_summaries
        )
    } else {
        "Memory: On".to_string()
    };
    InlineHeaderStatusBadge {
        text,
        tone: InlineHeaderStatusTone::Ready,
    }
}

fn persistent_memory_header_highlight(
    memory_status: &PersistentMemoryStatus,
) -> InlineHeaderHighlight {
    InlineHeaderHighlight {
        title: "Memory".to_string(),
        lines: persistent_memory_guide_lines(memory_status),
    }
}

pub(super) fn apply_persistent_memory_header_guide(
    header_context: &mut InlineHeaderContext,
    memory_status: &PersistentMemoryStatus,
) {
    header_context.persistent_memory = Some(persistent_memory_header_badge(memory_status));
    header_context
        .highlights
        .push(persistent_memory_header_highlight(memory_status));
}
