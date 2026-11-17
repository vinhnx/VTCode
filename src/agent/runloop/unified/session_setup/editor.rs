use std::collections::{HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use anyhow::{Context, Result};
use tokio::sync::mpsc::error::TrySendError;
use tokio::sync::mpsc::{self, Receiver, Sender};
use vtcode_commons::{EditorTarget, resolve_editor_target};
use vtcode_core::config::loader::ConfigManager;
use vtcode_tui_runtime::terminal_setup::terminal_app::TerminalAppLauncher;
use vtcode_ui::tui::app::InlineHandle;

use crate::agent::runloop::unified::external_editor::run_with_event_loop_suspended_async;

use super::types::BackgroundTaskGuard;

const EDITOR_OPEN_REQUEST_CAPACITY: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct EditorOpenRequest {
    target: EditorTarget,
}

impl EditorOpenRequest {
    pub(crate) fn new(target: EditorTarget) -> Self {
        Self { target }
    }

    pub(crate) fn from_raw_target(raw: &str, workspace: &Path) -> Option<Self> {
        resolve_editor_target(raw, workspace).map(Self::new)
    }
}

pub(crate) type EditorOpenRequestSender = Sender<EditorOpenRequest>;

/// Synchronizes the two delivery paths for transcript/modal file-open
/// events so a click opens the editor exactly once.
///
/// The TUI event thread forwards `OpenFileInEditor` synchronously through
/// the session event callback (immediate even while an agent turn owns
/// `InlineSession.events`), while the same event instance is also queued on
/// the main channel and drained later by the idle loop. Each successful
/// immediate forward records a count keyed by raw target; the deferred
/// drain consumes one count instead of re-sending, which keeps the fallback
/// path (early boot before the coordinator sender exists, full-queue
/// retries, callback-less headless sessions) intact without double opens.
/// Counts are keyed by the raw event string so each event instance pairs
/// with its own deferred drain entry even when different spellings resolve
/// to the same file.
///
/// Counts (not a set) keep back-to-back clicks on the same target paired
/// exactly: two immediate forwards require two deferred skips.
pub(crate) struct EditorOpenDispatcher {
    sender_holder: Arc<OnceLock<EditorOpenRequestSender>>,
    callback_forwarded_counts: Mutex<HashMap<String, usize>>,
    allow_immediate: bool,
}

impl EditorOpenDispatcher {
    pub(crate) fn new(allow_immediate: bool) -> Self {
        Self {
            sender_holder: Arc::new(OnceLock::new()),
            callback_forwarded_counts: Mutex::new(HashMap::new()),
            allow_immediate,
        }
    }

    pub(crate) fn set_sender(&self, sender: EditorOpenRequestSender) {
        let _ = self.sender_holder.set(sender);
    }

    fn forwarded_counts(&self) -> MutexGuard<'_, HashMap<String, usize>> {
        self.callback_forwarded_counts
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Forward from the synchronous TUI event callback. Returns true when
    /// the request was handed to the coordinator (the deferred drain must
    /// skip this event instance). Returns false — leaving delivery to the
    /// deferred drain — when immediate opens are disabled (terminal editors
    /// needing TUI suspension), the target is unparsable, or no coordinator
    /// sender is installed yet.
    pub(crate) fn try_forward_immediate(&self, raw: &str, workspace: &Path) -> bool {
        if !self.allow_immediate {
            return false;
        }
        let Some(request) = EditorOpenRequest::from_raw_target(raw, workspace) else {
            return false;
        };
        let Some(sender) = self.sender_holder.get() else {
            return false;
        };
        match sender.try_send(request) {
            Ok(()) => {
                *self.forwarded_counts().entry(raw.to_string()).or_insert(0) += 1;
                true
            }
            Err(TrySendError::Full(_)) => {
                tracing::debug!("dropping immediate file-open request because coordinator queue is full");
                false
            }
            Err(TrySendError::Closed(_)) => {
                tracing::debug!("dropping immediate file-open request because coordinator is unavailable");
                false
            }
        }
    }

    /// Forward from the idle-loop drain of `InlineSession.events`. Skips the
    /// event instance when the callback already forwarded it (see
    /// [`Self::try_forward_immediate`]); otherwise sends through the
    /// provided coordinator sender.
    pub(crate) fn try_forward_deferred(&self, sender: &EditorOpenRequestSender, raw: &str, workspace: &Path) {
        let Some(request) = EditorOpenRequest::from_raw_target(raw, workspace) else {
            return;
        };
        let mut counts = self.forwarded_counts();
        if let Some(count) = counts.get_mut(raw) {
            if *count > 1 {
                *count -= 1;
            } else {
                counts.remove(raw);
            }
            tracing::debug!(target = raw, "skipping deferred file-open request already forwarded immediately");
            return;
        }
        drop(counts);
        match sender.try_send(request) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                tracing::debug!("dropping file-open request from TUI because coordinator queue is full");
            }
            Err(TrySendError::Closed(_)) => {
                tracing::debug!("dropping file-open request from TUI because coordinator is unavailable");
            }
        }
    }
}

