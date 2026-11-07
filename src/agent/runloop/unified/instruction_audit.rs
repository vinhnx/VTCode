//! Read-only instruction audit behind `/doctor prompt-audit`.
//!
//! The setup checkup answers "is this installation healthy?". This audit
//! answers the complementary question: "is the guidance we always load still
//! earning its context budget?" It reports findings only and never mutates
//! files, mirroring `/doctor`'s confirm-before-apply contract.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use hashbrown::HashMap;
use tokio::sync::RwLock;

use vtcode_commons::estimate_tokens;
use vtcode_config::core::AgentConfig;
use vtcode_core::config::loader::VTCodeConfig;
use vtcode_core::instructions::{InstructionSourceKind, format_instruction_path, instruction_scope_label};
use vtcode_core::project_doc::load_instruction_appendix;
use vtcode_core::skills::types::Skill;
use vtcode_core::utils::ansi::{AnsiRenderer, MessageStyle};

/// Flag skill descriptions that bloat the always-loaded skill listing. The
/// stored maximum is 1024 bytes, so 512 is a "long but still valid" threshold.
const LONG_DESCRIPTION_WARN_BYTES: usize = 512;
/// Ignore short repeated lines; they are usually formatting, not shared guidance.
const SHARED_LINE_MIN_CHARS: usize = 60;
/// Report fenced code when it dominates the always-loaded guidance.
const FENCED_CODE_WARN_PERCENT: usize = 30;
const FENCED_CODE_WARN_MIN_LINES: usize = 20;

#[derive(Default)]
struct AuditCounts {
    passed: usize,
    warnings: usize,
}

struct SkillAudit {
    total: usize,
    description_tokens: usize,
    instruction_tokens: usize,
    long_descriptions: Vec<(String, usize)>,
}

impl SkillAudit {
    fn from_skills<'a>(skills: impl Iterator<Item = &'a Skill>) -> Self {
        let mut audit = Self {
            total: 0,
            description_tokens: 0,
            instruction_tokens: 0,
            long_descriptions: Vec::new(),
        };

        for skill in skills {
            audit.total += 1;
            let description = skill.manifest.description.trim();
            audit.description_tokens += estimate_tokens(description);
            audit.instruction_tokens += estimate_tokens(&skill.instructions);
            if description.len() > LONG_DESCRIPTION_WARN_BYTES {
                audit.long_descriptions.push((skill.manifest.name.clone(), description.len()));
            }
        }

        audit
            .long_descriptions
            .sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
        audit.long_descriptions.truncate(5);
        audit
    }
}

