use super::*;

pub(crate) fn render_topic_file(topic: MemoryTopic, facts: &[GroundedFactRecord]) -> String {
    let mut out = format!("# {}\n\n{}\n", topic.title(), topic.description());
    if facts.is_empty() {
        out.push_str("\n- No saved facts yet.\n");
    } else {
        out.push('\n');
        for f in facts {
            let (_, src) = decode_topic_source(&f.source);
            let _ = writeln!(out, "- [{}] {}", src.trim(), f.fact);
        }
    }
    out
}

pub(crate) fn render_memory_index(
    preferences: &[GroundedFactRecord],
    repository_facts: &[GroundedFactRecord],
    notes: &[MemoryNoteSummary],
    pending_rollouts: usize,
) -> String {
    let mut highlights: Vec<_> =
        preferences.iter().chain(repository_facts.iter()).cloned().collect();
    let skip = highlights.len().saturating_sub(MEMORY_HIGHLIGHT_LIMIT);
    highlights = highlights.into_iter().skip(skip).collect();
    let mut out = String::from("# VT Code Memory Registry\n\n## Files\n");
    out.push_str("- `memory_summary.md`: Startup-injected summary for future sessions.\n");
    out.push_str("- `preferences.md`: Durable user preferences and workflow notes.\n");
    out.push_str(
        "- `repository-facts.md`: Grounded repository facts and recurring tooling notes.\n",
    );
    out.push_str("- `notes/`: User-authored durable notes available to the native memory tool.\n");
    out.push_str("- `rollout_summaries/`: Per-session evidence summaries.\n");
    let _ = write!(out, "\n## Rollout Status\n- Pending rollout summaries: {pending_rollouts}\n");
    out.push_str("\n## Highlights\n");
    if highlights.is_empty() {
        out.push_str("- No persistent notes yet.\n");
    } else {
        for f in &highlights {
            let (_, src) = decode_topic_source(&f.source);
            let _ = writeln!(out, "- [{}] {}", src.trim(), f.fact);
        }
    }
    if !notes.is_empty() {
        out.push_str("\n## Note Files\n");
        for n in notes {
            let _ = write!(out, "- `{}`", n.relative_path);
            if let Some(first) = n.highlights.first() {
                let _ = write!(out, ": {first}");
            }
            out.push('\n');
        }
    }
    out
}

pub(crate) fn render_memory_summary(
    preferences: &[GroundedFactRecord],
    repository_facts: &[GroundedFactRecord],
    notes: &[MemoryNoteSummary],
) -> String {
    let mut bullets: Vec<_> = preferences
        .iter()
        .chain(repository_facts.iter())
        .map(|f| f.fact.clone())
        .collect();
    bullets.extend(notes.iter().filter_map(|n| {
        n.highlights.first().map(|h| format!("Note ({}): {}", n.relative_path, h))
    }));
    let skip = bullets.len().saturating_sub(MEMORY_HIGHLIGHT_LIMIT);
    bullets = bullets.into_iter().skip(skip).collect();
    if bullets.is_empty() {
        bullets.push("No durable memory notes have been consolidated yet.".to_string());
    }
    render_memory_summary_bullets(&bullets)
}

pub(crate) fn render_memory_summary_bullets(bullets: &[String]) -> String {
    let mut out = String::from("# VT Code Memory Summary\n");
    for b in bullets {
        let _ = writeln!(out, "- {}", b.trim());
    }
    out
}

pub(crate) fn render_rollout_summary(classified: &ClassifiedFacts) -> String {
    let mut out =
        format!("# Rollout Summary\n\n- Generated: {}\n", chrono::Utc::now().to_rfc3339());
    if classified.total() == 0 {
        out.push_str("\n- No durable facts captured.\n");
    } else {
        out.push('\n');
        for f in classified.preferences.iter().chain(&classified.repository_facts) {
            let _ = writeln!(out, "- [{}] {}", f.source, f.fact);
        }
    }
    out
}

pub(crate) fn unique_rollout_id() -> String {
    let millis = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    format!("rollout-{millis}")
}
