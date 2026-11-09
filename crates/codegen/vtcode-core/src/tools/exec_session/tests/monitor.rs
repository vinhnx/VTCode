use super::super::{MONITOR_DEFAULT_MAX_BYTES, MONITOR_DEFAULT_MAX_LINES, MonitorDigest, MonitorSpec, MonitorState};
use super::*;

#[test]
fn monitor_spec_rejects_empty_and_oversized_filter() {
    assert!(MonitorSpec::new("", None, None).is_err());
    assert!(MonitorSpec::new("   ", None, None).is_err());
    assert!(MonitorSpec::new(&"x".repeat(257), None, None).is_err());
    assert!(MonitorSpec::new("error", None, None).is_ok());
    // Boundary: exactly the max length is accepted.
    assert!(MonitorSpec::new(&"x".repeat(256), None, None).is_ok());
}

#[test]
fn monitor_spec_applies_defaults_and_clamps_bounds() {
    let default = MonitorSpec::new("m", None, None).expect("valid filter");
    assert_eq!(
        (default.matches("m"), default.matches("")),
        (true, false),
        "defaults must preserve plain substring semantics"
    );
    // max_lines = 0 clamps to 1: the second match drops.
    let mut clamped_lines = MonitorState::new(MonitorSpec::new("m", Some(0), None).expect("valid filter"));
    clamped_lines.push_text("m one\nm two\n");
    let digest = clamped_lines.drain_digest().expect("matches arrived");
    assert_eq!(digest.lines.len(), 1);
    assert_eq!(digest.dropped_lines, 1);
    assert_eq!(digest.total_seen, 2);
    // max_bytes far below the floor clamps up: a 20-byte line still fits.
    let mut clamped_bytes = MonitorState::new(MonitorSpec::new("m", Some(500), Some(10)).expect("valid filter"));
    clamped_bytes.push_text("m 123456789012345678\n");
    let digest = clamped_bytes.drain_digest().expect("matches arrived");
    assert_eq!(digest.lines.len(), 1, "floor clamp must accept a 20-byte line: {digest:?}");
    assert_eq!(MONITOR_DEFAULT_MAX_LINES, 50);
    assert_eq!(MONITOR_DEFAULT_MAX_BYTES, 4_096);
}

#[test]
fn monitor_matches_case_sensitive_substring_in_either_order() {
    let spec = MonitorSpec::new("Error", None, None).expect("valid filter");
    let mut forward = MonitorState::new(spec.clone());
    forward.push_text("Error: boom\nerror: quiet\n");
    let mut reversed = MonitorState::new(spec);
    reversed.push_text("error: quiet\nError: boom\n");
    let forward_digest = forward.drain_digest().expect("forward matches");
    let reversed_digest = reversed.drain_digest().expect("reversed matches");
    assert_eq!(forward_digest.lines, vec!["Error: boom".to_string()]);
    assert_eq!(reversed_digest.lines, forward_digest.lines);
    assert_eq!(reversed_digest.total_seen, 1);
    // Substring, not word match.
    let mut prefix = MonitorState::new(MonitorSpec::new("Error", None, None).expect("valid filter"));
    prefix.push_text("Errors: plural\n");
    assert_eq!(prefix.drain_digest().expect("prefix matches").lines, vec!["Errors: plural".to_string()]);
}

#[test]
fn monitor_splits_partial_lines_identically_to_whole_writes() {
    let mut chunked = MonitorState::new(MonitorSpec::new("error", None, None).expect("valid filter"));
    chunked.push_text("err");
    assert!(chunked.peek_digest().is_none(), "partial line must not match early");
    chunked.push_text("or: boom\nok\n");
    let mut whole = MonitorState::new(MonitorSpec::new("error", None, None).expect("valid filter"));
    whole.push_text("error: boom\nok\n");
    assert_eq!(chunked.drain_digest(), whole.drain_digest());
    assert_eq!(whole.drain_digest(), None, "drain must consume so output never reappears");
}

#[test]
fn monitor_flush_delivers_final_unterminated_line() {
    let mut state = MonitorState::new(MonitorSpec::new("error", None, None).expect("valid filter"));
    state.push_text("error: tail");
    assert!(state.peek_digest().is_none());
    state.flush();
    assert_eq!(state.drain_digest().expect("flushed line").lines, vec!["error: tail".to_string()]);
    // Flushing twice must not duplicate.
    state.flush();
    assert!(state.peek_digest().is_none());
}