/// Render the instruction, skill, and MCP context audit.
pub(crate) async fn run_instruction_audit(
    renderer: &mut AnsiRenderer,
    vt_cfg: Option<&VTCodeConfig>,
    active_dir: &Path,
    match_paths: &[PathBuf],
    loaded_skills: &Arc<RwLock<HashMap<String, Skill>>>,
) -> Result<()> {
    let default_agent_cfg = AgentConfig::default();
    let agent_cfg = vt_cfg.map(|cfg| &cfg.agent).unwrap_or(&default_agent_cfg);
    let budget = agent_cfg.instruction_max_bytes;

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "═══════════════════════════════════════════════════════════════")?;
    renderer.line(MessageStyle::Status, "VT Code Doctor: Instruction Audit")?;
    renderer.line(MessageStyle::Status, "═══════════════════════════════════════════════════════════════")?;
    renderer.line(
        MessageStyle::Info,
        "Read-only audit of always-loaded guidance. Nothing is changed; apply fixes yourself.",
    )?;
    renderer.line(MessageStyle::Info, "")?;

    let Some(bundle) = load_instruction_appendix(agent_cfg, active_dir, match_paths).await else {
        renderer.line(MessageStyle::Warning, "!  No instruction files found for this workspace.")?;
        renderer.line(
            MessageStyle::Info,
            "   -> Add AGENTS.md (or CLAUDE.md) at the repository root with stable project conventions.",
        )?;
        renderer.line(
            MessageStyle::Info,
            "   -> Keep it short; move long procedures into `.agents/skills/<name>/SKILL.md` so they load on demand.",
        )?;
        render_audit_footer(renderer)?;
        return Ok(());
    };
    let Some(project_doc) = bundle.project_doc.as_ref() else {
        renderer.line(MessageStyle::Warning, "!  No workspace instruction files were loaded.")?;
        render_audit_footer(renderer)?;
        return Ok(());
    };

    let project_root = &bundle.project_root;
    let home_dir = bundle.home_dir.as_deref();
    let display_path = |path: &Path| format_instruction_path(path, project_root, home_dir);

    let mut counts = AuditCounts::default();

    renderer.line(MessageStyle::Status, "[Instruction Files]")?;
    for segment in &project_doc.segments {
        let source = &segment.source;
        renderer.line(
            MessageStyle::Output,
            &format!(
                "  • {} — {} ({}, {})",
                display_path(&source.path),
                format_bytes(segment.contents.len()),
                instruction_scope_label(&source.scope),
                source_kind_label(&source.kind),
            ),
        )?;
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Instruction Budget]")?;
    let budget_pct = project_doc.bytes_read.saturating_mul(100).checked_div(budget).unwrap_or(0);
    if project_doc.truncated {
        renderer.line(
            MessageStyle::Warning,
            &format!(
                "  !  Budget exceeded: {} of {} used ({budget_pct}%); instruction files were truncated",
                format_bytes(project_doc.bytes_read),
                format_bytes(budget),
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            "   -> Raise agent.instruction_max_bytes, split detail into skills, or use nested AGENTS.md files.",
        )?;
        counts.warnings += 1;
    } else if budget_pct >= 80 {
        renderer.line(
            MessageStyle::Warning,
            &format!(
                "  !  Near budget: {} of {} used ({budget_pct}%)",
                format_bytes(project_doc.bytes_read),
                format_bytes(budget),
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            "   -> Move rarely used procedures into skills or docs before the budget starts truncating.",
        )?;
        counts.warnings += 1;
    } else {
        renderer.line(
            MessageStyle::Status,
            &format!("  ✓  {} of {} used ({budget_pct}%)", format_bytes(project_doc.bytes_read), format_bytes(budget),),
        )?;
        counts.passed += 1;
    }

    let docs = project_doc
        .segments
        .iter()
        .map(|segment| (segment.source.path.clone(), segment.contents.clone()))
        .collect::<Vec<_>>();

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Duplication]")?;
    let duplicate_groups = duplicate_content_groups(&docs);
    if duplicate_groups.is_empty() {
        renderer.line(MessageStyle::Status, "  ✓  No instruction file duplicates another in full")?;
        counts.passed += 1;
    } else {
        for group in &duplicate_groups {
            let paths = group.iter().map(|path| display_path(path)).collect::<Vec<_>>().join(", ");
            renderer
                .line(MessageStyle::Warning, &format!("  !  Identical instruction content: {paths} — keep one copy"))?;
            counts.warnings += 1;
        }
    }

    let shared_lines = shared_long_lines(&docs);
    if shared_lines.is_empty() {
        renderer.line(MessageStyle::Status, "  ✓  No long guidance line is repeated across files")?;
        counts.passed += 1;
    } else {
        renderer.line(
            MessageStyle::Warning,
            &format!(
                "  !  {} long line(s) appear in more than one instruction file — de-duplicate or move them into a skill",
                shared_lines.len()
            ),
        )?;
        for (line, paths) in shared_lines.iter().take(3) {
            let paths = paths.iter().map(|path| display_path(path)).collect::<Vec<_>>().join(", ");
            renderer.line(MessageStyle::Output, &format!("      {} ({paths})", truncate_line(line, 96)))?;
        }
        counts.warnings += 1;
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Derivable Detail]")?;
    let (total_lines, fenced_lines) = fenced_code_stats(&docs);
    let fenced_pct = fenced_lines.saturating_mul(100).checked_div(total_lines).unwrap_or(0);
    if fenced_lines >= FENCED_CODE_WARN_MIN_LINES && fenced_pct >= FENCED_CODE_WARN_PERCENT {
        renderer.line(
            MessageStyle::Warning,
            &format!(
                "  !  {fenced_lines} of {total_lines} instruction line(s) ({fenced_pct}%) sit in fenced code blocks"
            ),
        )?;
        renderer.line(
            MessageStyle::Info,
            "   -> Long command/API examples are usually derivable from the codebase; move them into docs or a skill.",
        )?;
        counts.warnings += 1;
    } else {
        renderer.line(MessageStyle::Status, "  ✓  Little fenced example content is loaded on every turn")?;
        counts.passed += 1;
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Skills]")?;
    let skill_audit = {
        let skills = loaded_skills.read().await;
        SkillAudit::from_skills(skills.values())
    };
    if skill_audit.total == 0 {
        renderer.line(MessageStyle::Output, "  No skills loaded in this session")?;
    } else {
        renderer.line(
            MessageStyle::Status,
            &format!(
                "  ✓  {} loaded skill(s): ~{} always-loaded description token(s), ~{} on-demand body token(s)",
                skill_audit.total, skill_audit.description_tokens, skill_audit.instruction_tokens,
            ),
        )?;
        counts.passed += 1;

        for (name, bytes) in &skill_audit.long_descriptions {
            renderer.line(
                MessageStyle::Warning,
                &format!("  !  Skill '{name}' has a {bytes}-byte description; shorten it to keep the listing lean"),
            )?;
        }
        if !skill_audit.long_descriptions.is_empty() {
            counts.warnings += 1;
        }
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[MCP]")?;
    match vt_cfg {
        Some(cfg) if cfg.mcp.enabled => {
            let providers = cfg.mcp.providers.len();
            renderer.line(
                MessageStyle::Output,
                &format!("  {providers} configured MCP provider(s); disable unused servers to shrink the tool listing"),
            )?;
        }
        Some(_) => renderer.line(MessageStyle::Output, "  MCP is disabled")?,
        None => renderer.line(MessageStyle::Output, "  No MCP configuration loaded")?,
    }

    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "═══════════════════════════════════════════════════════════════")?;
    renderer.line(MessageStyle::Status, "[Summary]")?;
    renderer.line(MessageStyle::Status, &format!("  {} passed, {} warning(s)", counts.passed, counts.warnings))?;
    if counts.warnings == 0 {
        renderer.line(MessageStyle::Status, "[OK] Instructions are lean and non-duplicated.")?;
    } else {
        renderer.line(
            MessageStyle::Warning,
            "[WARN] Review the warnings above; this audit is read-only and changed nothing.",
        )?;
    }
    render_audit_footer(renderer)?;
    Ok(())
}

