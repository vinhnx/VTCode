//! Workspace selection, backend startup, and navigation history.

use super::*;

impl DesktopView {
    /// Remember a workspace as recently used.
    fn remember_workspace(&mut self, workspace: &Path) {
        self.preferences.touch_workspace(&workspace.display().to_string());
        self.preferences.save();
    }

    /// The saved recent workspaces for the sidebar.
    pub(super) fn recent_workspaces(&self) -> Vec<String> {
        self.preferences.recent_workspaces.clone()
    }

    /// Short display name for the active workspace: the final path component
    /// (lossy), or `None` when no workspace is set. Shared by the welcome,
    /// composer, and utilities call sites; the sidebar, title bar, and status
    /// bar keep their own `to_str` chains, which fall back to placeholders on
    /// non-UTF8 paths instead of showing a lossy name.
    pub(super) fn workspace_file_name(&self) -> Option<String> {
        self.workspace
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
    }

    pub(super) fn choose_workspace(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspace_picker_open {
            return;
        }
        if self.retained_submission.is_some() {
            self.error = Some(rust_i18n::t!("app.workspace.unsent_guard").to_string());
            cx.notify();
            return;
        }
        if !self.workspace_switch_idle() {
            self.error = Some(rust_i18n::t!("app.workspace.busy_guard").to_string());
            cx.notify();
            return;
        }
        // Panels stay out of the way of a workspace switch: the native picker
        // floats above everything, and the new workspace lands on a clean page.
        self.close_active_route(window, cx);

        self.workspace_picker_open = true;
        let paths = cx.prompt_for_paths(workspace_picker_options());
        cx.notify();
        self.workspace_picker_task = Some(cx.spawn_in(window, async move |view, cx| {
            let result = paths.await;
            let _updated = view.update_in(cx, |view, window, cx| {
                view.workspace_picker_open = false;
                match result {
                    Ok(Ok(Some(paths))) => {
                        if let Some(path) = paths.into_iter().next() {
                            view.select_workspace_from_ui(path, window, cx);
                        }
                    }
                    Ok(Err(error)) => {
                        view.error =
                            Some(rust_i18n::t!("app.workspace.picker_failed", error = error.to_string()).to_string())
                    }
                    Ok(Ok(None)) => {}
                    Err(_) => view.error = Some(rust_i18n::t!("app.workspace.picker_closed").to_string()),
                }
                cx.notify();
            });
        }));
    }

    /// Whether no workspace/session transition is in flight, regardless of
    /// turn activity. Shared by the switch gate (which also requires an idle
    /// turn) and the submit gate (which queues behind a running turn).
    pub(super) fn workspace_transition_idle(&self) -> bool {
        self.pending_workspace.is_none()
            && !self.workspace_selection_pending
            && self.session_request.is_none()
            && self.first_submission.is_none()
    }

    pub(super) fn workspace_switch_idle(&self) -> bool {
        !self.running && self.workspace_transition_idle()
    }

    fn current_navigation_location(&self) -> Option<NavigationLocation> {
        Some(NavigationLocation {
            workspace: self.workspace.clone()?,
            session_id: self.selected_session_id.clone(),
        })
    }

    pub(super) fn record_current_navigation(&mut self) {
        if let Some(location) = self.current_navigation_location() {
            self.navigation_history.record(location);
        }
    }

    /// History navigation deliberately stays on the idle-only gate: routing it
    /// through the running-switch modal would entangle `pending_navigation`
    /// with the armed wait, so mid-turn history stays parked (pre-existing
    /// behavior, kept).
    pub(super) fn can_navigate_back(&self) -> bool {
        self.navigation_history.back_index().is_some()
            && self.pending_navigation.is_none()
            && self.session_change_allowed(None)
    }

    pub(super) fn can_navigate_forward(&self) -> bool {
        self.navigation_history.forward_index().is_some()
            && self.pending_navigation.is_none()
            && self.session_change_allowed(None)
    }

    /// Direction of the in-flight history navigation, derived from the pending
    /// target relative to the current cursor. The cursor only moves when the
    /// pending navigation resolves, so the comparison is stable while pending.
    pub(super) fn pending_navigation_direction(&self) -> Option<NavigationDirection> {
        let pending = self.pending_navigation?;
        let current = self.navigation_history.current_index()?;
        if pending.target_index < current {
            Some(NavigationDirection::Back)
        } else if pending.target_index > current {
            Some(NavigationDirection::Forward)
        } else {
            None
        }
    }

    /// Whether a trackpad swipe or mouse back/forward button may navigate.
    /// History, pending-transition, and session guards live in
    /// `can_navigate_back`/`can_navigate_forward`; this adds the modal-overlay
    /// guard so a swipe over an open palette, settings, preview, or review
    /// cannot navigate the background underneath it.
    pub(super) fn swipe_navigation_allowed(&self) -> bool {
        self.current_route() == Route::Workspace
            && self.palette.is_none()
            && !self.config_page_open
            && self.config_editor.is_none()
            && self.file_preview.is_none()
    }

