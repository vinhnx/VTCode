use super::*;
use vtcode_commons::ui_protocol::ProgressOperation;

fn enabled() -> ProgramStatus {
    let mut status = ProgramStatus::default();
    status.apply(ProgramStatusUpdate::Configure { enabled: true });
    status
}

fn child(id: &str, kind: LocalAgentKind, state: ProgramState, updated_at: i64) -> LocalAgentEntry {
    LocalAgentEntry {
        id: id.into(),
        kind,
        program_status: state,
        updated_at,
        display_label: "secret command".into(),
        agent_name: "private agent".into(),
        status: "untrusted display state".into(),
        color: None,
        summary: Some("secret summary".into()),
        preview: "private path".into(),
        transcript_path: None,
    }
}

#[test]
fn program_status_opt_in_and_non_terminal_silence() {
    let mut status = ProgramStatus::default();
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    assert!(output.is_empty());
    status.apply(ProgramStatusUpdate::Configure { enabled: true });
    // A headless/test session has no attached terminal, even when opted in.
    status.flush();
    assert!(status.delivered.is_empty());
    status.deliver(&mut output).unwrap();
    assert!(String::from_utf8(output).unwrap().contains("state=idle"));
}

#[test]
fn program_status_owned_nested_waits_restore_latest_underlying_outcome() {
    let mut status = enabled();
    let operation = ProgressOperation::start();
    status.progress(ProgressUpdate::Begin { operation, phase: ProgressPhase::WaitingForModel });
    assert_eq!(status.desired()[&status.parent].state, ProgramState::Working);
    status.apply(ProgramStatusUpdate::Wait { token: 3, kind: InteractionKind::Permission });
    status.apply(ProgramStatusUpdate::Wait { token: 7, kind: InteractionKind::Question });
    status.apply(ProgramStatusUpdate::Resume { token: 99 });
    assert_eq!(status.desired()[&status.parent].kind, Some(InteractionKind::Question));
    status.apply(ProgramStatusUpdate::Resume { token: 3 });
    assert_eq!(status.desired()[&status.parent].state, ProgramState::Blocked);
    status.apply(ProgramStatusUpdate::Outcome(ProgramState::Error));
    status.apply(ProgramStatusUpdate::Resume { token: 7 });
    assert_eq!(status.desired()[&status.parent].state, ProgramState::Error);
    status.progress(ProgressUpdate::Finish { operation });
    status.activity(ActivityState::Idle);
    assert_eq!(status.desired()[&status.parent].state, ProgramState::Error);
}

#[test]
fn program_status_recovery_cancellation_and_stale_progress() {
    let mut status = enabled();
    status.activity(ActivityState::Planning);
    assert_eq!(status.desired()[&status.parent].state, ProgramState::Idle);
    let old = ProgressOperation::start();
    let current = ProgressOperation::start();
    status.progress(ProgressUpdate::Begin {
        operation: old,
        phase: ProgressPhase::PreparingContext,
    });
    status.activity(ActivityState::Recovery);
    assert_eq!(status.desired()[&status.parent].state, ProgramState::Working);
    status.activity(ActivityState::Blocked);
    assert_eq!(status.desired()[&status.parent].kind, None);
    status.progress(ProgressUpdate::Begin {
        operation: current,
        phase: ProgressPhase::RunningTools,
    });
    status.progress(ProgressUpdate::Finish { operation: old });
    status.progress(ProgressUpdate::Begin { operation: old, phase: ProgressPhase::Initializing });
    assert_eq!(status.desired()[&status.parent].message, "Running tools");
    status.apply(ProgramStatusUpdate::Outcome(ProgramState::Idle));
    status.progress(ProgressUpdate::Phase {
        operation: current,
        phase: ProgressPhase::Processing,
    });
    assert_eq!(status.desired()[&status.parent].state, ProgramState::Idle);
}