#[test]
fn monitor_caps_lines_and_reports_drop_counts() {
    let mut state = MonitorState::new(MonitorSpec::new("m", Some(2), Some(4_096)).expect("valid filter"));
    state.push_text("m one\nm two\nm three\n");
    let peeked = state.peek_digest().expect("matches arrived");
    assert_eq!(peeked.lines.len(), 2);
    assert_eq!(peeked.dropped_lines, 1);
    assert_eq!(peeked.total_seen, 3);
    assert!(peeked.render().contains("1 lines"), "drop tail must stay visible: {}", peeked.render());
    // Peek is idempotent; drain returns the same window exactly once.
    assert_eq!(state.peek_digest(), Some(peeked.clone()));
    assert_eq!(state.drain_digest(), Some(peeked));
    assert_eq!(state.drain_digest(), None);
}

#[test]
fn monitor_byte_cap_drops_oversized_line_but_keeps_window_signal() {
    let mut state = MonitorState::new(MonitorSpec::new("m", Some(500), Some(256)).expect("valid filter"));
    state.push_text(&format!("m {}\n", "x".repeat(300)));
    let digest = state.drain_digest().expect("window signal must survive a full drop");
    assert!(digest.lines.is_empty());
    assert_eq!(digest.dropped_lines, 1);
    assert_eq!(digest.total_seen, 1);
    assert!(digest.dropped_bytes > 0);
}

#[test]
fn monitor_redacts_secrets_and_neutralizes_terminal_controls() {
    let mut state = MonitorState::new(MonitorSpec::new("key", None, None).expect("valid filter"));
    let secret = concat!("sk-", "test1234567890abcdef");
    state.push_text(&format!("key leak: {secret}\nkey bel: \u{7} bidi: \u{202e}\n"));
    let digest = state.drain_digest().expect("matches arrived");
    assert_eq!(digest.lines.len(), 2);
    assert!(digest.lines[0].contains("[REDACTED_SECRET]"));
    assert!(!digest.lines[0].contains("test1234567890abcdef"));
    assert!(!digest.lines[1].chars().any(|c| c == '\u{7}' || c == '\u{202e}'));
    assert!(digest.lines[1].contains('\u{FFFD}'));
}

#[test]
fn monitor_truncates_long_multibyte_line_on_char_boundary() {
    let mut state = MonitorState::new(MonitorSpec::new("é", None, None).expect("valid filter"));
    state.push_text(&format!("{}\n", "é".repeat(600)));
    let digest = state.drain_digest().expect("matches arrived");
    assert_eq!(digest.lines.len(), 1);
    // `truncate_text` keeps max_len chars then appends the marker.
    assert_eq!(digest.lines[0], format!("{}…", "é".repeat(512)));
}

#[test]
fn monitor_render_joins_lines_and_appends_drop_tail() {
    let clean = MonitorDigest {
        lines: vec!["a".to_string(), "b".to_string()],
        dropped_lines: 0,
        dropped_bytes: 0,
        total_seen: 2,
    };
    assert_eq!(clean.render(), "a\nb");
    let truncated = MonitorDigest {
        lines: vec!["a".to_string(), "b".to_string()],
        dropped_lines: 3,
        dropped_bytes: 99,
        total_seen: 5,
    };
    assert_eq!(truncated.render(), "a\nb\n[monitor digest truncated: 3 lines, 99 bytes dropped]");
}