    pub(super) fn navigate_back(&mut self, cx: &mut Context<Self>) {
        if self.can_navigate_back()
            && let Some(index) = self.navigation_history.back_index()
        {
            self.navigate_to_history_index(index, cx);
        }
    }

    pub(super) fn navigate_forward(&mut self, cx: &mut Context<Self>) {
        if self.can_navigate_forward()
            && let Some(index) = self.navigation_history.forward_index()
        {
            self.navigate_to_history_index(index, cx);
        }
    }

    fn navigate_to_history_index(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(target) = self.navigation_history.location(index).cloned() else {
            return;
        };
        let workspace_switch = self.workspace.as_ref() != Some(&target.workspace);
        if workspace_switch {
            self.pending_navigation = Some(PendingNavigation { target_index: index, workspace_switch: true });
            self.select_workspace(target.workspace.clone(), cx);
            if self.workspace.as_ref() != Some(&target.workspace)
                && self.pending_workspace.as_ref() != Some(&target.workspace)
                && !self.workspace_selection_pending
            {
                self.pending_navigation = None;
            }
            return;
        }

        if target.session_id == self.selected_session_id {
            let _moved = self.navigation_history.navigate_to(index);
            cx.notify();
            return;
        }

        if let Some(session_id) = target.session_id {
            if !self.sessions.iter().any(|session| session.session_id == session_id) {
                self.error = Some(rust_i18n::t!("app.workspace.thread_unavailable").to_string());
                cx.notify();
                return;
            }
            self.pending_navigation = Some(PendingNavigation { target_index: index, workspace_switch: false });
            if !self.request_session_change(protocol::method::SESSION_LOAD, json!({ keys::SESSION_ID: session_id }), cx)
            {
                self.pending_navigation = None;
            }
        } else {
            let Some(session_id) = self.selected_session_id.clone() else {
                return;
            };
            self.pending_navigation = Some(PendingNavigation { target_index: index, workspace_switch: false });
            if !self.request_session_change(
                protocol::method::SESSION_CLOSE,
                json!({ keys::SESSION_ID: session_id }),
                cx,
            ) {
                self.pending_navigation = None;
            }
        }
    }

    pub(super) fn continue_pending_workspace_navigation(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.pending_navigation.filter(|pending| pending.workspace_switch) else {
            return;
        };
        let Some(target) = self.navigation_history.location(pending.target_index).cloned() else {
            self.fail_pending_navigation();
            return;
        };
        if self.workspace.as_ref() != Some(&target.workspace) {
            self.fail_pending_navigation();
            return;
        }
        let Some(session_id) = target.session_id else {
            self.pending_navigation = None;
            let _moved = self.navigation_history.navigate_to(pending.target_index);
            self.status = rust_i18n::t!("app.workspace.workspace_ready").to_string();
            return;
        };
        if !self.sessions.iter().any(|session| session.session_id == session_id) {
            self.error = Some(rust_i18n::t!("app.workspace.thread_unavailable").to_string());
            self.fail_pending_navigation();
            return;
        }
        if !self.request_session_change(protocol::method::SESSION_LOAD, json!({ keys::SESSION_ID: session_id }), cx) {
            self.fail_pending_navigation();
        }
    }

    pub(super) fn fail_pending_navigation(&mut self) {
        let Some(pending) = self.pending_navigation.take() else {
            return;
        };
        if pending.workspace_switch
            && let Some(location) = self.current_navigation_location()
        {
            let _replaced = self.navigation_history.replace_and_navigate_to(pending.target_index, location);
        }
    }

    pub(super) fn select_workspace_from_ui(&mut self, path: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        self.close_active_route(window, cx);
        self.select_workspace_with_invalid_path_feedback(path, true, cx);
        self.restore_focus_after_review_close(window, cx);
    }

    pub(super) fn select_workspace(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.select_workspace_with_invalid_path_feedback(path, true, cx);
    }

    pub(super) fn select_workspace_on_startup(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.select_workspace_with_invalid_path_feedback(path, false, cx);
    }

