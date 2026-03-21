use anyhow::{Context, Result, anyhow};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy)]
pub struct GhosttyRenderRequest {
    pub cols: u16,
    pub rows: u16,
    pub scrollback_lines: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhosttyRenderOutput {
    pub screen_contents: String,
    pub scrollback: String,
}

pub fn render_terminal_snapshot(
    request: GhosttyRenderRequest,
    vt_stream: &[u8],
) -> Result<GhosttyRenderOutput> {
    if vt_stream.is_empty() {
        return Ok(GhosttyRenderOutput {
            screen_contents: String::new(),
            scrollback: String::new(),
        });
    }

    let helper_path = ghostty_helper_path()
        .ok_or_else(|| anyhow!("Ghostty VT helper is unavailable; falling back to legacy_vt100"))?;
    let mut child = Command::new(helper_path)
        .arg(request.cols.to_string())
        .arg(request.rows.to_string())
        .arg(request.scrollback_lines.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| {
            format!(
                "failed to spawn Ghostty VT host helper at {}",
                helper_path.display()
            )
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(vt_stream)
            .context("failed to write VT stream to Ghostty helper")?;
    }

    let output = child
        .wait_with_output()
        .context("failed to wait for Ghostty helper")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!(
            "Ghostty helper exited with status {}: {}",
            output.status,
            stderr.trim()
        ));
    }

    parse_output(&output.stdout)
}

fn parse_output(bytes: &[u8]) -> Result<GhosttyRenderOutput> {
    let (screen_len, rest) = parse_len(bytes).context("missing screen length")?;
    if rest.len() < screen_len {
        return Err(anyhow!("Ghostty helper truncated screen payload"));
    }

    let (screen_bytes, rest) = rest.split_at(screen_len);
    let (scrollback_len, rest) = parse_len(rest).context("missing scrollback length")?;
    if rest.len() < scrollback_len {
        return Err(anyhow!("Ghostty helper truncated scrollback payload"));
    }

    let (scrollback_bytes, trailing) = rest.split_at(scrollback_len);
    if !trailing.is_empty() {
        return Err(anyhow!("Ghostty helper returned trailing bytes"));
    }

    Ok(GhosttyRenderOutput {
        screen_contents: String::from_utf8(screen_bytes.to_vec())
            .context("Ghostty helper returned invalid UTF-8 for screen contents")?,
        scrollback: String::from_utf8(scrollback_bytes.to_vec())
            .context("Ghostty helper returned invalid UTF-8 for scrollback")?,
    })
}

fn parse_len(bytes: &[u8]) -> Result<(usize, &[u8])> {
    if bytes.len() < 8 {
        return Err(anyhow!("expected 8-byte length prefix"));
    }

    let mut raw = [0u8; 8];
    raw.copy_from_slice(&bytes[..8]);
    let len = u64::from_le_bytes(raw)
        .try_into()
        .map_err(|_| anyhow!("length prefix does not fit into usize"))?;
    Ok((len, &bytes[8..]))
}

fn ghostty_helper_path() -> Option<&'static Path> {
    static HELPER_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    HELPER_PATH
        .get_or_init(|| {
            resolve_helper_path(
                std::env::var_os("VTCODE_GHOSTTY_VT_HOST").map(PathBuf::from),
                std::env::var_os("VTCODE_GHOSTTY_VT_DIR").map(PathBuf::from),
                std::env::current_exe().ok(),
                option_env!("VTCODE_GHOSTTY_VT_HOST_BUILD").filter(|value| !value.is_empty()),
            )
        })
        .as_deref()
}

fn resolve_helper_path(
    env_helper: Option<PathBuf>,
    env_dir: Option<PathBuf>,
    current_exe: Option<PathBuf>,
    build_helper: Option<&str>,
) -> Option<PathBuf> {
    candidate_helper_paths(env_helper, env_dir, current_exe, build_helper)
        .into_iter()
        .find(|path| path.is_file())
}

