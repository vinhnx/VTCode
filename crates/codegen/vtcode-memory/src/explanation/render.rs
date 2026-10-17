use super::*;
use std::fmt::Write;

fn line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn short(text: &str) -> String {
    line(&public_text(text, 240))
}
fn markdown_short(text: &str) -> String {
    let mut escaped = String::new();
    for character in short(text).chars() {
        if matches!(
            character,
            '\\' | '`'
                | '*'
                | '_'
                | '{'
                | '}'
                | '['
                | ']'
                | '('
                | ')'
                | '<'
                | '>'
                | '#'
                | '+'
                | '-'
                | '.'
                | '!'
                | '|'
                | '~'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}
fn evidence_link(e: &EvidenceRef) -> String {
    format!("[evidence](vtcode-evidence:{}:{}:{})", e.session_id, e.offset, e.digest)
}

fn graph_labels(model: &ExplanationModel) -> std::collections::BTreeMap<String, String> {
    let mut labels: std::collections::BTreeMap<_, _> = model
        .actions
        .iter()
        .chain(&model.plan_evolution)
        .chain(model.decisions.iter().map(|decision| &decision.fact))
        .map(|entry| (projection::graph_node_id(entry), short(&entry.label)))
        .collect();
    for edge in model.graph.iter().filter(|edge| edge.relation == "changed file") {
        labels.entry(edge.from.clone()).or_insert_with(|| "Recorded edit".into());
    }
    labels
}

/// Full public entries with canonical source links.
pub fn render_details(model: &ExplanationModel) -> String {
    let mut out =
        format!("Alt+click a fact to inspect its canonical evidence.\n{}", render_summary_with_labels(model, short));
    for (name, facts) in [
        ("Recorded actions", &model.actions),
        ("Captured changes", &model.changes),
        ("Plan evolution", &model.plan_evolution),
        ("Failures", &model.failures),
    ] {
        let _ = write!(out, "\n\n{name}\n");
        for f in facts {
            let _ = writeln!(out, "- {} — {} {}", line(&f.label), f.status, evidence_link(&f.evidence));
        }
    }
    for d in &model.decisions {
        let _ = writeln!(
            out,
            "\nDecision: {} {}\nAgent-reported rationale: {}\nAlternatives: {}",
            line(&d.fact.label),
            evidence_link(&d.fact.evidence),
            line(&d.rationale),
            d.alternatives.join("; ")
        );
    }
    for v in &model.verification {
        let _ = writeln!(
            out,
            "\nCheck: {} — {}; exit {:?}; fresh: {} {}",
            line(&v.fact.label),
            v.fact.status,
            v.exit_code,
            v.fresh,
            evidence_link(&v.fact.evidence)
        );
    }
    for r in &model.review_priorities {
        let _ = writeln!(
            out,
            "\nReview {:?}: {} — {} {}",
            r.priority,
            line(&r.fact.label),
            r.reason,
            evidence_link(&r.fact.evidence)
        );
    }
    for entry in &model.token_breakdowns {
        if let Ok(breakdown) = serde_json::to_string(&entry.breakdown) {
            let _ = writeln!(out, "\nRequest-prefix tokens: {breakdown} {}", evidence_link(&entry.fact.evidence));
        }
    }
    out
}

/// Five sections within twenty logical lines, including mandatory diagnostics.
pub fn render_summary(model: &ExplanationModel) -> String {
    render_summary_with_labels(model, markdown_short)
}

fn render_summary_with_labels(model: &ExplanationModel, short: fn(&str) -> String) -> String {
    let mut lines = vec![
        "Goal".into(),
        format!(
            "  {} — {}{}",
            model.goals.last().map_or("Goal unavailable".into(), |g| short(&g.label)),
            short(&model.status),
            model
                .goals
                .last()
                .map_or(String::new(), |g| format!(" [{}]", evidence_link(&g.evidence)))
        ),
        "Changes".into(),
    ];
    if model.changes.is_empty() {
        lines.push("  No successful file changes recorded.".into());
    } else {
        lines.push(format!("  {} distinct files; {} edit operations.", model.changes.len(), model.edit_operations));
        lines.extend(
            model
                .changes
                .iter()
                .take(2)
                .map(|c| format!("  {} ({}) [{}]", short(&c.label), c.status, evidence_link(&c.evidence))),
        );
    }
    lines.push("Decisions".into());
    lines.push(model.decisions.first().map_or("  Public rationale unavailable.".into(), |d| {
        format!(
            "  {} — {} (agent-reported) [{}]",
            short(&d.fact.label),
            short(&d.rationale),
            evidence_link(&d.fact.evidence)
        )
    }));
    lines.push("Verification".into());
    if model.verification.is_empty() {
        lines.push("  No verification command recorded.".into());
    } else {
        lines.extend(model.verification.iter().rev().take(2).map(|v| {
            format!(
                "  {}: {}{} [{}]",
                short(&v.fact.label),
                v.fact.status,
                if v.fact.status == "passed" && !v.fresh {
                    " (before a later mutation attempt)"
                } else {
                    ""
                },
                evidence_link(&v.fact.evidence)
            )
        }));
    }
    lines.push("Review first".into());
    if model.review_priorities.is_empty() {
        lines.push("  No review signal recorded; this is not a coverage assessment.".into());
    } else {
        lines.extend(model.review_priorities.iter().take(2).map(|r| {
            format!("  {:?}: {} — {} [{}]", r.priority, short(&r.fact.label), r.reason, evidence_link(&r.fact.evidence))
        }));
    }
    if let Some(f) = model.failures.last() {
        lines.push(format!(
            "Failures: {} recorded; latest: {} [{}]",
            model.failures.len(),
            short(&f.label),
            evidence_link(&f.evidence)
        ));
    }
    let c = &model.completeness;
    lines.push(format!(
        "Evidence: {} malformed, {} unknown, {} legacy, {} evicted turns.",
        c.malformed_records, c.unknown_records, c.legacy_records, c.evicted_turns
    ));
    let categorized_evidence: std::collections::BTreeSet<_> = model
        .verification
        .iter()
        .map(|check| &check.fact)
        .chain(model.failures.iter())
        .map(|entry| &entry.evidence)
        .collect();
    let omitted = model
        .actions
        .iter()
        .filter(|entry| !categorized_evidence.contains(&entry.evidence))
        .count()
        + model.goals.len().saturating_sub(1)
        + model.plan_evolution.len()
        + model.failures.len().saturating_sub(1)
        + model.changes.len().saturating_sub(2)
        + model.decisions.len().saturating_sub(1)
        + model.verification.len().saturating_sub(2)
        + model.review_priorities.len().saturating_sub(2);
    if omitted > 0 {
        lines.push(format!(
            "{omitted} {} omitted; use /explain --details.",
            if omitted == 1 { "entry" } else { "entries" }
        ));
    }
    if !c.warnings.is_empty() {
        lines.push(format!("Unavailable: {}", c.warnings.iter().map(|w| short(w)).collect::<Vec<_>>().join("; ")));
    }
    debug_assert!(lines.len() <= 20);
    lines.join("\n")
}

/// Width-aware recorded order, failures, and action-to-file relationships.
pub fn render_diagram(model: &ExplanationModel, width: usize) -> String {
    let width = width.max(8);
    let mut out = format!("Recorded execution: {}\n", short(&model.status));
    for (i, action) in model.actions.iter().enumerate() {
        let prefix = format!("{} {} ", i + 1, if action.status.contains("fail") { "x" } else { "->" });
        let label = format!("{} [{}]", line(&action.label), action.status);
        let _ = writeln!(out, "{prefix}{label}");
    }
    let labels = graph_labels(model);
    for edge in model.graph.iter().filter(|e| e.relation != "recorded next") {
        let from = labels.get(&edge.from).map_or(edge.from.as_str(), String::as_str);
        let to = labels.get(&edge.to).map_or(edge.to.as_str(), String::as_str);
        let _ = writeln!(out, "{} -> {} ({})", short(from), short(to), edge.relation);
    }
    out.push_str("Timing and ancestry are shown only when recorded.\n");
    let mut wrapped = String::new();
    for row in out.lines() {
        let mut remaining = row;
        while !remaining.is_empty() {
            let chunk = vtcode_commons::preview::truncate_to_display_width(remaining, width);
            wrapped.push_str(chunk);
            wrapped.push('\n');
            remaining = remaining.get(chunk.len()..).unwrap_or_default();
        }
    }
    wrapped
}

/// Self-contained offline report; evidence is embedded and all text is escaped.
pub fn render_html(model: &ExplanationModel, evidence: &[EvidencePage]) -> String {
    render_html_with_workspace(model, evidence, None)
}

/// Offline report with separately captured, unattributed current workspace state.
pub fn render_html_with_workspace(
    model: &ExplanationModel,
    evidence: &[EvidencePage],
    workspace: Option<&WorkspaceDiffSnapshot>,
) -> String {
    fn escape(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#39;")
    }
    fn fact(out: &mut String, entry: &ExplanationEntry, available: &std::collections::BTreeSet<&EvidenceRef>) {
        let _ = write!(out, "<p>{} — {}", escape(&entry.label), escape(&entry.status));
        if let Some(timestamp) = &entry.timestamp {
            let _ = write!(out, " <time>{}</time>", escape(timestamp));
        }
        if available.contains(&entry.evidence) {
            let _ = write!(out, " <a href=\"#evidence-{}\">evidence</a>", entry.evidence.offset);
        } else {
            out.push_str(" (evidence unavailable in this report)");
        }
        out.push_str("</p>");
    }
    let mut out = String::from(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\"><meta name=\"viewport\" content=\"width=device-width\"><meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\"><title>VT Code execution explanation</title><style>body{font:16px system-ui;margin:2rem auto;max-width:1000px;padding:0 1rem;color:#17212b;background:#fff}pre{white-space:pre-wrap;overflow-wrap:anywhere}details{border:1px solid #687686;padding:.7rem;margin:.6rem 0}a{color:#004d99}svg{max-width:100%;height:auto}text{font:14px monospace;fill:#17212b}</style><body><h1>Execution explanation</h1>",
    );
    let mut summary = escape(&render_summary_with_labels(model, short));
    for page in evidence {
        summary = summary.replace(
            &escape(&evidence_link(&page.reference)),
            &format!("<a href=\"#evidence-{}\">evidence</a>", page.reference.offset),
        );
    }
    let _ = write!(
        out,
        "<p>Scope: {:?}. Outcome: {}. Revision: {}</p><pre>{}</pre>",
        model.scope,
        escape(&model.status),
        escape(&model.revision),
        summary
    );
    let available = evidence.iter().map(|page| &page.reference).collect();
    out.push_str("<details><summary>Current workspace state</summary>");
    if let Some(workspace) = workspace {
        let _ = write!(out, "<p>Captured at {}. {}</p>", escape(&workspace.captured_at), escape(&workspace.note));
        if let Some(text) = &workspace.text {
            let _ = write!(out, "<pre>{}</pre>", escape(text));
            if text.is_empty() {
                out.push_str("<p>No tracked Git changes against HEAD at capture time.</p>");
            }
        }
        if workspace.truncated {
            out.push_str("<p>Current diff truncated at the snapshot limit.</p>");
        }
    } else {
        out.push_str("<p>Current workspace state was not captured in this report.</p>");
    }
    out.push_str("</details>");
    for (name, entries) in [
        ("Goals", &model.goals),
        ("Captured changes", &model.changes),
        ("Timeline", &model.actions),
        ("Failures", &model.failures),
        ("Plan evolution", &model.plan_evolution),
    ] {
        let _ = write!(out, "<details><summary>{name} ({})</summary>", entries.len());
        for entry in entries {
            fact(&mut out, entry, &available);
        }
        out.push_str("</details>");
    }
    out.push_str("<details><summary>Decisions</summary>");
    if model.decisions.is_empty() {
        out.push_str("<p>Public rationale unavailable.</p>");
    }
    for decision in &model.decisions {
        fact(&mut out, &decision.fact, &available);
        let _ = write!(out, "<p>Agent-reported rationale: {}</p>", escape(&decision.rationale));
        for alternative in &decision.alternatives {
            let _ = write!(out, "<p>Alternative: {}</p>", escape(alternative));
        }
    }
    out.push_str("</details><details><summary>Verification</summary>");
    for check in &model.verification {
        fact(&mut out, &check.fact, &available);
        let _ = write!(out, "<p>Exit code: {:?}. Fresh: {}.</p>", check.exit_code, check.fresh);
    }
    out.push_str("</details><details><summary>Review first</summary><p>Review signals are not proven bugs or coverage assessments.</p>");
    for signal in &model.review_priorities {
        fact(&mut out, &signal.fact, &available);
        let _ = write!(out, "<p>{:?}: {}</p>", signal.priority, escape(&signal.reason));
    }
    out.push_str("</details><details><summary>Agent tree</summary>");
    let mut seen = std::collections::BTreeSet::new();
    for entry in model.goals.iter().chain(&model.actions) {
        if let Some(actor) = &entry.actor_id
            && seen.insert((entry.task_id.as_ref(), actor))
        {
            let _ = write!(
                out,
                "<p>{} → {}</p>",
                escape(entry.parent_actor_id.as_deref().unwrap_or("Parent unavailable")),
                escape(actor)
            );
            fact(&mut out, entry, &available);
        }
    }
    if seen.is_empty() {
        out.push_str("<p>Ancestry unavailable.</p>");
    }
    out.push_str("</details><details><summary>Usage and timing</summary>");
    if let Some(usage) = &model.usage {
        let _ = write!(
            out,
            "<p>{} input; {} cached; {} cache creation; {} output tokens.</p>",
            usage.input_tokens, usage.cached_input_tokens, usage.cache_creation_tokens, usage.output_tokens
        );
    } else {
        out.push_str("<p>Usage unavailable.</p>");
    }
    if let Some(cost) = &model.cost_usd {
        let _ = write!(out, "<p>Recorded cost: ${cost}.</p>");
    } else {
        out.push_str("<p>Cost unavailable for this scope.</p>");
    }
    for entry in &model.token_breakdowns {
        fact(&mut out, &entry.fact, &available);
        if let Ok(json) = serde_json::to_string_pretty(&entry.breakdown) {
            let _ = write!(out, "<pre>{}</pre>", escape(&json));
        }
    }
    out.push_str("</details>");
    out.push_str("<h2>Recorded execution</h2>");
    let height = model.actions.len().saturating_mul(36).saturating_add(12);
    let _ = write!(out, "<svg role=\"img\" aria-label=\"Recorded execution order\" viewBox=\"0 0 960 {height}\">");
    for (i, a) in model.actions.iter().enumerate() {
        let y = i.saturating_mul(36) + 24;
        let _ = write!(
            out,
            "<text x=\"8\" y=\"{y}\">{} → {} [{}]</text>",
            i + 1,
            escape(&short(&a.label)),
            escape(&a.status)
        );
    }
    out.push_str("</svg><h2>Recorded action and file graph</h2>");
    let labels = graph_labels(model);
    let height = model.graph.len().saturating_mul(36).saturating_add(12);
    let _ = write!(
        out,
        "<svg role=\"img\" aria-label=\"Recorded action and file relationships\" viewBox=\"0 0 960 {height}\">"
    );
    for (index, edge) in model.graph.iter().enumerate() {
        let y = index.saturating_mul(36) + 24;
        let from = labels.get(&edge.from).map_or(edge.from.as_str(), String::as_str);
        let to = labels.get(&edge.to).map_or(edge.to.as_str(), String::as_str);
        let _ = write!(
            out,
            "<text x=\"8\" y=\"{y}\">{} → {} ({})</text>",
            escape(from),
            escape(to),
            escape(&edge.relation)
        );
    }
    out.push_str("</svg><h2>Evidence</h2>");
    for page in evidence {
        let _ = write!(
            out,
            "<details id=\"evidence-{}\"><summary>{}</summary><pre>{}</pre>{}</details>",
            page.reference.offset,
            escape(page.reference.item_id.as_deref().unwrap_or("Event")),
            escape(&page.text),
            if page.next_offset.is_some() {
                "<p>Evidence truncated in this report.</p>"
            } else {
                ""
            }
        );
    }
    out.push_str("<h2>Projected facts</h2><pre>");
    match serde_json::to_string_pretty(model) {
        Ok(json) => out.push_str(&escape(&json)),
        Err(_) => out.push_str("Projection serialization unavailable."),
    }
    out.push_str("</pre></body></html>");
    out
}