    fn select_workspace_with_invalid_path_feedback(
        &mut self,
        path: PathBuf,
        show_invalid_path_error: bool,
        cx: &mut Context<Self>,
    ) {
        // A turn may have started while the asynchronous picker was open.
        if self.retained_submission.is_some() {
            self.error = Some(rust_i18n::t!("app.workspace.unsent_guard").to_string());
            cx.notify();
            return;
        }
        if !self.workspace_switch_idle() {
            self.error = Some(rust_i18n::t!("app.workspace.busy_guard").to_string());
            cx.notify();
            return;
        }
        self.workspace_selection_pending = true;
        self.error = None;
        self.workspace_selection_task = Some(cx.spawn(async move |view, cx| {
            let canonical = cx
                .background_spawn(async move { files::canonicalize_workspace_folder(&path) })
                .await;
            let _updated = view.update(cx, |view, cx| {
                view.workspace_selection_pending = false;
                match canonical {
                    Some(canonical) => view.select_canonical_workspace(canonical, cx),
                    None => {
                        view.fail_pending_navigation();
                        if show_invalid_path_error {
                            view.error = Some(rust_i18n::t!("app.workspace.invalid_folder").to_string());
                        }
                    }
                }
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn select_canonical_workspace(&mut self, canonical: PathBuf, cx: &mut Context<Self>) {
        // Revalidate because a turn or session transition may start while the path is checked.
        if self.retained_submission.is_some() {
            self.fail_pending_navigation();
            self.error = Some(rust_i18n::t!("app.workspace.unsent_guard").to_string());
            return;
        }
        if !self.workspace_switch_idle() {
            self.fail_pending_navigation();
            self.error = Some(rust_i18n::t!("app.workspace.busy_guard").to_string());
            return;
        }
        if self.workspace.as_ref() == Some(&canonical) {
            self.continue_pending_workspace_navigation(cx);
            return;
        }
        if let Some(backend) = self.backend.as_ref() {
            self.pending_workspace = Some(canonical);
            self.status = rust_i18n::t!("app.workspace.shutting_down").to_string();
            if backend.commands.try_send(BackendCommand::Stop).is_err() {
                self.pending_workspace = None;
                self.fail_pending_navigation();
                self.error = Some(rust_i18n::t!(i18n::QUEUE_FULL).to_string());
            }
            cx.notify();
            return;
        }
        self.start_workspace(canonical, cx);
    }

    pub(super) fn start_workspace(&mut self, workspace: PathBuf, cx: &mut Context<Self>) {
        self.prepare_workspace(&workspace, cx);
        match launch_backend(workspace, self.event_tx.clone()) {
            Ok(backend) => self.initialize_backend(backend),
            Err(error) => self.report_service_launch_failure(error),
        }
        cx.notify();
    }

    pub(super) fn report_service_launch_failure(&mut self, error: String) {
        self.fail_pending_navigation();
        self.status = rust_i18n::t!("app.workspace.connect_failed").to_string();
        self.error = Some(format!("{error}\n\n{}", service_reconnect_guidance(self.retained_submission.is_some())));
    }

    pub(super) fn prepare_workspace(&mut self, workspace: &Path, cx: &mut Context<Self>) {
        // A same-workspace restart reloads CLI configuration without losing draft sources.
        let same_workspace = self.workspace.as_deref() == Some(workspace);
        self.reset_webmcp_state(None);
        let draft_sources = same_workspace.then(|| std::mem::take(&mut self.attachments));
        self.workspace = Some(workspace.to_path_buf());
        self.remember_workspace(workspace);
        self.pending_workspace = None;
        self.sessions.clear();
        self.selected_session_id = None;
        self.reset_session_state();
        if self.pending_navigation.is_none() {
            self.record_current_navigation();
        }
        if let Some(sources) = draft_sources {
            self.attachments = sources;
        }
        self.configuration = None;
        self.cli_config_settings = None;
        self.config_catalog_cache = None;
        self.config_search_corpus = None;
        self.config_editor = None;
        self.config_editor_error = None;
        self.config_write_pending = false;
        if !same_workspace {
            self.config_target = protocol::ConfigTarget::Workspace;
            self.config_write_notice = None;
        }
        self.service_info = None;
        self.pending_permission_choice = None;
        self.composer_resources.clear();
        self.slash_commands.clear();
        self.resources_loading = false;
        self.resources_loaded = false;
        self.active_turn_id = None;
        self.cancel_requested = false;
        self.cancel_request_sent = false;
        // Explicit though reset_session_state above already interrupts: the
        // armed wait must never survive a workspace start by ordering luck.
        self.interrupt_armed_running_switch();
        self.running = false;
        self.error = None;
        self.status = rust_i18n::t!("app.workspace.starting_service").to_string();
        if !same_workspace {
            self.files.clear();
            self.file_directory_paths = Arc::new(HashSet::new());
            self.expanded_file_directories.clear();
            self.file_tree.update(cx, |tree, cx| tree.set_items(Vec::new(), cx));
        }
        self.git_branch = None;
        self.preview_task = None;
        self.external_launch_task = None;
        self.refresh_files(cx);
        self.file_preview = None;
        self.models.clear();
    }

    pub(super) fn initialize_backend(&mut self, backend: BackendHandle) {
        let initialize = protocol::InitializeParams {
            protocol: protocol::ProtocolRange {
                minimum: protocol::PROTOCOL_VERSION,
                maximum: protocol::PROTOCOL_VERSION,
            },
            client_name: "VT Code Desktop".to_string(),
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        };
        if send_request(&backend.commands, protocol::method::INITIALIZE, json!(initialize)) {
            self.backend = Some(backend);
            // Reuse the session transition gate until the handshake is validated.
            self.session_request = Some(protocol::method::INITIALIZE);
        } else {
            self.report_service_launch_failure(rust_i18n::t!(i18n::REQUEST_QUEUE_UNAVAILABLE).to_string());
        }
    }
}