#[tokio::test]
#[cfg(unix)]
async fn pipe_session_monitor_digest_returns_only_matches() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    manager
        .create_pipe_session(
            "monitor-match".to_string().into(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "printf 'ok one\\nerror: bad\\nok two\\nerror: worse\\n'".to_string(),
            ],
            workspace_root.clone(),
            HashMap::new(),
        )
        .await?;
    manager
        .set_monitor_filter("monitor-match", MonitorSpec::new("error", None, None)?)
        .await?;

    let mut activity_rx = manager
        .activity_receiver("monitor-match")
        .await?
        .expect("pipe sessions should expose activity receiver");
    timeout(Duration::from_secs(2), activity_rx.changed()).await??;
    let digest = manager
        .read_monitor_digest("monitor-match", true)
        .await?
        .expect("matched lines should arrive");
    assert_eq!(digest.lines, vec!["error: bad".to_string(), "error: worse".to_string()]);
    assert_eq!(digest.dropped_lines, 0);
    assert_eq!(digest.total_seen, 2);

    let drained = manager.read_monitor_digest("monitor-match", true).await?;
    assert!(drained.is_none(), "drained digest must not reappear: {drained:?}");
    manager.close_session("monitor-match").await?;

    // Replacing the filter resets the window. A quiet session keeps this
    // deterministic: no output can race the re-attach.
    manager
        .create_pipe_session(
            "monitor-replace".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 5".to_string()],
            workspace_root,
            HashMap::new(),
        )
        .await?;
    manager
        .set_monitor_filter("monitor-replace", MonitorSpec::new("error", None, None)?)
        .await?;
    manager
        .set_monitor_filter("monitor-replace", MonitorSpec::new("ok", None, None)?)
        .await?;
    let replaced = manager.read_monitor_digest("monitor-replace", false).await?;
    assert!(replaced.is_none(), "replaced filter must reset the window: {replaced:?}");
    manager.close_session("monitor-replace").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn monitor_filter_on_missing_session_fails_closed() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root, pty_sessions);

    let result = manager
        .set_monitor_filter("no-such-session", MonitorSpec::new("error", None, None)?)
        .await;
    assert!(result.is_err());
    let digest = manager.read_monitor_digest("no-such-session", false).await;
    assert!(digest.is_err());
    Ok(())
}

#[tokio::test]
#[cfg(all(unix, feature = "tui"))]
async fn monitor_filter_on_pty_session_is_rejected() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    manager
        .create_pty_session(
            "monitor-pty".to_string().into(),
            vec!["/bin/sh".to_string(), "-c".to_string(), "sleep 1".to_string()],
            workspace_root,
            size,
            HashMap::new(),
            None,
        )
        .await?;

    let result = manager
        .set_monitor_filter("monitor-pty", MonitorSpec::new("error", None, None)?)
        .await;
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("pipe-only"));
    assert!(manager.read_monitor_digest("monitor-pty", false).await?.is_none());

    manager.close_session("monitor-pty").await?;
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn drain_ready_monitor_digests_collects_per_session_windows() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    for (id, marker) in [("wake-b", "bee"), ("wake-a", "aye")] {
        manager
            .create_pipe_session(
                id.to_string().into(),
                vec![
                    "/bin/sh".to_string(),
                    "-c".to_string(),
                    format!("printf 'noise\\n{marker}: hit\\n'"),
                ],
                workspace_root.clone(),
                HashMap::new(),
            )
            .await?;
        manager.set_monitor_filter(id, MonitorSpec::new(marker, None, None)?).await?;
        let mut activity_rx = manager.activity_receiver(id).await?.expect("pipe activity receiver");
        timeout(Duration::from_secs(2), activity_rx.changed()).await??;
    }
    // Unfiltered session: output arrives but no window exists.
    manager
        .create_pipe_session(
            "wake-plain".to_string().into(),
            vec![
                "/bin/sh".to_string(),
                "-c".to_string(),
                "printf 'aye: hit\\n'".to_string(),
            ],
            workspace_root,
            HashMap::new(),
        )
        .await?;

    let notes = manager.drain_ready_monitor_digests().await;
    assert_eq!(notes.len(), 2, "only filtered sessions with matches wake: {notes:?}");
    assert_eq!(notes[0].session_id, "wake-a", "session-id order must be stable");
    assert_eq!(notes[1].session_id, "wake-b");
    assert!(notes[0].text.contains("aye: hit"));
    assert!(notes[1].text.contains("bee: hit"));
    assert_eq!(notes[0].total_seen, 1);

    let redrained = manager.drain_ready_monitor_digests().await;
    assert!(redrained.is_empty(), "drained windows must not wake twice: {redrained:?}");

    for id in ["wake-a", "wake-b", "wake-plain"] {
        manager.close_session(id).await?;
    }
    Ok(())
}

