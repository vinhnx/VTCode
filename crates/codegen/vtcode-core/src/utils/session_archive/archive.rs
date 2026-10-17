use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};

use super::identifier::{generate_unique_archive_path, reserve_new_session_archive_path};
use super::message::SessionMessage;
use super::metadata::{SessionArchiveMetadata, SessionContinuationMetadata};
use super::query::{find_session_by_identifier, resolve_sessions_dir_for_archive_writes, write_private_json_file_sync};
use super::retention::apply_session_retention_best_effort;
use super::settings::history_persistence_enabled;
use super::shrink::prepare_snapshot_for_write;
use super::snapshot::{
    SessionListing, SessionProgress, SessionProgressArgs, SessionProgressPersistenceStatus, SessionSnapshot,
    VerifiedSessionArchiveIdentifier,
};
use super::transcript::{clean_transcript_lines, progress_transcript_from_recent_messages};
use crate::config::constants::defaults;
use crate::core::agent::snapshots::SnapshotTurnDiagnostics;
use crate::telemetry::perf::PerfSpan;
use crate::utils::session_transcript_norm::normalize_distinct_tools_for_summary;
use vtcode_commons::fs::write_private_json_file;

#[derive(Debug, Clone)]
pub struct SessionArchive {
    path: PathBuf,
    metadata: SessionArchiveMetadata,
    started_at: DateTime<Utc>,
    progress_throttle: Arc<Mutex<ProgressThrottle>>,
}

#[derive(Debug)]
struct ProgressThrottle {
    last_written: Instant,
    last_turn: usize,
}

impl ProgressThrottle {
    fn new() -> Self {
        let min_interval = Duration::from_millis(defaults::DEFAULT_SESSION_PROGRESS_MIN_INTERVAL_MS);
        let last_written = Instant::now().checked_sub(min_interval).unwrap_or_else(Instant::now);
        Self { last_written, last_turn: 0 }
    }
}

impl SessionArchive {
    fn from_path(path: PathBuf, metadata: SessionArchiveMetadata, started_at: DateTime<Utc>) -> Self {
        Self {
            path,
            metadata,
            started_at,
            progress_throttle: Arc::new(Mutex::new(ProgressThrottle::new())),
        }
    }

    fn build_snapshot(
        &self,
        total_messages: usize,
        distinct_tools: Vec<String>,
        transcript: Vec<String>,
        messages: Vec<SessionMessage>,
        progress: Option<SessionProgress>,
    ) -> SessionSnapshot {
        use crate::utils::error_log_collector::drain_error_logs;

        SessionSnapshot {
            metadata: self.metadata.clone(),
            started_at: self.started_at,
            ended_at: Utc::now(),
            total_messages,
            distinct_tools,
            transcript,
            messages,
            progress: progress.map(Box::new),
            error_logs: drain_error_logs(),
        }
    }

    /// Records the active primary agent (session "mode") so a later resume can
    /// restore it. Called once the active agent is selected, after the archive
    /// and thread have been started.
    pub fn set_primary_agent(&mut self, primary_agent: impl Into<String>) {
        self.metadata.primary_agent = Some(primary_agent.into());
    }

    pub fn metadata(&self) -> &SessionArchiveMetadata {
        &self.metadata
    }

    fn build_final_snapshot(
        &self,
        transcript: Vec<String>,
        total_messages: usize,
        distinct_tools: Vec<String>,
        messages: Vec<SessionMessage>,
        turn_diagnostics: Option<SnapshotTurnDiagnostics>,
    ) -> SessionSnapshot {
        self.build_snapshot(
            total_messages,
            normalize_distinct_tools_for_summary(&distinct_tools),
            clean_transcript_lines(&transcript),
            messages,
            turn_diagnostics.map(|turn_diagnostics| SessionProgress {
                turn_diagnostics: Some(turn_diagnostics),
                ..SessionProgress::default()
            }),
        )
    }

    fn build_progress_snapshot(&self, args: SessionProgressArgs) -> SessionSnapshot {
        let SessionProgressArgs {
            total_messages,
            distinct_tools,
            messages,
            recent_messages,
            turn_number,
            token_usage,
            max_context_tokens,
            loaded_skills,
            turn_diagnostics,
        } = args;

        let transcript = progress_transcript_from_recent_messages(&recent_messages);
        let distinct_tools = normalize_distinct_tools_for_summary(&distinct_tools);
        let tool_summaries = distinct_tools.clone();

        self.build_snapshot(
            total_messages,
            distinct_tools,
            transcript,
            messages,
            Some(SessionProgress {
                turn_number,
                recent_messages,
                tool_summaries,
                token_usage,
                max_context_tokens,
                loaded_skills: loaded_skills.unwrap_or_default(),
                turn_diagnostics,
            }),
        )
    }