#[test]
fn program_status_typed_children_cap_order_stable_ids_and_removal() {
    let mut status = enabled();
    let mut entries = (0..65)
        .map(|n| child(&format!("secret-{n}"), LocalAgentKind::Background, ProgramState::Done, n))
        .collect::<Vec<_>>();
    entries.push(child("old-active", LocalAgentKind::Delegated, ProgramState::Working, -10));
    entries.push(child("recent-error", LocalAgentKind::ExecSession, ProgramState::Error, 100));
    entries.push(child("new-idle", LocalAgentKind::ExecSession, ProgramState::Idle, 1000));
    status.children(&entries);
    assert_eq!(status.desired().len(), 64);
    let active_id = format!("{}/{}", status.parent, record_segment("delegated", "old-active"));
    assert_eq!(status.children[&active_id].state, ProgramState::Working);
    let excluded_id = format!("{}/{}", status.parent, record_segment("background", "secret-0"));
    assert!(!status.children.contains_key(&excluded_id));
    let idle_id = format!("{}/{}", status.parent, record_segment("exec-session", "new-idle"));
    assert!(!status.children.contains_key(&idle_id));
    let expected = status.children.clone();
    entries.reverse();
    status.children(&entries);
    assert_eq!(status.children, expected);
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    let wire = String::from_utf8(output).unwrap();
    assert!(!wire.contains("secret"));
    assert!(!wire.contains("private"));
    status.children(&[]);
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    assert_eq!(String::from_utf8(output).unwrap().matches("state=clear").count(), 63);
    assert_eq!(status.delivered.len(), 1);
}

#[test]
fn program_status_dedup_and_disable_only_owned_subtree() {
    let mut status = enabled();
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    output.clear();
    status.deliver(&mut output).unwrap();
    assert!(output.is_empty());
    status.apply(ProgramStatusUpdate::Configure { enabled: false });
    status.deliver(&mut output).unwrap();
    let wire = String::from_utf8(output).unwrap();
    assert!(wire.contains(&format!("state=clear:id={}:app=vtcode", status.parent)));
    assert!(!wire.contains("state=clear:app="));
    assert!(status.delivered.is_empty());
    status.apply(ProgramStatusUpdate::Configure { enabled: true });
    status.deliver(&mut Vec::new()).unwrap();
    assert_eq!(status.delivered.len(), 1);
}

struct FailedFlush;
impl Write for FailedFlush {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Err(io::Error::other("delivery failed"))
    }
}