#[tokio::test]
#[cfg(unix)]
async fn drain_ready_monitor_digests_caps_sessions_per_tick() -> anyhow::Result<()> {
    let temp_dir = tempdir()?;
    let workspace_root = canonicalize_workspace(temp_dir.path());
    let pty_sessions = PtySessionManager::new(workspace_root.clone(), PtyConfig::default());
    let manager = ExecSessionManager::new(workspace_root.clone(), pty_sessions);

    for index in 0..9 {
        let id = format!("cap-{index:02}");
        manager
            .create_pipe_session(
                id.clone().into(),
                vec!["/bin/sh".to_string(), "-c".to_string(), "printf 'hit\\n'".to_string()],
                workspace_root.clone(),
                HashMap::new(),
            )
            .await?;
        manager.set_monitor_filter(&id, MonitorSpec::new("hit", None, None)?).await?;
        let mut activity_rx = manager.activity_receiver(&id).await?.expect("pipe activity receiver");
        timeout(Duration::from_secs(2), activity_rx.changed()).await??;
    }

    let first = manager.drain_ready_monitor_digests().await;
    assert_eq!(first.len(), 8, "one tick drains at most the session cap");
    assert_eq!(first[0].session_id, "cap-00");
    let second = manager.drain_ready_monitor_digests().await;
    assert_eq!(second.len(), 1, "leftovers stay buffered for the next tick");
    assert_eq!(second[0].session_id, "cap-08");

    for index in 0..9 {
        manager.close_session(&format!("cap-{index:02}")).await?;
    }
    Ok(())
}

#[test]
fn monitor_state_holds_bounds_under_flood() {
    let mut state = MonitorState::new(MonitorSpec::new("hit", Some(50), Some(4_096)).expect("valid filter"));
    for index in 0..20_000 {
        // Asymmetric mix: matches, near-misses, multibyte, secrets, controls.
        match index % 5 {
            0 => state.push_text(&format!("hit line {index} with é multibyte\n")),
            1 => state.push_text("quiet line with no marker\n"),
            2 => state.push_text(&format!("hit {secret}\n", secret = concat!("sk-", "test1234567890abcdef"))),
            3 => state.push_text("hit bel \u{7} bidi \u{202e} end\n"),
            _ => state.push_text("HIT uppercase does not match\n"),
        }
    }
    let digest = state.drain_digest().expect("flood must leave a window");
    // 3 of every 5 lines match: 12,000 matched of 20,000 fed.
    assert_eq!(digest.total_seen, 12_000);
    assert_eq!(digest.lines.len(), 50, "line cap must hold exactly");
    assert_eq!(digest.dropped_lines, 12_000 - 50);
    assert!(digest.dropped_bytes > 0);
    for line in &digest.lines {
        assert!(!line.contains("test1234567890abcdef"), "every kept line stays redacted");
        assert!(!line.chars().any(|c| c == '\u{7}' || c == '\u{202e}'));
    }
    assert!(state.drain_digest().is_none(), "drain must reset flood counts");
}

#[test]
fn monitor_splits_carriage_return_progress_output() {
    let mut state = MonitorState::new(MonitorSpec::new("hit", None, None).expect("valid filter"));
    state.push_text("spin |\rspin /\rhit done\rspin -\n");
    let digest = state.drain_digest().expect("cr-separated match");
    assert_eq!(digest.lines, vec!["hit done".to_string()]);
    assert_eq!(digest.total_seen, 1);
}

#[test]
fn monitor_force_flushes_oversize_tail_without_losing_matches() {
    let mut state = MonitorState::new(MonitorSpec::new("needle", None, None).expect("valid filter"));
    state.push_text(&"x".repeat(70 * 1024));
    state.push_text("needle\n");
    let digest = state.drain_digest().expect("tail match survives force-flush");
    assert_eq!(digest.total_seen, 1);
    assert!(digest.lines.iter().any(|line| line.contains("needle")));
}

#[test]
fn monitor_spanning_match_reports_exactly_once() {
    let mut state = MonitorState::new(MonitorSpec::new("needle", None, None).expect("valid filter"));
    // Match bytes straddle the force-flush cut: "nee" stays in the overlap.
    state.push_text(&format!("{}nee", "x".repeat(70 * 1024)));
    state.push_text("dle\n");
    let digest = state.drain_digest().expect("spanning match");
    assert_eq!(digest.total_seen, 1, "spanning match must not duplicate: {digest:?}");
    assert_eq!(digest.lines.len(), 1);
}

#[test]
fn monitor_force_flush_cut_stays_on_char_boundaries() {
    let mut state = MonitorState::new(MonitorSpec::new("éé", None, None).expect("valid filter"));
    state.push_text(&format!("{}é", "x".repeat(70 * 1024)));
    state.push_text("é\n");
    let digest = state.drain_digest().expect("multibyte spanning match");
    assert_eq!(digest.total_seen, 1);
}