fn candidate_helper_paths(
    env_helper: Option<PathBuf>,
    env_dir: Option<PathBuf>,
    current_exe: Option<PathBuf>,
    build_helper: Option<&str>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::with_capacity(5);
    if let Some(path) = env_helper {
        candidates.push(path);
    }

    if let Some(dir) = env_dir {
        candidates.push(dir.join(helper_name()));
    }

    if let Some(exe) = current_exe
        && let Some(exe_dir) = exe.parent()
    {
        candidates.push(exe_dir.join("ghostty-vt").join(helper_name()));
        candidates.push(exe_dir.join(helper_name()));
    }

    if let Some(path) = build_helper {
        candidates.push(PathBuf::from(path));
    }

    candidates
}

fn helper_name() -> &'static str {
    if cfg!(windows) {
        "ghostty_vt_host.exe"
    } else {
        "ghostty_vt_host"
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::{
        GhosttyRenderOutput, GhosttyRenderRequest, candidate_helper_paths, parse_output,
        render_terminal_snapshot, resolve_helper_path,
    };

    #[test]
    fn empty_vt_stream_returns_empty_snapshot() {
        let output = render_terminal_snapshot(
            GhosttyRenderRequest {
                cols: 80,
                rows: 24,
                scrollback_lines: 1000,
            },
            &[],
        )
        .expect("empty VT stream should not require helper");

        assert_eq!(
            output,
            GhosttyRenderOutput {
                screen_contents: String::new(),
                scrollback: String::new(),
            }
        );
    }

    #[test]
    fn parse_output_decodes_length_prefixed_payloads() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(5u64).to_le_bytes());
        bytes.extend_from_slice(b"hello");
        bytes.extend_from_slice(&(5u64).to_le_bytes());
        bytes.extend_from_slice(b"world");

        let output = parse_output(&bytes).expect("valid payload should parse");
        assert_eq!(
            output,
            GhosttyRenderOutput {
                screen_contents: "hello".to_string(),
                scrollback: "world".to_string(),
            }
        );
    }

    #[test]
    fn parse_output_rejects_trailing_bytes() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&(1u64).to_le_bytes());
        bytes.extend_from_slice(b"a");
        bytes.extend_from_slice(&(1u64).to_le_bytes());
        bytes.extend_from_slice(b"b");
        bytes.extend_from_slice(b"extra");

        let error = parse_output(&bytes).expect_err("trailing bytes must be rejected");
        assert!(error.to_string().contains("trailing bytes"));
    }

    #[test]
    fn resolve_helper_prefers_env_override() {
        let temp_dir = tempdir().expect("tempdir");
        let helper = temp_dir.path().join("custom-helper");
        std::fs::write(&helper, b"").expect("helper file");

        let resolved = resolve_helper_path(Some(helper.clone()), None, None, None)
            .expect("env override should resolve");
        assert_eq!(resolved, helper);
    }

    #[test]
    fn candidate_helper_paths_include_sidecar_dir_next_to_binary() {
        let candidates = candidate_helper_paths(
            None,
            None,
            Some(PathBuf::from("/tmp/vtcode")),
            Some("/tmp/build-helper"),
        );

        assert!(
            candidates
                .iter()
                .any(|path| path == &PathBuf::from("/tmp/ghostty-vt").join(super::helper_name()))
        );
        assert!(
            candidates
                .iter()
                .any(|path| path == &PathBuf::from("/tmp/build-helper"))
        );
    }

    #[test]
    fn build_helper_renders_snapshot_when_available() {
        if option_env!("VTCODE_GHOSTTY_VT_HOST_BUILD")
            .filter(|value| !value.is_empty())
            .is_none()
        {
            return;
        }

        let output = render_terminal_snapshot(
            GhosttyRenderRequest {
                cols: 80,
                rows: 24,
                scrollback_lines: 1000,
            },
            b"hello from ghostty\r\n",
        )
        .expect("build helper should render VT stream");

        assert!(output.screen_contents.contains("hello from ghostty"));
    }
}
