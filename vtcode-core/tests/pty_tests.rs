use std::time::Duration;

use anyhow::Result;
use portable_pty::PtySize;
use tempfile::tempdir;

use vtcode_core::config::PtyConfig;
use vtcode_core::tools::{PtyCommandRequest, PtyManager};

#[tokio::test]
async fn run_pty_command_captures_output() -> Result<()> {
    let temp_dir = tempdir()?;
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), PtyConfig::default());

    let working_dir = manager.resolve_working_dir(Some("."))?;
    let request = PtyCommandRequest {
        command: vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf 'hello from pty'".to_string(),
        ],
        working_dir,
        timeout: Duration::from_secs(5),
        size: PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        },
    };

    let result = manager.run_command(request).await?;
    assert_eq!(result.exit_code, 0);
    assert!(result.output.contains("hello from pty"));

    Ok(())
}

#[test]
fn create_list_and_close_session_preserves_screen_contents() -> Result<()> {
    let temp_dir = tempdir()?;
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), PtyConfig::default());

    let working_dir = manager.resolve_working_dir(Some("."))?;
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    let session_id = "session-test".to_string();
    manager.create_session(
        session_id.clone(),
        vec![
            "sh".to_string(),
            "-c".to_string(),
            "printf ready && sleep 0.1".to_string(),
        ],
        working_dir,
        size,
    )?;

    std::thread::sleep(Duration::from_millis(150));

    let sessions = manager.list_sessions();
    assert_eq!(sessions.len(), 1);
    let snapshot = &sessions[0];
    assert_eq!(snapshot.id, session_id);
    assert!(
        snapshot
            .screen_contents
            .as_deref()
            .map(|contents| contents.contains("ready"))
            .unwrap_or(false)
    );
    assert!(
        snapshot
            .scrollback
            .as_deref()
            .map(|contents| contents.contains("ready"))
            .unwrap_or(false)
    );

    let closed = manager.close_session(&session_id)?;
    assert!(
        closed
            .screen_contents
            .as_deref()
            .map(|contents| contents.contains("ready"))
            .unwrap_or(false)
    );
    assert!(
        closed
            .scrollback
            .as_deref()
            .map(|contents| contents.contains("ready"))
            .unwrap_or(false)
    );

    Ok(())
}

#[test]
fn resolve_working_dir_rejects_missing_directory() {
    let temp_dir = tempdir().unwrap();
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), PtyConfig::default());

    let error = manager.resolve_working_dir(Some("missing"));
    assert!(error.unwrap_err().to_string().contains("does not exist"));
}

#[test]
fn session_input_roundtrip_and_resize() -> Result<()> {
    let temp_dir = tempdir()?;
    let mut config = PtyConfig::default();
    config.scrollback_lines = 200;
    let manager = PtyManager::new(temp_dir.path().to_path_buf(), config);

    let working_dir = manager.resolve_working_dir(Some("."))?;
    let size = PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    };

    let session_id = "roundtrip".to_string();
    manager.create_session(
        session_id.clone(),
        vec![
            "sh".to_string(),
            "-c".to_string(),
            "while read line; do if [ \"$line\" = \"exit\" ]; then break; fi; printf 'got:%s\\n' \"$line\"; done".to_string(),
        ],
        working_dir,
        size,
    )?;

    std::thread::sleep(Duration::from_millis(150));

    manager.send_input_to_session(&session_id, b"hello", true)?;
    std::thread::sleep(Duration::from_millis(150));

    let drained = manager.read_session_output(&session_id, true)?;
    let drained_text = drained.as_deref().expect("expected drained output");
    assert!(drained_text.contains("got:hello"));

    assert!(manager.read_session_output(&session_id, false)?.is_none());

    manager.send_input_to_session(&session_id, b"world", true)?;
    std::thread::sleep(Duration::from_millis(150));

    let peek = manager.read_session_output(&session_id, false)?;
    let peek_text = peek
        .as_deref()
        .expect("expected pending output")
        .to_string();
    assert!(peek_text.contains("got:world"));

    let drained_again = manager.read_session_output(&session_id, true)?;
    assert_eq!(drained_again.as_deref(), Some(peek_text.as_str()));

    let updated = manager.resize_session(
        &session_id,
        PtySize {
            rows: 48,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        },
    )?;
    assert_eq!(updated.rows, 48);
    assert_eq!(updated.cols, 120);

    let snapshot = manager.snapshot_session(&session_id)?;
    let scrollback = snapshot
        .scrollback
        .as_deref()
        .expect("scrollback should be present");
    assert!(scrollback.contains("got:hello"));
    assert!(scrollback.contains("got:world"));

    manager.close_session(&session_id)?;

    Ok(())
}