struct FailedWrite;
impl Write for FailedWrite {
    fn write(&mut self, _bytes: &[u8]) -> io::Result<usize> {
        Err(io::Error::other("write failed"))
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn program_status_failed_write_and_clear_retry() {
    let mut status = enabled();
    assert!(status.deliver(&mut FailedWrite).is_err());
    assert!(status.delivered.is_empty());
    assert!(status.deliver(&mut FailedFlush).is_err());
    assert!(status.delivered.is_empty());
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    assert!(!output.is_empty());
    status.apply(ProgramStatusUpdate::Configure { enabled: false });
    assert!(status.deliver(&mut FailedFlush).is_err());
    assert!(status.delivered.is_empty());
    output.clear();
    status.deliver(&mut output).unwrap();
    assert!(String::from_utf8(output).unwrap().contains("state=clear"));
}

#[test]
fn program_status_disable_clears_uncertain_delivery_without_caching_it() {
    let mut status = enabled();
    assert!(status.deliver(&mut FailedFlush).is_err());
    assert!(status.delivered.is_empty());
    status.apply(ProgramStatusUpdate::Configure { enabled: false });
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        format!("\x1b]7501;state=clear:id={}:app=vtcode:title=:msg=\x1b\\", status.parent)
    );
    assert!(status.attempted.is_empty());
    output = Vec::new();
    status.deliver(&mut output).unwrap();
    assert!(output.is_empty());
}

#[test]
fn program_status_removed_child_clears_uncertain_delivery() {
    let mut status = enabled();
    status.deliver(&mut Vec::new()).unwrap();
    status.children(&[child("uncertain", LocalAgentKind::Background, ProgramState::Working, 1)]);
    assert!(status.deliver(&mut FailedFlush).is_err());
    assert_eq!(status.delivered.len(), 1);
    status.children(&[]);
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    let wire = String::from_utf8(output).unwrap();
    assert!(wire.contains("state=clear:"));
    assert!(!wire.contains(&format!(":id={}:app=", status.parent)));
    assert_eq!(status.attempted.len(), 1);
    assert_eq!(status.delivered.len(), 1);
}

#[test]
fn program_status_failed_wait_delivery_republishes_restored_state() {
    let mut status = enabled();
    status.deliver(&mut Vec::new()).unwrap();
    status.apply(ProgramStatusUpdate::Wait { token: 10, kind: InteractionKind::Question });
    assert!(status.deliver(&mut FailedFlush).is_err());
    status.apply(ProgramStatusUpdate::Resume { token: 10 });
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    assert!(String::from_utf8(output).unwrap().contains("state=idle:"));
}

#[test]
fn program_status_failed_subtree_clear_republishes_children_on_enable() {
    let mut status = enabled();
    let entries = [child("finished", LocalAgentKind::Background, ProgramState::Done, 1)];
    status.children(&entries);
    status.deliver(&mut Vec::new()).unwrap();
    status.apply(ProgramStatusUpdate::Configure { enabled: false });
    assert!(status.deliver(&mut FailedFlush).is_err());
    status.apply(ProgramStatusUpdate::Configure { enabled: true });
    status.children(&entries);
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    let wire = String::from_utf8(output).unwrap();
    assert_eq!(wire.matches("\x1b]7501;").count(), 2);
    assert!(wire.contains("state=idle:"));
    assert!(wire.contains("state=done:"));
    assert_eq!(status.delivered.len(), 2);
}

#[test]
fn program_status_disabled_flush_skips_terminal_lock() {
    let mut status = ProgramStatus { terminal: true, ..Default::default() };
    // Disabled reporting must not join terminal I/O or restoration bookkeeping.
    let guard = super::super::panic_hook::lock_terminal_operations();
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || {
        status.flush();
        tx.send(()).unwrap();
        status
    });
    let result = rx.recv_timeout(std::time::Duration::from_secs(1));
    // Release and join even on regression, so a failed check cannot hang cleanup.
    drop(guard);
    let status = worker.join().unwrap();
    assert!(result.is_ok(), "disabled flush must return while terminal operations are locked: {result:?}");
    assert!(status.attempted.is_empty());
    assert!(status.delivered.is_empty());
}

#[test]
fn program_status_live_enable_projects_latest_session_children_and_waits() {
    use crate::tui::core_tui::{app::AppSession, app::types::InlineCommand, types::InlineTheme};

    let mut session = AppSession::new(InlineTheme::default(), None, 24);
    let configure = |enabled| InlineCommand::ProgramStatus(ProgramStatusUpdate::Configure { enabled });
    let update = |update| InlineCommand::ProgramStatus(update);
    let first = child("first", LocalAgentKind::Background, ProgramState::Working, 1);
    session.handle_command(InlineCommand::SetLocalAgents { entries: vec![first.clone()] });
    session.handle_command(update(ProgramStatusUpdate::Wait { token: 42, kind: InteractionKind::Auth }));
    session.handle_command(update(ProgramStatusUpdate::Outcome(ProgramState::Done)));
    let latest = vec![
        child("first", LocalAgentKind::Background, ProgramState::Error, 2),
        child("second", LocalAgentKind::Delegated, ProgramState::Working, 3),
    ];
    session.handle_command(InlineCommand::SetLocalAgents { entries: latest.clone() });
    assert!(session.core.program_status.children.is_empty());
    assert_eq!(session.core.local_agents, latest);

    session.handle_command(configure(true));
    let status = &mut session.core.program_status;
    assert_eq!(status.desired()[&status.parent].state, ProgramState::Blocked);
    assert_eq!(status.desired()[&status.parent].kind, Some(InteractionKind::Auth));
    assert_eq!(status.children.len(), 2);
    assert_eq!(
        status
            .children
            .values()
            .filter(|report| report.state == ProgramState::Error)
            .count(),
        1
    );
    status.deliver(&mut Vec::new()).unwrap();
    let prior_ids: Vec<_> = status.children.keys().cloned().collect();

    session.handle_command(configure(false));
    let status = &mut session.core.program_status;
    assert!(status.children.is_empty());
    assert!(status.deliver(&mut FailedFlush).is_err());
    assert!(!status.attempted.is_empty(), "failed disable must remain retryable");
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    assert!(String::from_utf8(output).unwrap().contains("state=clear:"));
    assert!(status.attempted.is_empty());

    // Keep parent ownership and the rendering snapshot current while disabled.
    session.handle_command(update(ProgramStatusUpdate::Resume { token: 42 }));
    session.handle_command(update(ProgramStatusUpdate::Outcome(ProgramState::Error)));
    session.handle_command(InlineCommand::SetLocalAgents { entries: vec![first] });
    session.handle_command(configure(true));
    let status = &mut session.core.program_status;
    assert_eq!(status.desired()[&status.parent].state, ProgramState::Error);
    assert_eq!(status.desired()[&status.parent].kind, None);
    assert_eq!(status.children.len(), 1);
    assert_eq!(status.children.values().next().unwrap().state, ProgramState::Working);
    assert!(prior_ids.contains(status.children.keys().next().unwrap()));
    assert!(status.delivered.is_empty(), "enable must publish a fresh subtree");
}

