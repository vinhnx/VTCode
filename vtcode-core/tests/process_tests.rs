use std::time::Duration;

use anyhow::Result;

use vtcode_core::config::constants::shell;
use vtcode_core::utils::process::{ProcessRequest, run_process};

#[tokio::test]
async fn run_process_captures_stdout_and_stderr() -> Result<()> {
    let args = vec![
        "-c".to_string(),
        "printf ready && printf warn >&2".to_string(),
    ];
    let output = run_process(ProcessRequest {
        program: "sh",
        args: &args,
        display: "sh -c printf ready && printf warn >&2",
        current_dir: None,
        timeout: Duration::from_secs(5),
        stdin: None,
        max_stdout_bytes: shell::DEFAULT_MAX_STDOUT_BYTES,
        max_stderr_bytes: shell::DEFAULT_MAX_STDERR_BYTES,
    })
    .await?;

    assert!(output.success);
    assert_eq!(output.stdout, "ready");
    assert_eq!(output.stderr, "warn");
    assert!(!output.stdout_truncated);
    assert!(!output.stderr_truncated);
    assert!(!output.timed_out);

    Ok(())
}

#[tokio::test]
async fn run_process_truncates_when_limit_exceeded() -> Result<()> {
    let args = vec!["-c".to_string(), "printf 'abcdefghijklm'".to_string()];
    let output = run_process(ProcessRequest {
        program: "sh",
        args: &args,
        display: "sh -c printf 'abcdefghijklm'",
        current_dir: None,
        timeout: Duration::from_secs(5),
        stdin: None,
        max_stdout_bytes: 5,
        max_stderr_bytes: shell::DEFAULT_MAX_STDERR_BYTES,
    })
    .await?;

    assert_eq!(output.stdout, "abcde");
    assert!(output.stdout_truncated);
    assert_eq!(output.stdout_bytes, 5);

    Ok(())
}

#[tokio::test]
async fn run_process_returns_partial_output_on_timeout() -> Result<()> {
    let args = vec!["-c".to_string(), "printf 'start'; sleep 1".to_string()];
    let output = run_process(ProcessRequest {
        program: "sh",
        args: &args,
        display: "sh -c printf 'start'; sleep 1",
        current_dir: None,
        timeout: Duration::from_millis(100),
        stdin: None,
        max_stdout_bytes: shell::DEFAULT_MAX_STDOUT_BYTES,
        max_stderr_bytes: shell::DEFAULT_MAX_STDERR_BYTES,
    })
    .await?;

    assert!(output.timed_out);
    assert!(!output.stdout.is_empty());
    assert!(output.stdout.contains("start"));
    assert!(!output.success);

    Ok(())
}
