use crate::exec::events::ThreadEvent;
use crate::llm::provider::Message;
use crate::utils::session_archive::{
    SessionArchive, SessionArchiveMetadata, SessionForkMode, SessionListing, find_session_by_identifier,
    list_recent_sessions, reserve_session_archive_identifier, session_listing_matches_workspace,
};
use crate::utils::session_debug::runtime_archive_session_id;
use anyhow::{Result, anyhow};
use parking_lot::Mutex;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub use vtcode_threads::{SubmissionId, ThreadId};

const DEFAULT_EVENT_BUFFER_CAPACITY: usize = 512;

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

    pub fn from_snapshot(snapshot: ThreadSnapshot) -> Self {
        Self {
            metadata: snapshot.metadata,
            archive_listing: snapshot.archive_listing,
            messages: snapshot.messages,
            loaded_skills: snapshot.loaded_skills,
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
    ForkNewArchive {
        custom_suffix: Option<String>,
        summarize: bool,
    },
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
        Self { capacity: capacity.max(1), ..Self::default() }
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

    pub fn metadata(&self) -> Option<SessionArchiveMetadata> {
        self.inner.session.lock().metadata.clone()
    }

    pub fn replace_metadata(&self, metadata: Option<SessionArchiveMetadata>) {
        self.inner.session.lock().metadata = metadata;
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
            return Err(anyhow!("thread '{}' already has an in-flight turn", session.thread_id));
        }

        session.turn_in_flight = true;
        Ok(SubmissionId::generate())
    }

    pub fn finish_turn(&self) {
        self.inner.session.lock().turn_in_flight = false;
    }

    pub fn record_event(&self, submission_id: Option<SubmissionId>, turn_id: Option<String>, event: ThreadEvent) {
        let thread_id = self.thread_id();
        self.inner.event_store.lock().push(&thread_id, submission_id, turn_id, event);
    }

    pub fn replay_recent(&self) -> Vec<ThreadEventRecord> {
        self.inner.event_store.lock().snapshot()
    }

    pub fn recent_events(&self) -> Vec<ThreadEvent> {
        self.replay_recent().into_iter().map(|record| record.event).collect()
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
        ThreadRuntimeHandle::new(ThreadId::new(identifier.into()), bootstrap, self.event_buffer_capacity)
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
}

pub async fn list_recent_sessions_in_scope(limit: usize, scope: &SessionQueryScope) -> Result<Vec<SessionListing>> {
    let mut listings = list_recent_sessions(limit.saturating_mul(4).max(limit)).await?;
    if let SessionQueryScope::CurrentWorkspace(workspace) = scope {
        listings.retain(|listing| session_listing_matches_workspace(listing, workspace));
    }
    let current_id = runtime_archive_session_id();
    listings.retain(|listing| {
        if listing.snapshot.total_messages == 0 {
            return false;
        }
        if let Some(ref current) = current_id
            && listing.identifier() == *current
        {
            return false;
        }
        true
    });
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
    let mut metadata = preserve_prompt_cache_lineage_if_compatible(metadata, &source.snapshot.metadata);
    metadata.continuation_metadata = source.snapshot.metadata.continuation_metadata.clone();
    // Preserve the source session's primary agent ("mode") so a resume/fork keeps
    // running the same agent instead of falling back to the config default.
    if metadata.primary_agent.is_none() {
        metadata.primary_agent = source.snapshot.metadata.primary_agent.clone();
    }
    let mut bootstrap = ThreadBootstrap::from_listing(source.clone());
    bootstrap.metadata = Some(metadata.clone());

    let thread_id = match &intent {
        ArchivedSessionIntent::ResumeInPlace => source.identifier(),
        ArchivedSessionIntent::ForkNewArchive { custom_suffix, .. } => {
            if let Some(identifier) = reserved_identifier {
                identifier
            } else {
                reserve_session_archive_identifier(&metadata.workspace_label, custom_suffix.clone()).await?
            }
        }
    };

    if let ArchivedSessionIntent::ForkNewArchive { summarize, .. } = &intent {
        metadata.parent_session_id = Some(source.identifier());
        metadata.fork_mode = Some(if *summarize {
            SessionForkMode::Summarized
        } else {
            SessionForkMode::FullCopy
        });
        bootstrap.metadata = Some(metadata.clone());
    }

    let archive = match intent {
        ArchivedSessionIntent::ResumeInPlace => SessionArchive::resume_from_listing(&source, metadata),
        ArchivedSessionIntent::ForkNewArchive { .. } => {
            SessionArchive::new_with_identifier(metadata, thread_id.clone()).await?
        }
    };

    Ok(PreparedArchivedSession { source, workspace, bootstrap, thread_id, archive })
}

/// Fork a live thread into a standalone archive another process can own.
///
/// The canonical event log is single-writer: the CLI holds an exclusive
/// `flock` on `session.lock` for the whole session, so a second process
/// cannot resume the same thread id. Forking sidesteps that by giving the
/// receiver its own archive and its own (new) canonical log, so both sides
/// keep working independently.
///
/// Returns the fork's session id and the archive it was written to.
pub async fn fork_thread_for_handoff(
    listing: &SessionListing,
    workspace: PathBuf,
    metadata: SessionArchiveMetadata,
) -> Result<(String, SessionArchive)> {
    let prepared = prepare_archived_session(
        listing.clone(),
        workspace,
        metadata,
        ArchivedSessionIntent::ForkNewArchive { custom_suffix: None, summarize: false },
        None,
    )
    .await?;
    // The fork archive must exist on disk before the receiver looks it up:
    // `find_session_by_identifier` reads the persisted snapshot, so a fork
    // that was only prepared in memory would not be discoverable.
    //
    // The source snapshot is copied whole rather than rebuilt: `progress`
    // carries the loaded skills and turn number the receiver restores, and
    // `finalize` would drop that block in favor of the metadata fallback.
    let mut snapshot = listing.snapshot.clone();
    snapshot.metadata = prepared.archive.metadata().clone();
    prepared.archive.persist_snapshot(snapshot)?;
    Ok((prepared.thread_id, prepared.archive))
}

fn preserve_prompt_cache_lineage_if_compatible(
    mut metadata: SessionArchiveMetadata,
    source: &SessionArchiveMetadata,
) -> SessionArchiveMetadata {
    let is_compatible = metadata.workspace_path == source.workspace_path
        && metadata.provider == source.provider
        && metadata.model == source.model;
    if is_compatible && let Some(lineage_id) = source.prompt_cache_lineage_id.as_ref() {
        metadata.prompt_cache_lineage_id = Some(lineage_id.clone());
    }
    metadata
}

pub fn messages_from_session_listing(listing: &SessionListing) -> Vec<Message> {
    if !listing.snapshot.messages.is_empty() {
        listing.snapshot.messages.iter().map(Message::from).collect()
    } else if let Some(progress) = &listing.snapshot.progress
        && !progress.recent_messages.is_empty()
    {
        progress.recent_messages.iter().map(Message::from).collect()
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
    let workspace_label = workspace.file_name().and_then(|value| value.to_str()).unwrap_or("workspace");

    SessionArchiveMetadata::new(
        workspace_label,
        workspace.to_string_lossy().to_string(),
        model,
        provider,
        theme,
        reasoning_effort,
    )
    .ensure_prompt_cache_lineage_id()
}

#[cfg(test)]
mod tests;