    pub async fn new(metadata: SessionArchiveMetadata, custom_suffix: Option<String>) -> Result<Self> {
        let sessions_dir = resolve_sessions_dir_for_archive_writes().await?;
        let started_at = Utc::now();
        let path = generate_unique_archive_path(&sessions_dir, &metadata, started_at, custom_suffix.as_deref());

        Ok(Self::from_path(path, metadata, started_at))
    }

    /// Create a session archive using an explicitly reserved session identifier.
    pub async fn new_with_identifier(metadata: SessionArchiveMetadata, session_identifier: String) -> Result<Self> {
        let sessions_dir = resolve_sessions_dir_for_archive_writes().await?;
        let path = reserve_new_session_archive_path(&sessions_dir, &session_identifier)?;

        Ok(Self::from_path(path, metadata, Utc::now()))
    }

    /// Reopen an existing archive file so follow-up runs can overwrite the snapshot in place.
    pub fn resume_from_listing(listing: &SessionListing, metadata: SessionArchiveMetadata) -> Self {
        Self::from_path(listing.path.clone(), metadata, listing.snapshot.started_at)
    }

    pub fn finalize(
        &self,
        transcript: Vec<String>,
        total_messages: usize,
        distinct_tools: Vec<String>,
        messages: Vec<SessionMessage>,
    ) -> Result<PathBuf> {
        self.finalize_with_diagnostics(transcript, total_messages, distinct_tools, messages, None)
    }

    /// Finalize the archive while retaining lightweight diagnostics for the
    /// last turn. The final snapshot still omits the heavy progress history.
    pub fn finalize_with_diagnostics(
        &self,
        transcript: Vec<String>,
        total_messages: usize,
        distinct_tools: Vec<String>,
        messages: Vec<SessionMessage>,
        turn_diagnostics: Option<SnapshotTurnDiagnostics>,
    ) -> Result<PathBuf> {
        let snapshot =
            self.build_final_snapshot(transcript, total_messages, distinct_tools, messages, turn_diagnostics);

        let path = self.write_snapshot(snapshot)?;
        if let Some(parent) = path.parent() {
            apply_session_retention_best_effort(parent);
        }
        Ok(path)
    }

    pub fn persist_progress(&self, args: SessionProgressArgs) -> Result<PathBuf> {
        let mut perf = PerfSpan::new("vtcode.perf.session_progress_write_ms");
        perf.tag("mode", "sync");

        let snapshot = self.build_progress_snapshot(args);

        self.write_snapshot(snapshot)
    }

    /// Persist progress asynchronously and report whether this call wrote a
    /// new durable snapshot.
    ///
    /// A throttled checkpoint still returns the archive path for diagnostics,
    /// but it must not be treated as durable by callers that acknowledge
    /// state only after history has been written.
    pub async fn persist_progress_async_with_status(
        &self,
        args: SessionProgressArgs,
    ) -> Result<SessionProgressPersistenceStatus> {
        self.persist_progress_async_with_status_inner(args, false).await
    }

    async fn persist_progress_async_with_status_inner(
        &self,
        args: SessionProgressArgs,
        bypass_throttle: bool,
    ) -> Result<SessionProgressPersistenceStatus> {
        let mut perf = PerfSpan::new("vtcode.perf.session_progress_write_ms");
        perf.tag("mode", if bypass_throttle { "async_forced" } else { "async" });

        if !history_persistence_enabled() {
            return Ok(SessionProgressPersistenceStatus::Disabled(self.path.clone()));
        }
        if !bypass_throttle && !self.should_persist_progress(args.turn_number)? {
            return Ok(SessionProgressPersistenceStatus::Throttled(self.path.clone()));
        }

        let snapshot = self.build_progress_snapshot(args);
        let path = self.write_snapshot_async(snapshot).await?;
        Ok(SessionProgressPersistenceStatus::Persisted(path))
    }

    /// Persist progress immediately, bypassing the normal progress throttle.
    ///
    /// This is intended for terminal handoff checkpoints where the latest
    /// conversation must be durable before an external artifact is written.
    pub async fn persist_progress_async_with_status_forced(
        &self,
        args: SessionProgressArgs,
    ) -> Result<SessionProgressPersistenceStatus> {
        self.persist_progress_async_with_status_inner(args, true).await
    }

