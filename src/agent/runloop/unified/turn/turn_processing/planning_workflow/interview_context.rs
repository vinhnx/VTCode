//! Planning workflow open-decision detection helpers.

pub(super) fn has_open_decision_markers(text: &str) -> bool {
    text.lines().any(line_has_open_decision_marker)
}

pub(super) fn line_has_open_decision_marker(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return false;
    }

    let lower = trimmed.to_ascii_lowercase();
    if !lower.contains("next open decision") {
        return false;
    }

    !contains_any(
        &lower,
        &[
            "none",
            "no remaining",
            "no further",
            "resolved",
            "closed",
            "locked",
            "n/a",
            "not applicable",
        ],
    )
}

pub(super) fn contains_any(text: &str, needles: &[&str]) -> bool {
    needles.iter().any(|needle| text.contains(needle))
}
