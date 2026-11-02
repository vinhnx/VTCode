use std::fmt::Write as _;
use std::path::Path;

use super::labels::{format_instruction_path, instruction_source_label};
use super::types::InstructionSegment;

pub fn extract_instruction_highlights(segments: &[InstructionSegment], limit: usize) -> Vec<String> {
    if limit == 0 {
        return Vec::new();
    }

    let mut highlights = Vec::with_capacity(limit);
    for segment in segments {
        let mut found_bullet = false;
        for line in segment.contents.lines() {
            if highlights.len() >= limit {
                break;
            }

            let trimmed = line.trim();
            if trimmed.starts_with('-') {
                let highlight = trimmed.trim_start_matches('-').trim();
                if !highlight.is_empty() {
                    highlights.push(highlight.to_string());
                    found_bullet = true;
                }
            }
        }

        if highlights.len() >= limit {
            break;
        }

        if found_bullet {
            continue;
        }

        for line in segment.contents.lines() {
            if highlights.len() >= limit {
                break;
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            let fallback = trimmed.trim_start_matches('#').trim();
            if !fallback.is_empty() {
                highlights.push(fallback.to_string());
                break;
            }
        }

        if highlights.len() >= limit {
            break;
        }
    }

    highlights
}

pub fn render_instruction_markdown(
    title: &str,
    segments: &[InstructionSegment],
    truncated: bool,
    project_root: &Path,
    home_dir: Option<&Path>,
    truncation_note: &str,
) -> String {
    let combined_len = segments.iter().map(|segment| segment.contents.len()).sum::<usize>();
    let mut section = String::with_capacity(combined_len.saturating_add(512));
    let _ = writeln!(section, "## {title}\n");

    // Full segment bodies are inlined below, so a "Key points" highlight list
    // is a pure echo of content the model already receives (AGENTS.md alone
    // re-printed ~1.2 KB of the first bullets). Skip it. The map and
    // precedence sentence only pay for themselves with multiple sources.
    let show_map = segments.len() > 1;
    if show_map {
        section.push_str(
            "Instructions are listed from lowest to highest precedence. When conflicts exist, defer to the later entries.\n\n",
        );
        section.push_str("### Instruction map\n");
        for (index, segment) in segments.iter().enumerate() {
            let _ = writeln!(
                section,
                "- {}. {} ({})",
                index + 1,
                format_instruction_path(&segment.source.path, project_root, home_dir),
                instruction_source_label(&segment.source),
            );
        }
    }

    if !segments.is_empty() {
        for (index, segment) in segments.iter().enumerate() {
            let _ = writeln!(
                section,
                "\n### {}. {} ({})\n",
                index + 1,
                format_instruction_path(&segment.source.path, project_root, home_dir),
                instruction_source_label(&segment.source),
            );
            section.push_str(segment.contents.trim());
            section.push('\n');
        }
    }

    if truncated && !truncation_note.is_empty() {
        let _ = writeln!(section, "\n_{truncation_note}_");
    }

    section.push('\n');
    section
}