fn render_audit_footer(renderer: &mut AnsiRenderer) -> Result<()> {
    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "[Next Actions]")?;
    renderer.line(
        MessageStyle::Info,
        "[TIP] Run `/doctor` for the installation, provider, dependency, and MCP checkup.",
    )?;
    renderer.line(
        MessageStyle::Info,
        "[TIP] Split large root guidance into nested module AGENTS.md files (loaded only when working there).",
    )?;
    renderer.line(
        MessageStyle::Info,
        "[TIP] Move long procedures and references into `.agents/skills/<name>/SKILL.md`.",
    )?;
    renderer.line(MessageStyle::Info, "")?;
    renderer.line(MessageStyle::Status, "═══════════════════════════════════════════════════════════════")?;
    renderer.line(MessageStyle::Info, "")?;
    Ok(())
}

fn source_kind_label(kind: &InstructionSourceKind) -> &'static str {
    match kind {
        InstructionSourceKind::Agents => "instructions",
        InstructionSourceKind::Rule => "rule",
        InstructionSourceKind::Extra => "extra",
    }
}

fn format_bytes(bytes: usize) -> String {
    if bytes < 1_024 {
        format!("{bytes} B")
    } else {
        format!("{:.1} KiB", bytes as f64 / 1_024.0)
    }
}

fn truncate_line(line: &str, max_chars: usize) -> String {
    let mut chars = line.chars();
    let truncated = chars.by_ref().take(max_chars).collect::<String>();
    if chars.next().is_some() {
        format!("{truncated}…")
    } else {
        truncated
    }
}

/// Group instruction documents whose normalized contents are identical.
fn duplicate_content_groups(docs: &[(PathBuf, String)]) -> Vec<Vec<PathBuf>> {
    let mut groups: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for (path, contents) in docs {
        let normalized = contents.split_whitespace().collect::<Vec<_>>().join(" ").to_ascii_lowercase();
        if normalized.is_empty() {
            continue;
        }
        groups.entry(normalized).or_default().push(path.clone());
    }
    groups.into_values().filter(|paths| paths.len() > 1).collect()
}

/// Find long guidance lines repeated across more than one instruction file.
fn shared_long_lines(docs: &[(PathBuf, String)]) -> Vec<(String, Vec<PathBuf>)> {
    let mut lines: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for (path, contents) in docs {
        let mut seen_in_doc = std::collections::BTreeSet::new();
        for line in contents.lines() {
            let trimmed = line.trim();
            if !is_shared_line_candidate(trimmed) || !seen_in_doc.insert(trimmed.to_string()) {
                continue;
            }
            lines.entry(trimmed.to_string()).or_default().push(path.clone());
        }
    }

    lines.into_iter().filter(|(_, paths)| paths.len() > 1).collect()
}

