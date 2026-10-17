//! Deterministic explanation command and private offline exports.
use super::{SlashCommandContext, SlashCommandControl};
use anyhow::{Context, Result, bail};
use vtcode_core::utils::ansi::MessageStyle;
use vtcode_memory::explanation::{ExplanationScope, render_diagram, render_html_with_workspace, render_summary};

#[derive(Default)]
struct Options {
    scope: ExplanationScope,
    details: bool,
    diagram: bool,
    web: bool,
    export: bool,
}

fn parse(args: &str) -> Result<Options> {
    let mut options = Options::default();
    let mut words = args.split_whitespace();
    while let Some(word) = words.next() {
        match word {
            "--scope" => {
                options.scope = match words.next() {
                    Some("task") => ExplanationScope::Task,
                    Some("session") => ExplanationScope::Session,
                    _ => bail!("scope must be task or session"),
                }
            }
            "--details" => options.details = true,
            "diagram" => options.diagram = true,
            "--web" => options.web = true,
            "--export" if words.next() == Some("html") => options.export = true,
            _ => bail!("Usage: /explain [--scope task|session] [--details|diagram|--web|--export html]"),
        }
    }
    if [options.details, options.diagram, options.web, options.export]
        .into_iter()
        .filter(|v| *v)
        .count()
        > 1
    {
        bail!("choose one explanation output mode");
    }
    Ok(options)
}

fn diagram_markdown(text: &str) -> String {
    let longest = text.split(|character| character != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.saturating_add(1).max(3));
    format!("{fence}text\n{text}\n{fence}")
}

pub(super) async fn handle_explain(ctx: SlashCommandContext<'_>, args: &str) -> Result<SlashCommandControl> {
    let options = match parse(args) {
        Ok(o) => o,
        Err(e) => {
            ctx.renderer.line(MessageStyle::Error, &e.to_string())?;
            return Ok(SlashCommandControl::Continue);
        }
    };
    let Some(emitter) = ctx.harness_emitter else {
        ctx.renderer
            .line(MessageStyle::Error, "Canonical evidence is unavailable for this session.")?;
        return Ok(SlashCommandControl::Continue);
    };
    let model = match emitter.explanation(options.scope).await {
        Ok(m) => m,
        Err(e) => {
            ctx.renderer
                .line(MessageStyle::Error, &format!("Explanation unavailable: {e}"))?;
            return Ok(SlashCommandControl::Continue);
        }
    };
    if options.web
        && let Some(bridge) = ctx.webmcp_bridge.as_ref()
    {
        let url = format!("{}/#explanation", bridge.pairing_origin().trim_end_matches('/'));
        if let Err(error) = webbrowser::open(&url) {
            ctx.renderer
                .line(MessageStyle::Warning, &format!("Browser opening failed: {error}. Open {url}"))?;
        }
    } else if options.export || options.web {
        let mut evidence = Vec::new();
        let mut report_model = model.clone();
        let mut bytes = 0usize;
        for reference in model.evidence_references() {
            let mut page = match emitter.evidence(reference, 0).await {
                Ok(page) => page,
                Err(_) => {
                    report_model
                        .completeness
                        .warnings
                        .push("Some canonical evidence expired or could not be read during export".into());
                    continue;
                }
            };
            while let Some(offset) = page.next_offset {
                if bytes.saturating_add(page.text.len()) >= 16 * 1024 * 1024 {
                    break;
                }
                let next = match emitter.evidence(page.reference.clone(), offset).await {
                    Ok(page) => page,
                    Err(_) => {
                        report_model
                            .completeness
                            .warnings
                            .push("An exported evidence record is incomplete".into());
                        break;
                    }
                };
                page.text.push_str(&next.text);
                page.next_offset = next.next_offset;
            }
            bytes = bytes.saturating_add(page.text.len());
            evidence.push(page);
            if bytes >= 16 * 1024 * 1024 {
                report_model
                    .completeness
                    .warnings
                    .push("Offline evidence exceeds 16 MiB; remaining evidence unavailable in report".into());
                break;
            }
        }
        let workspace = vtcode_core::git::capture_workspace_diff(&ctx.config.workspace).await;
        let html = render_html_with_workspace(&report_model, &evidence, Some(&workspace));
        let directory =
            vtcode_memory::session_directory(&ctx.config.workspace, &model.session_id).join("derived/explain");
        let path = directory.join(format!("explanation-{}.html", uuid::Uuid::new_v4()));
        let artifact = path.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            vtcode_commons::VtCodePaths::ensure_user_dir(&directory)?;
            vtcode_commons::VtCodePaths::write_private_file_atomic(&artifact, html.as_bytes())?;
            Ok(())
        })
        .await
        .context("explanation export task failed")??;
        ctx.renderer
            .line(MessageStyle::Info, &format!("Explanation report: {}", path.display()))?;
        if options.web
            && let Err(error) = webbrowser::open(&path.to_string_lossy())
        {
            ctx.renderer
                .line(MessageStyle::Warning, &format!("Browser opening failed: {error}; report remains available."))?;
        }
    } else if options.details {
        let mut text = vtcode_memory::explanation::render_details(&model);
        let workspace = vtcode_core::git::capture_workspace_diff(&ctx.config.workspace).await;
        text.push_str(&format!(
            "\n\nCurrent workspace state\nCaptured at {}. {}\n",
            workspace.captured_at, workspace.note
        ));
        if let Some(diff) = workspace.text {
            text.push_str(&diff);
        }
        if workspace.truncated {
            text.push_str("\nCurrent diff truncated at the snapshot limit.\n");
        }
        if !ctx.handle.review_evidence(text.lines().map(str::to_owned).collect()) {
            ctx.renderer
                .line(MessageStyle::Warning, "Transcript Review is unavailable; use /explain --export html.")?;
        }
    } else {
        let text = if options.diagram {
            diagram_markdown(&render_diagram(&model, crossterm::terminal::size().map_or(80, |(w, _)| w as usize)))
        } else {
            render_summary(&model)
        };
        ctx.renderer.render_markdown_output(MessageStyle::Info, &text)?;
    }
    Ok(SlashCommandControl::Continue)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explanation_modes_and_invalid_scope() {
        assert!(parse("").is_ok());
        assert!(parse("diagram --scope session").unwrap().diagram);
        assert!(parse("--export html").unwrap().export);
        assert!(parse("--web").unwrap().web);
        assert!(parse("--scope all").is_err());
        assert!(parse("--web --details").is_err());
        assert!(parse("--export").is_err());
    }
    #[test]
    fn diagram_fence_preserves_hostile_recorded_text_as_code() {
        let text = "recorded\n```\n[unsafe](https://example.test)\n````";
        let markdown = diagram_markdown(text);
        assert!(markdown.starts_with("`````text\n"));
        assert!(markdown.ends_with("\n`````"));
        assert!(markdown.contains(text));
    }
}