    pub async fn persist_progress_async(&self, args: SessionProgressArgs) -> Result<PathBuf> {
        Ok(self.persist_progress_async_with_status(args).await?.path().to_path_buf())
    }

    fn write_snapshot(&self, snapshot: SessionSnapshot) -> Result<PathBuf> {
        let Some(snapshot) = prepare_snapshot_for_write(snapshot)? else {
            return Ok(self.path.clone());
        };

        write_private_json_file_sync(&self.path, &snapshot)?;
        Ok(self.path.clone())
    }

    async fn write_snapshot_async(&self, snapshot: SessionSnapshot) -> Result<PathBuf> {
        let Some(snapshot) = prepare_snapshot_for_write(snapshot)? else {
            return Ok(self.path.clone());
        };

        write_private_json_file(&self.path, &snapshot).await?;
        Ok(self.path.clone())
    }

    fn should_persist_progress(&self, turn_number: usize) -> Result<bool> {
        let min_interval = Duration::from_millis(defaults::DEFAULT_SESSION_PROGRESS_MIN_INTERVAL_MS);
        let min_turns = defaults::DEFAULT_SESSION_PROGRESS_MIN_TURN_DELTA;

        let mut throttle = self
            .progress_throttle
            .lock()
            .map_err(|err| anyhow::anyhow!("session progress throttle lock poisoned: {err}"))
            .context("Failed to evaluate session progress persistence throttle")?;
        if turn_number <= throttle.last_turn {
            return Ok(false);
        }
        if throttle.last_written.elapsed() < min_interval && turn_number.saturating_sub(throttle.last_turn) < min_turns
        {
            return Ok(false);
        }

        throttle.last_written = Instant::now();
        throttle.last_turn = turn_number;
        Ok(true)
    }
    /// Update loaded skills in the archive metadata
    pub fn set_loaded_skills(&mut self, skills: Vec<String>) {
        self.metadata.loaded_skills = skills;
    }

    /// Update continuation metadata in the archive metadata.
    pub fn set_continuation_metadata(&mut self, continuation_metadata: Option<SessionContinuationMetadata>) {
        self.metadata.continuation_metadata = continuation_metadata;
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Verify that a persisted snapshot resolves back to this archive and
    /// return its typed resume identifier.
    pub async fn verify_persisted_resume_identifier(
        &self,
        persisted_path: &Path,
    ) -> Result<Option<VerifiedSessionArchiveIdentifier>> {
        if self.path != persisted_path {
            return Ok(None);
        }

        let Some(identifier) = persisted_path.file_stem().and_then(|stem| stem.to_str()) else {
            return Ok(None);
        };
        let Some(listing) = find_session_by_identifier(identifier).await? else {
            return Ok(None);
        };
        if listing.path != self.path {
            return Ok(None);
        }

        Ok(Some(listing.verified_identifier()?))
    }

    /// Create a forked session from an existing session snapshot
    ///
    /// This creates a new session archive that inherits metadata from the source
    /// session but operates independently. The forked session will have a new
    /// archive file with a custom suffix if provided.
    ///
    /// # Arguments
    /// * `source_snapshot` - The snapshot of the session to fork from
    /// * `custom_suffix` - Optional custom suffix for the new session ID
    ///
    /// # Returns
    /// A new SessionArchive instance for the forked session
    pub async fn fork(source_snapshot: &SessionSnapshot, custom_suffix: Option<String>) -> Result<Self> {
        create_fork_archive(source_snapshot, custom_suffix, None).await
    }
}

async fn create_fork_archive(
    source_snapshot: &SessionSnapshot,
    custom_suffix: Option<String>,
    explicit_identifier: Option<String>,
) -> Result<SessionArchive> {
    let sessions_dir = resolve_sessions_dir_for_archive_writes().await?;
    let started_at = Utc::now();

    let forked_metadata = source_snapshot.metadata.fork_seed();

    let path = if let Some(session_identifier) = explicit_identifier {
        reserve_new_session_archive_path(&sessions_dir, &session_identifier)?
    } else {
        generate_unique_archive_path(&sessions_dir, &forked_metadata, started_at, custom_suffix.as_deref())
    };

    Ok(SessionArchive::from_path(path, forked_metadata, started_at))
}
