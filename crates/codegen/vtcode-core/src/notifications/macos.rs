use anyhow::Result;
use std::io::Write;
use std::process::{Command, Stdio};

pub(super) fn send_macos_osascript_notification(title: &str, message: &str) -> Result<()> {
    let mut child = Command::new("/usr/bin/osascript")
        .arg("-")
        .arg(message)
        .arg(title)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()?;

    let script = r#"on run argv
display notification (item 1 of argv) with title (item 2 of argv)
end run
"#;

    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| anyhow::anyhow!("failed to open osascript stdin"))?;
    stdin.write_all(script.as_bytes())?;
    drop(stdin);

    let output = child.wait_with_output()?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        if stderr.is_empty() {
            anyhow::bail!("osascript exited with status {}", output.status);
        }
        anyhow::bail!("osascript exited with status {}: {}", output.status, stderr);
    }
}
