mod archive;
mod identifier;
mod message;
mod metadata;
mod query;
mod retention;
mod search;
mod settings;
mod shrink;
mod snapshot;
mod transcript;

pub use self::archive::SessionArchive;
#[cfg(test)]
pub(crate) use self::identifier::generate_unique_archive_path;
pub use self::identifier::{generate_session_archive_identifier, reserve_session_archive_identifier};
pub(crate) use self::query::resolve_sessions_dir_for_archive_writes;
pub use self::query::{find_session_by_id_or_name, find_session_by_identifier, list_recent_sessions};
#[cfg(test)]
pub(crate) use self::retention::{SessionRetentionLimits, apply_session_retention_with_limits};
#[cfg(test)]
pub(crate) use self::shrink::{compact_snapshot_to_max_bytes, truncate_preview};
#[cfg(test)]
pub(crate) use self::transcript::clean_transcript_lines;

pub use self::message::SessionMessage;
pub use self::metadata::{
    SessionArchiveMetadata, SessionContinuationExhaustionReason, SessionContinuationMetadata,
    SessionContinuationRecommendedAction, SessionForkMode,
};

pub use self::search::{SearchResult, search_sessions};
pub use self::settings::{
    SESSION_DIR_ENV, SESSION_MAX_AGE_DAYS_ENV, SESSION_MAX_FILES_ENV, SESSION_MAX_SIZE_MB_ENV,
    apply_session_history_config_from_vtcode, history_persistence_enabled,
};
#[cfg(test)]
pub(crate) use self::settings::{
    SessionHistorySettings, clear_sessions_dir_override_for_tests, clear_test_env_override,
    override_sessions_dir_for_tests, session_history_settings, set_test_env_override_path,
};
pub use self::snapshot::{
    SessionListing, SessionProgress, SessionProgressArgs, SessionProgressPersistenceStatus, SessionSnapshot,
    VerifiedSessionArchiveIdentifier, session_listing_matches_workspace, session_workspace_path,
};

const SESSION_FILE_PREFIX: &str = "session";
const SESSION_FILE_EXTENSION: &str = "json";
const DEFAULT_SESSION_MAX_FILES: usize = 100;
const DEFAULT_SESSION_MAX_AGE_DAYS: u64 = 14;
const DEFAULT_SESSION_MAX_SIZE_MB: u64 = 100;
const BYTES_PER_MB: u64 = 1024 * 1024;

#[cfg(test)]
mod metadata_compat_tests {
    use super::{SessionArchive, SessionArchiveMetadata};

    #[test]
    fn primary_agent_metadata_is_backward_compatible() {
        let metadata = serde_json::from_value::<SessionArchiveMetadata>(serde_json::json!({
            "workspace_label": "ws",
            "workspace_path": "/tmp/ws",
            "model": "gpt-5.6-sol",
            "provider": "openai",
            "theme": "test",
            "reasoning_effort": "low"
        }))
        .expect("deserialize metadata without primary_agent");

        assert_eq!(metadata.primary_agent, None);

        let value = serde_json::to_value(&metadata).expect("serialize metadata");
        assert!(value.get("primary_agent").is_none());
    }

    #[test]
    fn set_primary_agent_records_mode_on_archive() {
        let metadata = SessionArchiveMetadata::new("ws", "/tmp/ws", "gpt-5.6-sol", "openai", "test", "low");
        let mut archive = tokio::runtime::Runtime::new()
            .expect("runtime")
            .block_on(SessionArchive::new_with_identifier(metadata, "ws-0000".to_string()))
            .expect("create archive");
        assert_eq!(archive.metadata().primary_agent, None);

        archive.set_primary_agent("plan");
        assert_eq!(archive.metadata().primary_agent.as_deref(), Some("plan"));
    }
}

#[cfg(test)]
#[path = "session_archive_tests.rs"]
mod tests;
