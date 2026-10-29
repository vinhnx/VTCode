//! Effective confirmation-skipping for session turns.

/// Effective confirmation-skipping for this turn: the session flag, or Full
/// access (which auto-approves promptable calls session-wide). Evaluated at
/// each use site so plan-policy writes and Full-access toggles stay
/// authoritative without save/restore bookkeeping.
pub(super) fn effective_session_skip_confirmations(
    session_skip_confirmations: bool,
    tool_registry: &vtcode_core::tools::registry::ToolRegistry,
) -> bool {
    session_skip_confirmations || tool_registry.sandbox_config().is_full_access()
}

#[cfg(test)]
mod tests {
    use super::effective_session_skip_confirmations;

    /// Full access implies confirmation-skipping for secondary prompts even
    /// when the session flag is off (e.g. enabled mid-session via /permissions).
    #[tokio::test]
    async fn full_access_implies_session_skip_confirmations() {
        let temp_dir = tempfile::TempDir::new().expect("temp dir");
        std::fs::write(temp_dir.path().join("vtcode.toml"), "[workspace]\nuse_root_config = true\n")
            .expect("write isolated workspace config");
        let registry = vtcode_core::tools::registry::ToolRegistry::new(temp_dir.path().to_path_buf()).await;

        assert!(!effective_session_skip_confirmations(false, &registry));
        assert!(effective_session_skip_confirmations(true, &registry));

        let mut sandbox = vtcode_config::SandboxConfig::default();
        sandbox.enable_full_access();
        registry.apply_sandbox_config(&sandbox);
        assert!(effective_session_skip_confirmations(false, &registry));
        assert!(effective_session_skip_confirmations(true, &registry));
    }
}
