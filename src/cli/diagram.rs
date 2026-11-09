use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};

/// Wall-clock budget per render, matching the desktop inline previews.
const RENDER_BUDGET: Duration = Duration::from_secs(10);

pub async fn handle_diagram_command(command: vtcode_core::cli::args::DiagramCommand) -> Result<()> {
    match command {
        vtcode_core::cli::args::DiagramCommand::Render(args) => {
            handle_diagram_render_command(&args.input, args.output).await
        }
    }
}

async fn handle_diagram_render_command(input: &Path, output: Option<PathBuf>) -> Result<()> {
    let content = std::fs::read_to_string(input).with_context(|| format!("reading {}", input.display()))?;
    let source: &str = if is_markdown_path(input) {
        vtcode_diagram::extract_mermaid_fence(&content)
            .with_context(|| format!("no mermaid diagram found in {}", input.display()))?
    } else {
        content.as_str()
    };
    if source.trim().is_empty() {
        anyhow::bail!("diagram source is empty");
    }
    let sandbox = vtcode_diagram::DiagramSandbox::new(vtcode_diagram::PELE_BUNDLE_JS)?;
    let rendered = sandbox
        .render(source, RENDER_BUDGET)
        .with_context(|| format!("rendering {}", input.display()))?;
    let out_path = output.unwrap_or_else(|| input.with_extension("svg"));
    std::fs::write(&out_path, rendered.svg.as_bytes()).with_context(|| format!("writing {}", out_path.display()))?;
    println!("Rendered {} ({}x{}) to {}", input.display(), rendered.width, rendered.height, out_path.display());
    Ok(())
}

fn is_markdown_path(path: &Path) -> bool {
    matches!(path.extension().and_then(|extension| extension.to_str()), Some(ext) if ext.eq_ignore_ascii_case("md") || ext.eq_ignore_ascii_case("markdown"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).expect("write fixture");
        path
    }

    #[tokio::test]
    async fn renders_mermaid_file_to_explicit_svg() {
        let dir = tempfile::tempdir().expect("temp dir");
        let input = fixture(dir.path(), "flow.mmd", "flowchart TD\n  A[Mermaid] --> B[Pele]\n");
        let output = dir.path().join("flow.svg");
        handle_diagram_render_command(&input, Some(output.clone()))
            .await
            .expect("renders");
        let svg = std::fs::read_to_string(&output).expect("read svg");
        assert!(svg.contains("<svg"), "svg element present");
        assert!(!svg.contains("<script"), "no script element");
        assert!(!svg.contains("<a "), "links:false drops anchors");
    }

    #[tokio::test]
    async fn defaults_output_beside_input_and_reads_markdown_fences() {
        let dir = tempfile::tempdir().expect("temp dir");
        let input =
            fixture(dir.path(), "notes.MD", "# Notes\n\n```mermaid\nsequenceDiagram\n  Alice->>Bob: Hello\n```\n");
        handle_diagram_render_command(&input, None).await.expect("renders");
        let svg = std::fs::read_to_string(dir.path().join("notes.svg")).expect("read default svg");
        assert!(svg.contains("<svg"), "svg element present");
    }

    #[tokio::test]
    async fn markdown_without_a_fence_fails_before_rendering() {
        let dir = tempfile::tempdir().expect("temp dir");
        let input = fixture(dir.path(), "notes.md", "# Notes\n\nno diagram here\n");
        let error = handle_diagram_render_command(&input, None).await.expect_err("must fail");
        assert!(error.to_string().contains("no mermaid diagram found"), "{error:?}");
        assert!(!dir.path().join("notes.svg").exists(), "no output written");
    }

    #[tokio::test]
    async fn invalid_source_reports_the_pele_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let input = fixture(dir.path(), "bad.mmd", "Hello\n");
        let error = handle_diagram_render_command(&input, None).await.expect_err("must fail");
        assert!(error.to_string().contains("rendering"), "{error:?}");
    }
}