type EditorOpenFuture = Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>>;

trait EditorOpenBackend: Send + Sync {
    fn open(&self, target: EditorTarget) -> EditorOpenFuture;
}

struct TerminalEditorOpenBackend {
    workspace: PathBuf,
    handle: InlineHandle,
}

impl TerminalEditorOpenBackend {
    fn new(workspace: PathBuf, handle: InlineHandle) -> Self {
        Self { workspace, handle }
    }
}

impl EditorOpenBackend for TerminalEditorOpenBackend {
    fn open(&self, target: EditorTarget) -> EditorOpenFuture {
        let workspace = self.workspace.clone();
        let handle = self.handle.clone();
        Box::pin(async move {
            let config_workspace = workspace.clone();
            let editor_config = tokio::task::spawn_blocking(move || {
                ConfigManager::load_from_workspace(config_workspace)
                    .map(|manager| manager.config().tools.editor.clone())
            })
            .await
            .context("editor configuration task failed")??;

            if !editor_config.enabled {
                tracing::debug!(target = %target.canonical_string(), "ignoring file-open request because external editors are disabled");
                return Ok(());
            }

            let preferred_editor =
                (!editor_config.preferred_editor.trim().is_empty()).then(|| editor_config.preferred_editor.clone());
            let suspend_tui = editor_config.suspend_tui
                && TerminalAppLauncher::editor_command_requires_terminal(preferred_editor.as_deref());
            let launcher = TerminalAppLauncher::new(workspace);

            run_with_event_loop_suspended_async(&handle, suspend_tui, || async move {
                tokio::task::spawn_blocking(move || launcher.launch_editor_target_non_waiting(target, preferred_editor))
                    .await
                    .map_err(|error| anyhow::anyhow!("editor launch task failed: {error}"))?
            })
            .await
            .context("failed to launch editor")
        })
    }
}

pub(crate) fn bounded_editor_open_requests() -> (EditorOpenRequestSender, Receiver<EditorOpenRequest>) {
    mpsc::channel(EDITOR_OPEN_REQUEST_CAPACITY)
}

pub(crate) struct EditorOpenCoordinator {
    receiver: Receiver<EditorOpenRequest>,
    pending: VecDeque<EditorTarget>,
    queued_targets: HashSet<String>,
    active_targets: HashSet<String>,
    backend: Arc<dyn EditorOpenBackend>,
}

impl EditorOpenCoordinator {
    fn new(receiver: Receiver<EditorOpenRequest>, backend: Arc<dyn EditorOpenBackend>) -> Self {
        Self {
            receiver,
            pending: VecDeque::new(),
            queued_targets: HashSet::new(),
            active_targets: HashSet::new(),
            backend,
        }
    }

    async fn next_pending_target(&mut self) -> Option<EditorTarget> {
        loop {
            if let Some(target) = self.pending.pop_front() {
                let key = target.canonical_string();
                self.queued_targets.remove(&key);
                self.active_targets.insert(key);
                return Some(target);
            }

            let request = self.receiver.recv().await?;
            self.enqueue(request.target);
        }
    }

    fn enqueue(&mut self, target: EditorTarget) {
        let key = target.canonical_string();
        if self.active_targets.contains(&key) || !self.queued_targets.insert(key) {
            return;
        }
        self.pending.push_back(target);
    }

    fn finish_target(&mut self, target: &EditorTarget) {
        // Drain duplicates that arrived while this launch was pending while
        // the target is still marked active. Requests arriving afterwards are
        // a new user action and may launch again.
        while let Ok(request) = self.receiver.try_recv() {
            self.enqueue(request.target);
        }
        self.active_targets.remove(&target.canonical_string());
    }

    async fn run(mut self) {
        while let Some(target) = self.next_pending_target().await {
            let launch_result = self.backend.open(target.clone()).await;
            if let Err(error) = launch_result {
                tracing::warn!(target = %target.canonical_string(), %error, "failed to open file from TUI");
            }
            self.finish_target(&target);
        }
    }
}