fn is_shared_line_candidate(line: &str) -> bool {
    line.chars().count() >= SHARED_LINE_MIN_CHARS
        && !line.starts_with('#')
        && !line.starts_with("//")
        && !line.starts_with("<!--")
        && !line.starts_with("```")
        && !line.starts_with("~~~")
        && !line.starts_with('|')
}

/// Return `(total lines, fenced code lines)` across every instruction document.
fn fenced_code_stats(docs: &[(PathBuf, String)]) -> (usize, usize) {
    let mut total = 0;
    let mut fenced = 0;
    for (_, contents) in docs {
        let mut in_fence = false;
        for line in contents.lines() {
            total += 1;
            let trimmed = line.trim_start();
            if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
                in_fence = !in_fence;
                fenced += 1;
            } else if in_fence {
                fenced += 1;
            }
        }
    }
    (total, fenced)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, contents: &str) -> (PathBuf, String) {
        (PathBuf::from(path), contents.to_string())
    }

    #[test]
    fn identical_documents_are_grouped() {
        let docs = vec![
            doc("AGENTS.md", "Use four spaces.\nRun tests."),
            doc("nested/AGENTS.md", "use   four spaces.\nrun tests."),
            doc("other/AGENTS.md", "Different guidance entirely."),
        ];
        let groups = duplicate_content_groups(&docs);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0], vec![PathBuf::from("AGENTS.md"), PathBuf::from("nested/AGENTS.md")]);
    }

    #[test]
    fn shared_line_detection_ignores_headers_and_short_lines() {
        let repeated = "Always run the focused verifier before claiming the change is complete.";
        let docs = vec![
            doc("AGENTS.md", &format!("# Rules\n\n{repeated}\nshort")),
            doc("nested/AGENTS.md", &format!("# Rules\n\n{repeated}\nshort")),
        ];
        let shared = shared_long_lines(&docs);
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].0, repeated);
        assert_eq!(shared[0].1.len(), 2);
    }

    #[test]
    fn shared_line_detection_does_not_double_count_within_one_file() {
        let repeated = "This long line appears twice in one document and should still count once.";
        let docs = vec![
            doc("AGENTS.md", &format!("{repeated}\n{repeated}")),
            doc("nested/AGENTS.md", repeated),
        ];
        let shared = shared_long_lines(&docs);
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].1.len(), 2);
    }

    #[test]
    fn fenced_code_stats_track_fence_bodies() {
        let docs = vec![doc("AGENTS.md", "intro\n```bash\ncargo test\n```\noutro")];
        assert_eq!(fenced_code_stats(&docs), (5, 3));
    }

    #[test]
    fn formatting_helpers_are_stable() {
        assert_eq!(format_bytes(512), "512 B");
        assert_eq!(format_bytes(2_048), "2.0 KiB");
        assert!(estimate_tokens("body") > 0);
        assert_eq!(truncate_line("abcdef", 3), "abc…");
        assert_eq!(truncate_line("abc", 3), "abc");
    }

    #[test]
    fn duplicate_content_groups_ignore_blank_documents() {
        let docs = vec![
            doc("AGENTS.md", "  \n\t"),
            doc("nested/AGENTS.md", ""),
            doc("other/AGENTS.md", "Real guidance."),
        ];
        assert!(duplicate_content_groups(&docs).is_empty());
    }

    #[test]
    fn skill_audit_flags_long_descriptions() {
        let short = Skill::new(
            vtcode_core::skills::types::SkillManifest {
                name: "short".to_string(),
                description: "Short and discoverable.".to_string(),
                ..Default::default()
            },
            PathBuf::from("/skills/short"),
            "body".to_string(),
        )
        .expect("skill");

        let long = Skill::new(
            vtcode_core::skills::types::SkillManifest {
                name: "long".to_string(),
                description: "x".repeat(LONG_DESCRIPTION_WARN_BYTES + 1),
                ..Default::default()
            },
            PathBuf::from("/skills/long"),
            "body".to_string(),
        )
        .expect("skill");

        let audit = SkillAudit::from_skills([&short, &long].into_iter());
        assert_eq!(audit.total, 2);
        assert!(audit.description_tokens > 0);
        assert_eq!(audit.long_descriptions.len(), 1);
        assert_eq!(audit.long_descriptions[0].0, "long");
        assert_eq!(audit.instruction_tokens, estimate_tokens("body") * 2);
    }
}
