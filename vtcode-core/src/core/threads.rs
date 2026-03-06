use crate::exec::events::ThreadEvent;
use crate::llm::provider::Message;
use crate::utils::session_archive::{
    SessionArchiveMetadata, SessionListing, find_session_by_identifier,
    reserve_session_archive_identifier,
};
use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadOp {
    ConfigureSession,
    UserTurn,
    Interrupt,
    ResumeFromSnapshot,
    ForkFromSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadSubmission {
    pub id: SubmissionId,
    pub op: ThreadOp,
}

impl ThreadSubmission {
    pub fn new(op: ThreadOp) -> Self {
        Self {
            id: SubmissionId::new(),
            op,
        }
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
    pub archive_path: Option<PathBuf>,
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
        let messages = if let Some(progress) = &listing.snapshot.progress
            && !progress.recent_messages.is_empty()
        {
            progress.recent_messages.iter().map(Message::from).collect()
        } else if !listing.snapshot.messages.is_empty() {
            listing.snapshot.messages.iter().map(Message::from).collect()
        } else {
            Vec::new()
        };

        let loaded_skills = listing
            .snapshot
            .progress
            .as_ref()
            .map(|progress| progress.loaded_skills.clone())
            .filter(|skills| !skills.is_empty())
            .unwrap_or_else(|| listing.snapshot.metadata.loaded_skills.clone());

        Self {
            metadata: Some(listing.snapshot.metadata.clone()),
            archive_listing: Some(listing),
            messages,
            loaded_skills,
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

#[derive(Default)]
struct ThreadEventStore {
    capacity: usize,
    next_sequence: u64,
    events: VecDeque<ThreadEventRecord>,
    last_dedupe_key: Option<(Option<String>, String)>,
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
        let event_key = serde_json::to_string(&event).unwrap_or_else(|_| format!("{event:?}"));
        let dedupe_key = (
            submission_id.as_ref().map(|value| value.as_str().to_string()),
            event_key,
        );
        if self.last_dedupe_key.as_ref() == Some(&dedupe_key) {
            return;
        }
        self.last_dedupe_key = Some(dedupe_key);

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
    archive_path: Option<PathBuf>,
    archive_listing: Option<SessionListing>,
    messages: Vec<Message>,
    loaded_skills: Vec<String>,
    submissions: Vec<ThreadSubmission>,
    turn_in_flight: bool,
}

impl ThreadSessionState {
    fn snapshot(&self) -> ThreadSnapshot {
        ThreadSnapshot {
            thread_id: self.thread_id.clone(),
            metadata: self.metadata.clone(),
            archive_path: self.archive_path.clone(),
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
        let archive_path = bootstrap.archive_listing.as_ref().map(|listing| listing.path.clone());
        let session = ThreadSessionState {
            thread_id,
            metadata: bootstrap.metadata,
            archive_path,
            archive_listing: bootstrap.archive_listing,
            messages: bootstrap.messages,
            loaded_skills: bootstrap.loaded_skills,
            submissions: Vec::new(),
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

    pub fn set_loaded_skills(&self, loaded_skills: Vec<String>) {
        self.inner.session.lock().loaded_skills = loaded_skills;
    }

    pub fn loaded_skills(&self) -> Vec<String> {
        self.inner.session.lock().loaded_skills.clone()
    }

    pub fn submit(&self, op: ThreadOp) -> Result<ThreadSubmission> {
        let mut session = self.inner.session.lock();
        if matches!(op, ThreadOp::UserTurn) && session.turn_in_flight {
            return Err(anyhow!(
                "thread '{}' already has an in-flight turn",
                session.thread_id
            ));
        }

        let submission = ThreadSubmission::new(op.clone());
        if matches!(op, ThreadOp::UserTurn) {
            session.turn_in_flight = true;
        }
        if matches!(op, ThreadOp::Interrupt) {
            session.turn_in_flight = false;
        }
        session.submissions.push(submission.clone());
        Ok(submission)
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
            self.start_thread_with_identifier(listing.identifier(), ThreadBootstrap::from_listing(listing))
        }))
    }

    pub async fn fork_thread(
        &self,
        source_identifier: &str,
        workspace_label: &str,
        custom_suffix: Option<String>,
    ) -> Result<Option<ThreadRuntimeHandle>> {
        let Some(listing) = find_session_by_identifier(source_identifier).await? else {
            return Ok(None);
        };

        let bootstrap = ThreadBootstrap::from_listing(listing);
        let handle = self
            .start_thread(workspace_label, custom_suffix, bootstrap)
            .await?;
        Ok(Some(handle))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exec::events::{ThreadEvent, ThreadStartedEvent};

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
        assert_eq!(handle.messages().len(), 1);
        assert_eq!(handle.loaded_skills(), vec!["repo-skill".to_string()]);
    }

    #[test]
    fn submit_enforces_single_in_flight_turn() {
        let manager = ThreadManager::new();
        let handle = manager.start_thread_with_identifier("thread-123", ThreadBootstrap::new(None));

        let _first = handle.submit(ThreadOp::UserTurn).expect("first turn");
        let err = handle.submit(ThreadOp::UserTurn).expect_err("second turn should fail");
        assert!(err.to_string().contains("in-flight turn"));
        handle.finish_turn();
        handle.submit(ThreadOp::UserTurn).expect("turn after finish");
    }
}