pub(crate) fn spawn_editor_open_coordinator(
    workspace: PathBuf,
    handle: &InlineHandle,
) -> (EditorOpenRequestSender, BackgroundTaskGuard) {
    let (sender, receiver) = bounded_editor_open_requests();
    let backend = Arc::new(TerminalEditorOpenBackend::new(workspace, handle.clone()));
    let coordinator = EditorOpenCoordinator::new(receiver, backend);
    let task = tokio::spawn(coordinator.run());
    (sender, BackgroundTaskGuard::new(task))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use vtcode_commons::EditorTarget;

    struct NoopBackend;

    impl EditorOpenBackend for NoopBackend {
        fn open(&self, _target: EditorTarget) -> EditorOpenFuture {
            Box::pin(async { Ok(()) })
        }
    }

    #[tokio::test]
    async fn coalesces_duplicate_targets_while_a_launch_is_pending() {
        let (sender, receiver) = bounded_editor_open_requests();
        let mut coordinator = EditorOpenCoordinator::new(receiver, Arc::new(NoopBackend));

        sender
            .send(EditorOpenRequest::new(EditorTarget::new(PathBuf::from("src/main.rs"), None)))
            .await
            .expect("send first request");
        sender
            .send(EditorOpenRequest::new(EditorTarget::new(PathBuf::from("src/main.rs"), None)))
            .await
            .expect("send duplicate request");
        drop(sender);

        assert_eq!(
            coordinator.next_pending_target().await,
            Some(EditorTarget::new(PathBuf::from("src/main.rs"), None))
        );
        coordinator.finish_target(&EditorTarget::new(PathBuf::from("src/main.rs"), None));
        assert_eq!(coordinator.next_pending_target().await, None);
    }

    #[tokio::test]
    async fn preserves_distinct_targets_in_bounded_request_order() {
        let (sender, receiver) = bounded_editor_open_requests();
        let mut coordinator = EditorOpenCoordinator::new(receiver, Arc::new(NoopBackend));

        sender
            .send(EditorOpenRequest::new(EditorTarget::new(PathBuf::from("src/main.rs"), None)))
            .await
            .expect("send first request");
        sender
            .send(EditorOpenRequest::new(EditorTarget::new(PathBuf::from("src/lib.rs"), None)))
            .await
            .expect("send second request");
        drop(sender);

        let first_target = EditorTarget::new(PathBuf::from("src/main.rs"), None);
        let second_target = EditorTarget::new(PathBuf::from("src/lib.rs"), None);
        assert_eq!(coordinator.next_pending_target().await, Some(first_target.clone()));
        coordinator.finish_target(&first_target);
        assert_eq!(coordinator.next_pending_target().await, Some(second_target.clone()));
        coordinator.finish_target(&second_target);
        assert_eq!(coordinator.next_pending_target().await, None);
    }

    #[test]
    fn deferred_drain_skips_event_instance_forwarded_immediately() {
        let workspace = PathBuf::from("/tmp");
        let (sender, mut receiver) = bounded_editor_open_requests();
        let dispatcher = EditorOpenDispatcher::new(true);
        dispatcher.set_sender(sender.clone());

        assert!(dispatcher.try_forward_immediate("/tmp/demo.rs", &workspace));
        dispatcher.try_forward_deferred(&sender, "/tmp/demo.rs", &workspace);

        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn deferred_drain_forwards_when_immediate_path_declined() {
        let workspace = PathBuf::from("/tmp");
        let (sender, mut receiver) = bounded_editor_open_requests();
        // Terminal editors keep the deferred path: the callback declines.
        let dispatcher = EditorOpenDispatcher::new(false);

        assert!(!dispatcher.try_forward_immediate("/tmp/demo.rs", &workspace));
        dispatcher.try_forward_deferred(&sender, "/tmp/demo.rs", &workspace);

        assert!(receiver.try_recv().is_ok());
    }

    #[test]
    fn back_to_back_clicks_on_same_target_pair_exactly() {
        let workspace = PathBuf::from("/tmp");
        let (sender, mut receiver) = bounded_editor_open_requests();
        let dispatcher = EditorOpenDispatcher::new(true);
        dispatcher.set_sender(sender.clone());

        assert!(dispatcher.try_forward_immediate("/tmp/demo.rs", &workspace));
        assert!(dispatcher.try_forward_immediate("/tmp/demo.rs", &workspace));
        dispatcher.try_forward_deferred(&sender, "/tmp/demo.rs", &workspace);
        dispatcher.try_forward_deferred(&sender, "/tmp/demo.rs", &workspace);

        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_ok());
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn immediate_forward_without_sender_falls_back_to_deferred() {
        let workspace = PathBuf::from("/tmp");
        let (sender, mut receiver) = bounded_editor_open_requests();
        let dispatcher = EditorOpenDispatcher::new(true);

        assert!(!dispatcher.try_forward_immediate("/tmp/demo.rs", &workspace));
        dispatcher.try_forward_deferred(&sender, "/tmp/demo.rs", &workspace);

        assert!(receiver.try_recv().is_ok());
    }
}
