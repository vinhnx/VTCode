//! Context reset logic for long-running harness sessions.
use anyhow::Result;
use std::path::Path;

const CONTEXT_RESET_DIR: &str = ".vtcode/tasks";
pub const CONTEXT_RESET_FILE: &str = "current_context_reset.md";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContextResetDecision {
    Continue,
    Reset { reason: String, stall_count: u32 },
}

#[derive(Debug, Clone)]
pub struct ContextResetManifest {
    pub triggered_at: String,
    pub trigger: String,
    pub stall_count: u32,
}

impl ContextResetManifest {
    pub fn to_markdown(&self) -> String {
        format!(
            "# Context Reset Manifest\n\n - **Triggered at:** {}\n - **Trigger:** {}\n - **Stall count:** {}\n\nThis session starts fresh from external artifacts only.\nRead the current feature list and progress tracker to reorient.\n",
            self.triggered_at, self.trigger, self.stall_count
        )
    }
    pub fn from_markdown(md: &str) -> Option<Self> {
        /// Extract the value following `- **<key>:**` on a line, trimmed.
        fn field_value(line: &str, key: &str) -> Option<String> {
            let line = line.trim();
            let prefix = format!("- **{key}:**");
            line.strip_prefix(&prefix)
                .map(|rest| rest.trim().to_string())
        }

        let mut a = String::new();
        let mut b = String::new();
        let mut c = 0u32;
        for line in md.lines() {
            if let Some(v) = field_value(line, "Triggered at") {
                a = v;
            } else if let Some(v) = field_value(line, "Trigger") {
                b = v;
            } else if let Some(v) = field_value(line, "Stall count") {
                c = v.parse().unwrap_or(0);
            }
        }
        if a.is_empty() || b.is_empty() {
            return None;
        }
        Some(Self {
            triggered_at: a,
            trigger: b,
            stall_count: c,
        })
    }
}

pub fn should_reset(
    mode: &str,
    compaction: bool,
    stall: u32,
    threshold: u32,
) -> ContextResetDecision {
    match mode {
        "off" => ContextResetDecision::Continue,
        "on_compaction" => {
            if compaction {
                ContextResetDecision::Reset {
                    reason: "compaction triggered reset".into(),
                    stall_count: 0,
                }
            } else {
                ContextResetDecision::Continue
            }
        }
        "on_stall" => {
            if stall >= threshold && threshold > 0 {
                ContextResetDecision::Reset {
                    reason: format!("stall {stall}>={threshold}"),
                    stall_count: stall,
                }
            } else {
                ContextResetDecision::Continue
            }
        }
        _ => ContextResetDecision::Continue,
    }
}

pub fn write_manifest(workspace_root: &Path, manifest: &ContextResetManifest) -> Result<bool> {
    let dir = workspace_root.join(CONTEXT_RESET_DIR);
    std::fs::create_dir_all(&dir)?;
    std::fs::write(dir.join(CONTEXT_RESET_FILE), manifest.to_markdown())?;
    Ok(true)
}

pub fn read_manifest(workspace_root: &Path) -> Option<ContextResetManifest> {
    let p = workspace_root
        .join(CONTEXT_RESET_DIR)
        .join(CONTEXT_RESET_FILE);
    let c = std::fs::read_to_string(&p).ok()?;
    if c.trim().is_empty() {
        None
    } else {
        ContextResetManifest::from_markdown(&c)
    }
}

pub fn consume_manifest(workspace_root: &Path) {
    let _ = std::fs::remove_file(
        workspace_root
            .join(CONTEXT_RESET_DIR)
            .join(CONTEXT_RESET_FILE),
    );
}

/// The `stall_count` is 0 because compaction-triggered resets are not stall-driven;
/// the threshold check is enforced only for `on_stall` mode in `should_reset`.
pub fn maybe_write_reset_after_compaction(workspace_root: &Path, mode: &str) -> bool {
    if mode != "on_compaction" {
        return false;
    }
    let m = ContextResetManifest {
        triggered_at: chrono::Utc::now().to_rfc3339(),
        trigger: "compaction".into(),
        stall_count: 0,
    };
    write_manifest(workspace_root, &m).unwrap_or(false)
}

pub fn maybe_write_reset_on_stall(
    workspace_root: &Path,
    stall_count: u32,
    mode: &str,
    threshold: u32,
) {
    if let ContextResetDecision::Reset {
        stall_count: sc, ..
    } = should_reset(mode, false, stall_count, threshold)
    {
        let m = ContextResetManifest {
            triggered_at: chrono::Utc::now().to_rfc3339(),
            trigger: "stall".into(),
            stall_count: sc,
        };
        let _ = write_manifest(workspace_root, &m);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn off_always_continues() {
        assert!(matches!(
            should_reset("off", true, 10, 2),
            ContextResetDecision::Continue
        ));
    }
    #[test]
    fn compaction_triggers() {
        assert!(matches!(
            should_reset("on_compaction", true, 0, 0),
            ContextResetDecision::Reset { .. }
        ));
    }
    #[test]
    fn stall_triggers_at_threshold() {
        assert!(matches!(
            should_reset("on_stall", false, 2, 2),
            ContextResetDecision::Reset { stall_count: 2, .. }
        ));
    }
    #[test]
    fn manifest_round_trips_through_markdown() {
        let m = ContextResetManifest {
            triggered_at: "2026-01-01T00:00:00Z".into(),
            trigger: "stall".into(),
            stall_count: 3,
        };
        let md = m.to_markdown();
        let back = ContextResetManifest::from_markdown(&md).expect("round-trip");
        assert_eq!(back.triggered_at, m.triggered_at);
        assert_eq!(back.trigger, m.trigger);
        assert_eq!(back.stall_count, 3);
    }
    #[test]
    fn manifest_from_empty_is_none() {
        assert!(ContextResetManifest::from_markdown("").is_none());
    }
}