#[test]
fn program_status_shutdown_preserves_pending_finished_children_and_parent() {
    let mut status = enabled();
    status.children(&[
        child("done", LocalAgentKind::Background, ProgramState::Done, 1),
        child("busy", LocalAgentKind::Delegated, ProgramState::Working, 2),
    ]);
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    status.apply(ProgramStatusUpdate::Outcome(ProgramState::Error));
    status.closed = true;
    output.clear();
    status.deliver(&mut output).unwrap();
    assert_eq!(status.delivered.len(), 2);
    assert!(status.delivered.values().all(|report| report.state.is_finished()));
    let wire = String::from_utf8(output).unwrap();
    assert!(wire.contains("state=error"));
    assert_eq!(wire.matches("state=clear").count(), 1);
    status.deliver(&mut Vec::new()).unwrap();
}

#[test]
fn program_status_shutdown_does_not_clear_finished_child_with_parent() {
    let mut status = enabled();
    status.children(&[child("done", LocalAgentKind::Background, ProgramState::Done, 1)]);
    status.deliver(&mut Vec::new()).unwrap();
    status.closed = true;
    let mut output = Vec::new();
    status.deliver(&mut output).unwrap();
    assert_eq!(status.delivered.len(), 1);
    let wire = String::from_utf8(output).unwrap();
    assert!(wire.contains("state=idle"));
    assert!(!wire.contains("state=clear"));
}

#[test]
fn program_status_restoration_preserves_finished_descendants() {
    let records = BTreeMap::from([
        ("vt-owned".into(), ProgramState::Blocked),
        ("vt-owned/finished".into(), ProgramState::Error),
        ("vt-owned/running".into(), ProgramState::Working),
    ]);
    let mut output = Vec::new();
    retire_records(&records, &mut output);
    let wire = String::from_utf8(output).unwrap();
    assert!(wire.contains("state=idle:id=vt-owned:"));
    assert!(wire.contains("state=clear:id=vt-owned/running:"));
    assert!(!wire.contains("id=vt-owned/finished:"));
    assert!(!wire.contains("state=clear:id=vt-owned:"));
    let mut empty = Vec::new();
    retire_records(&BTreeMap::new(), &mut empty);
    assert!(empty.is_empty());
}

#[test]
fn program_status_commands_do_not_change_input_or_accessibility() {
    use crate::tui::core_tui::{
        session::Session,
        types::{InlineCommand, InlineTheme},
    };
    let mut session = Session::new(InlineTheme::default(), None, 24);
    let original_activity = session.activity_state;
    session.handle_command(InlineCommand::ProgramStatus(ProgramStatusUpdate::Configure { enabled: true }));
    session.handle_command(InlineCommand::ProgramStatus(ProgramStatusUpdate::Wait {
        token: 4,
        kind: InteractionKind::Auth,
    }));
    assert_eq!(session.activity_state, original_activity);
    assert!(!session.needs_animation_tick());
}
