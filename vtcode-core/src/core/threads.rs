use crate::exec::events::ThreadEvent;
use crate::llm::provider::Message;
use crate::utils::session_archive::{
    SessionArchive, SessionArchiveMetadata, SessionListing, find_session_by_identifier,
    list_recent_sessions, reserve_session_archive_identifier, session_listing_matches_workspace,
};
use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use uuid::Uuid;

const DEFAULT_EVENT_BUFFER_CAPACITY: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThreadId(String);

impl ThreadId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SubmissionId(String);

impl SubmissionId {
    pub fn new() -> Self {
        Self(format!("sub-{}", Uuid::new_v4()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for SubmissionId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct ThreadEventRecord {
    pub sequence: u64,
    pub thread_id: ThreadId,
    pub submission_id: Option<SubmissionId>,
    pub turn_id: Option<String>,
    pub event: ThreadEvent,
}

#[derive(Debug, Clone)]
pub struct ThreadSnapshot {
    pub thread_id: ThreadId,
    pub metadata: Option<SessionArchiveMetadata>,
    pub archive_listing: Option<SessionListing>,
    pub messages: Vec<Message>,
    pub loaded_skills: Vec<String>,
    pub turn_in_flight: bool,
}

#[derive(Debug, Clone)]
pub struct ThreadBootstrap {
    pub metadata: Option<SessionArchiveMetadata>,
    pub archive_listing: Option<SessionListing>,
    pub messages: Vec<Message>,
    pub loaded_skills: Vec<String>,
}

impl ThreadBootstrap {
    pub fn new(metadata: Option<SessionArchiveMetadata>) -> Self {
        Self {
            metadata,
            archive_listing: None,
            messages: Vec::new(),
            loaded_skills: Vec::new(),
        }
    }

    pub fn from_listing(listing: SessionListing) -> Self {
        Self {
            metadata: Some(listing.snapshot.metadata.clone()),
            messages: messages_from_session_listing(&listing),
            loaded_skills: loaded_skills_from_session_listing(&listing),
            archive_listing: Some(listing),
        }
    }

    pub fn with_messages(mut self, messages: Vec<Message>) -> Self {
        self.messages = messages;
        self
    }

    pub fn with_loaded_skills(mut self, loaded_skills: Vec<String>) -> Self {
        self.loaded_skills = loaded_skills;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionQueryScope {
    CurrentWorkspace(PathBuf),
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArchivedSessionIntent {
    ResumeInPlace,
    ForkNewArchive { custom_suffix: Option<String> },
}

#[derive(Debug, Clone)]
pub struct PreparedArchivedSession {
    pub source: SessionListing,
    pub workspace: PathBuf,
    pub bootstrap: ThreadBootstrap,
    pub thread_id: String,
    pub archive: SessionArchive,
}

#[derive(Default)]
struct ThreadEventStore {
    capacity: usize,
    next_sequence: u64,
    events: VecDeque<ThreadEventRecord>,
}

impl ThreadEventStore {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            capacity: capacity.max(1),
            ..Self::default()
        }
    }

    fn push(
        &mut self,
        thread_id: &ThreadId,
        submission_id: Option<SubmissionId>,
        turn_id: Option<String>,
        event: ThreadEvent,
    ) {
        let record = ThreadEventRecord {
            sequence: self.next_sequence,
            thread_id: thread_id.clone(),
            submission_id,
            turn_id,
            event,
        };
        self.next_sequence = self.next_sequence.saturating_add(1);

        if self.events.len() >= self.capacity {
            self.events.pop_front();
        }
        self.events.push_back(record);
    }

    fn snapshot(&self) -> Vec<ThreadEventRecord> {
        self.events.iter().cloned().collect()
    }
}

struct ThreadSessionState {
    thread_id: ThreadId,
    metadata: Option<SessionArchiveMetadata>,
    archive_listing: Option<SessionListing>,
    messages: Vec<Message>,
    loaded_skills: Vec<String>,
    turn_in_flight: bool,
}

impl ThreadSessionState {
    fn snapshot(&self) -> ThreadSnapshot {
        ThreadSnapshot {
            thread_id: self.thread_id.clone(),
            metadata: self.metadata.clone(),
            archive_listing: self.archive_listing.clone(),
            messages: self.messages.clone(),
            loaded_skills: self.loaded_skills.clone(),
            turn_in_flight: self.turn_in_flight,
        }
    }
}

#[derive(Clone)]
pub struct ThreadRuntimeHandle {
    inner: Arc<ThreadRuntimeInner>,
}

struct ThreadRuntimeInner {
    session: Mutex<ThreadSessionState>,
    event_store: Mutex<ThreadEventStore>,
}

impl ThreadRuntimeHandle {
    fn new(thread_id: ThreadId, bootstrap: ThreadBootstrap, event_capacity: usize) -> Self {
        let session = ThreadSessionState {
            thread_id,
            metadata: bootstrap.metadata,
            archive_listing: bootstrap.archive_listing,
            messages: bootstrap.messages,
            loaded_skills: bootstrap.loaded_skills,
            turn_in_flight: false,
        };

        Self {
            inner: Arc::new(ThreadRuntimeInner {
                session: Mutex::new(session),
                event_store: Mutex::new(ThreadEventStore::with_capacity(event_capacity)),
            }),
        }
    }

    pub fn thread_id(&self) -> ThreadId {
        self.inner.session.lock().thread_id.clone()
    }

    pub fn snapshot(&self) -> ThreadSnapshot {
        self.inner.session.lock().snapshot()
    }

    pub fn archive_listing(&self) -> Option<SessionListing> {
        self.inner.session.lock().archive_listing.clone()
    }

    pub fn messages(&self) -> Vec<Message> {
        self.inner.session.lock().messages.clone()
    }

    pub fn replace_messages(&self, messages: Vec<Message>) {
        self.inner.session.lock().messages = messages;
    }

    pub fn append_message(&self, message: Message) {
        self.inner.session.lock().messages.push(message);
    }

    pub fn begin_turn(&self) -> Result<SubmissionId> {
        let mut session = self.inner.session.lock();
        if session.turn_in_flight {
            return Err(anyhow!(
                "thread '{}' already has an in-flight turn",
                session.thread_id
            ));
        }

        session.turn_in_flight = true;
        Ok(SubmissionId::new())
    }

    pub fn finish_turn(&self) {
        self.inner.session.lock().turn_in_flight = false;
    }

    pub fn record_event(
        &self,
        submission_id: Option<SubmissionId>,
        turn_id: Option<String>,
        event: ThreadEvent,
    ) {
        let thread_id = self.thread_id();
        self.inner
            .event_store
            .lock()
            .push(&thread_id, submission_id, turn_id, event);
    }

    pub fn replay_recent(&self) -> Vec<ThreadEventRecord> {
        self.inner.event_store.lock().snapshot()
    }

    pub fn recent_events(&self) -> Vec<ThreadEvent> {
        self.replay_recent()
            .into_iter()
            .map(|record| record.event)
            .collect()
    }
}

#[derive(Clone)]
pub struct ThreadManager {
    event_buffer_capacity: usize,
}

impl Default for ThreadManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ThreadManager {
    pub fn new() -> Self {
        Self {
            event_buffer_capacity: DEFAULT_EVENT_BUFFER_CAPACITY,
        }
    }

    pub fn with_event_buffer_capacity(event_buffer_capacity: usize) -> Self {
        Self {
            event_buffer_capacity: event_buffer_capacity.max(1),
        }
    }

    pub fn start_thread_with_identifier(
        &self,
        identifier: impl Into<String>,
        bootstrap: ThreadBootstrap,
    ) -> ThreadRuntimeHandle {
        ThreadRuntimeHandle::new(
            ThreadId::new(identifier.into()),
            bootstrap,
            self.event_buffer_capacity,
        )
    }

    pub async fn start_thread(
        &self,
        workspace_label: &str,
        custom_suffix: Option<String>,
        bootstrap: ThreadBootstrap,
    ) -> Result<ThreadRuntimeHandle> {
        let identifier = reserve_session_archive_identifier(workspace_label, custom_suffix).await?;
        Ok(self.start_thread_with_identifier(identifier, bootstrap))
    }

    pub async fn resume_thread(&self, identifier: &str) -> Result<Option<ThreadRuntimeHandle>> {
        let listing = find_session_by_identifier(identifier).await?;
        Ok(listing.map(|listing| {
            self.start_thread_with_identifier(
                listing.identifier(),
                ThreadBootstrap::from_listing(listing),
            )
        }))
    }
}

pub async fn list_recent_sessions_in_scope(
    limit: usize,
    scope: &SessionQueryScope,
) -> Result<Vec<SessionListing>> {
    let mut listings = list_recent_sessions(limit.saturating_mul(4).max(limit)).await?;
    if let SessionQueryScope::CurrentWorkspace(workspace) = scope {
        listings.retain(|listing| session_listing_matches_workspace(listing, workspace));
    }
    listings.truncate(limit);
    Ok(listings)
}

pub async fn prepare_archived_session(
    source: SessionListing,
    workspace: PathBuf,
    metadata: SessionArchiveMetadata,
    intent: ArchivedSessionIntent,
    reserved_identifier: Option<String>,
) -> Result<PreparedArchivedSession> {
    let mut bootstrap = ThreadBootstrap::from_listing(source.clone());
    bootstrap.metadata = Some(metadata.clone());

    let thread_id = match &intent {
        ArchivedSessionIntent::ResumeInPlace => source.identifier(),
        ArchivedSessionIntent::ForkNewArchive { custom_suffix } => {
            if let Some(identifier) = reserved_identifier {
                identifier
            } else {
                reserve_session_archive_identifier(&metadata.workspace_label, custom_suffix.clone())
                    .await?
            }
        }
    };

    let archive = match intent {
        ArchivedSessionIntent::ResumeInPlace => {
            SessionArchive::resume_from_listing(&source, metadata)
        }
        ArchivedSessionIntent::ForkNewArchive { .. } => {
            SessionArchive::new_with_identifier(metadata, thread_id.clone()).await?
        }
    };

    Ok(PreparedArchivedSession {
        source,
        workspace,
        bootstrap,
        thread_id,
        archive,
    })
}

pub fn messages_from_session_listing(listing: &SessionListing) -> Vec<Message> {
    if let Some(progress) = &listing.snapshot.progress
        && !progress.recent_messages.is_empty()
    {
        progress.recent_messages.iter().map(Message::from).collect()
    } else if !listing.snapshot.messages.is_empty() {
        listing
            .snapshot
            .messages
            .iter()
            .map(Message::from)
            .collect()
    } else {
        Vec::new()
    }
}

pub fn loaded_skills_from_session_listing(listing: &SessionListing) -> Vec<String> {
    listing
        .snapshot
        .progress
        .as_ref()
        .map(|progress| progress.loaded_skills.clone())
        .filter(|skills| !skills.is_empty())
        .unwrap_or_else(|| listing.snapshot.metadata.loaded_skills.clone())
}

pub fn build_thread_archive_metadata(
    workspace: &Path,
    model: &str,
    provider: &str,
    theme: &str,
    reasoning_effort: &str,
) -> SessionArchiveMetadata {
    let workspace_label = workspace
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace");

    SessionArchiveMetadata::new(
        workspace_label,
        workspace.to_string_lossy().to_string(),
        model,
        provider,
        theme,
        reasoning_effort,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::events::{ThreadEvent, ThreadStartedEvent};
    use crate::llm::provider::MessageRole;
    use crate::utils::session_archive::{
        SessionArchiveMetadata, SessionMessage, SessionProgress, SessionSnapshot,
        clear_sessions_dir_override_for_tests, override_sessions_dir_for_tests,
    };
    use chrono::Utc;
    use std::sync::{LazyLock, Mutex};
    use tempfile::TempDir;

    static SESSION_DIR_TEST_GUARD: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    #[test]
    fn event_store_evicts_old_records() {
        let manager = ThreadManager::with_event_buffer_capacity(2);
        let handle = manager.start_thread_with_identifier("thread-1", ThreadBootstrap::new(None));

        handle.record_event(
            None,
            None,
            ThreadEvent::ThreadStarted(ThreadStartedEvent {
                thread_id: "thread-1".to_string(),
            }),
        );
        handle.record_event(
            None,
            Some("turn-1".to_string()),
            ThreadEvent::ThreadStarted(ThreadStartedEvent {
                thread_id: "thread-1-turn-1".to_string(),
            }),
        );
        handle.record_event(
            None,
            Some("turn-2".to_string()),
            ThreadEvent::ThreadStarted(ThreadStartedEvent {
                thread_id: "thread-1-turn-2".to_string(),
            }),
        );

        let records = handle.replay_recent();
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].sequence, 1);
        assert_eq!(records[1].sequence, 2);
    }

    #[test]
    fn start_thread_with_identifier_preserves_message_history() {
        let manager = ThreadManager::new();
        let bootstrap = ThreadBootstrap::new(None)
            .with_messages(vec![Message::user("hello".to_string())])
            .with_loaded_skills(vec!["repo-skill".to_string()]);
        let handle = manager.start_thread_with_identifier("thread-123", bootstrap);

        assert_eq!(handle.thread_id().as_str(), "thread-123");
        let snapshot = handle.snapshot();
        assert_eq!(snapshot.messages.len(), 1);
        assert_eq!(snapshot.loaded_skills, vec!["repo-skill".to_string()]);
    }

    #[test]
    fn submit_enforces_single_in_flight_turn() {
        let manager = ThreadManager::new();
        let handle = manager.start_thread_with_identifier("thread-123", ThreadBootstrap::new(None));

        let _first = handle.begin_turn().expect("first turn");
        let err = handle.begin_turn().expect_err("second turn should fail");
        assert!(err.to_string().contains("in-flight turn"));
        handle.finish_turn();
        handle.begin_turn().expect("turn after finish");
    }

    #[test]
    fn list_recent_sessions_in_scope_filters_by_workspace() {
        let _guard = SESSION_DIR_TEST_GUARD
            .lock()
            .expect("session dir test guard");
        let tmp = TempDir::new().expect("temp dir");
        override_sessions_dir_for_tests(tmp.path());

        let listing = SessionListing {
            path: tmp.path().join("session-alpha.json"),
            snapshot: SessionSnapshot {
                metadata: SessionArchiveMetadata::new(
                    "ws",
                    tmp.path().join("workspace").display().to_string(),
                    "model",
                    "provider",
                    "theme",
                    "medium",
                ),
                started_at: Utc::now(),
                ended_at: Utc::now(),
                total_messages: 1,
                distinct_tools: Vec::new(),
                transcript: Vec::new(),
                messages: vec![SessionMessage::new(MessageRole::User, "hello")],
                progress: None,
                error_logs: Vec::new(),
            },
        };
        std::fs::write(
            &listing.path,
            serde_json::to_string(&listing.snapshot).expect("serialize snapshot"),
        )
        .expect("write listing");

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let filtered = runtime
            .block_on(list_recent_sessions_in_scope(
                5,
                &SessionQueryScope::CurrentWorkspace(tmp.path().join("workspace")),
            ))
            .expect("filter by workspace");
        let all = runtime
            .block_on(list_recent_sessions_in_scope(5, &SessionQueryScope::All))
            .expect("list all");

        clear_sessions_dir_override_for_tests();

        assert_eq!(filtered.len(), 1);
        assert_eq!(all.len(), 1);
    }

    #[test]
    fn prepare_archived_session_resume_reuses_source_identifier_and_archive() {
        let _guard = SESSION_DIR_TEST_GUARD
            .lock()
            .expect("session dir test guard");
        let tmp = TempDir::new().expect("temp dir");
        override_sessions_dir_for_tests(tmp.path());

        let listing = SessionListing {
            path: tmp.path().join("session-source.json"),
            snapshot: SessionSnapshot {
                metadata: SessionArchiveMetadata::new(
                    "ws",
                    tmp.path().join("workspace").display().to_string(),
                    "old-model",
                    "old-provider",
                    "old-theme",
                    "medium",
                ),
                started_at: Utc::now(),
                ended_at: Utc::now(),
                total_messages: 2,
                distinct_tools: vec!["tool_a".to_string()],
                transcript: Vec::new(),
                messages: vec![SessionMessage::new(MessageRole::User, "hello")],
                progress: Some(SessionProgress {
                    turn_number: 1,
                    recent_messages: vec![SessionMessage::new(MessageRole::Assistant, "recent")],
                    tool_summaries: Vec::new(),
                    token_usage: None,
                    max_context_tokens: None,
                    loaded_skills: vec!["skill_a".to_string()],
                }),
                error_logs: Vec::new(),
            },
        };

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let prepared = runtime
            .block_on(prepare_archived_session(
                listing.clone(),
                tmp.path().join("workspace"),
                SessionArchiveMetadata::new(
                    "ws",
                    tmp.path().join("workspace").display().to_string(),
                    "new-model",
                    "new-provider",
                    "new-theme",
                    "high",
                ),
                ArchivedSessionIntent::ResumeInPlace,
                Some("should-not-be-used".to_string()),
            ))
            .expect("prepare resume");

        clear_sessions_dir_override_for_tests();

        assert_eq!(prepared.thread_id, listing.identifier());
        assert_eq!(prepared.archive.path(), listing.path.as_path());
        assert_eq!(prepared.bootstrap.messages[0].content.as_text(), "recent");
        assert_eq!(
            prepared.bootstrap.loaded_skills,
            vec!["skill_a".to_string()]
        );
        assert_eq!(
            prepared
                .bootstrap
                .metadata
                .as_ref()
                .expect("metadata")
                .model,
            "new-model"
        );
    }

    #[test]
    fn prepare_archived_session_fork_uses_new_identifier_and_preserves_history() {
        let _guard = SESSION_DIR_TEST_GUARD
            .lock()
            .expect("session dir test guard");
        let tmp = TempDir::new().expect("temp dir");
        override_sessions_dir_for_tests(tmp.path());

        let listing = SessionListing {
            path: tmp.path().join("session-source.json"),
            snapshot: SessionSnapshot {
                metadata: SessionArchiveMetadata::new(
                    "ws",
                    tmp.path().join("workspace").display().to_string(),
                    "model",
                    "provider",
                    "theme",
                    "medium",
                ),
                started_at: Utc::now(),
                ended_at: Utc::now(),
                total_messages: 1,
                distinct_tools: Vec::new(),
                transcript: Vec::new(),
                messages: vec![SessionMessage::new(MessageRole::User, "hello")],
                progress: None,
                error_logs: Vec::new(),
            },
        };

        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        let prepared = runtime
            .block_on(prepare_archived_session(
                listing.clone(),
                tmp.path().join("workspace"),
                SessionArchiveMetadata::new(
                    "ws",
                    tmp.path().join("workspace").display().to_string(),
                    "model",
                    "provider",
                    "theme",
                    "medium",
                ),
                ArchivedSessionIntent::ForkNewArchive {
                    custom_suffix: Some("branch".to_string()),
                },
                Some("session-forked".to_string()),
            ))
            .expect("prepare fork");

        clear_sessions_dir_override_for_tests();

        assert_eq!(prepared.thread_id, "session-forked");
        assert_ne!(prepared.archive.path(), listing.path.as_path());
        assert!(
            prepared
                .archive
                .path()
                .ends_with(Path::new("session-forked.json"))
        );
        assert_eq!(prepared.bootstrap.messages[0].content.as_text(), "hello");
    }

    #[test]
    fn messages_from_session_listing_preserves_assistant_phases_from_progress() {
        let listing = SessionListing {
            path: PathBuf::from("session.json"),
            snapshot: SessionSnapshot {
                metadata: SessionArchiveMetadata::new(
                    "ws", "/tmp/ws", "gpt-5.4", "openai", "theme", "medium",
                ),
                started_at: Utc::now(),
                ended_at: Utc::now(),
                total_messages: 2,
                distinct_tools: Vec::new(),
                transcript: Vec::new(),
                messages: Vec::new(),
                progress: Some(SessionProgress {
                    turn_number: 2,
                    recent_messages: vec![
                        SessionMessage::from(
                            &Message::assistant("Working".to_string())
                                .with_phase(Some(crate::llm::provider::AssistantPhase::Commentary)),
                        ),
                        SessionMessage::from(
                            &Message::assistant("Done".to_string()).with_phase(Some(
                                crate::llm::provider::AssistantPhase::FinalAnswer,
                            )),
                        ),
                    ],
                    tool_summaries: Vec::new(),
                    token_usage: None,
                    max_context_tokens: None,
                    loaded_skills: Vec::new(),
                }),
                error_logs: Vec::new(),
            },
        };

        let messages = messages_from_session_listing(&listing);
        assert_eq!(
            messages
                .iter()
                .map(|message| message.phase)
                .collect::<Vec<_>>(),
            vec![
                Some(crate::llm::provider::AssistantPhase::Commentary),
                Some(crate::llm::provider::AssistantPhase::FinalAnswer),
            ]
        );
    }
}
