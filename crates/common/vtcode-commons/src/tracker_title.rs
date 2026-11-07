//! Shared task-tracker checklist title resolution.
//!
//! Both the CLI transcript/panel and the desktop Tasks viewer resolve raw
//! checklist titles through these helpers so generated timestamp slugs never
//! surface as random names. String-based by design: CLI callers walk tool
//! payloads while desktop callers walk typed snapshots, and both converge
//! here for the humanization and fallback rules.
use crate::formatting::truncate_byte_budget;
use crate::slug::is_humanized_codename;

/// Byte budget for fallback titles derived from the first checklist item,
/// matching the header budget used at tracker creation.
pub const TRACKER_TITLE_ITEM_BUDGET: usize = 60;

/// Humanize generated tracker titles (`1789108823046-kind-lagoon` → `Kind
/// Lagoon`) so headers read as a name instead of a file-stem ID. Only
/// millisecond-timestamp-prefixed slugs are rewritten; user titles
/// (`Release`, paths, sentences) pass through verbatim.
#[must_use]
pub fn humanize_tracker_title(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "Task tracker".to_string();
    }
    let mut parts = trimmed.splitn(2, ['-', '_']);
    let prefix = parts.next().unwrap_or_default();
    let slug = parts.next().unwrap_or_default();
    let is_generated_id = prefix.len() >= 10
        && prefix.chars().all(|character| character.is_ascii_digit())
        && !slug.is_empty()
        && slug
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_');
    if !is_generated_id {
        return trimmed.to_string();
    }
    let humanized = slug
        .split(['-', '_'])
        .filter(|word| !word.is_empty())
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ");
    if humanized.is_empty() {
        trimmed.to_string()
    } else {
        humanized
    }
}

/// Descriptive header title: never surface a generated plan codename.
///
/// Checklist titles created from approved plans already carry the plan-summary
/// clause, but older/manual payloads may still carry a timestamp slug
/// (`1789108823046-jolly-forest`) or its humanized form (`Jolly Forest`).
/// Those read as random names and say nothing about the work, so derive a
/// purpose-indicating title from the first checklist item instead. User titles
/// pass through verbatim (humanized only for timestamp slugs).
#[must_use]
pub fn resolve_tracker_title(
    raw_title: Option<&str>,
    first_item_short: Option<&str>,
    fallback_default: &str,
) -> String {
    let raw = raw_title.map(str::trim).filter(|title| !title.is_empty());
    let Some(raw) = raw else {
        return descriptive_or_fallback(first_item_short, fallback_default);
    };
    if is_generated_tracker_title(raw) {
        return descriptive_or_fallback(first_item_short, fallback_default);
    }
    humanize_tracker_title(raw)
}

fn is_generated_tracker_title(raw: &str) -> bool {
    let trimmed = raw.trim();
    let mut parts = trimmed.splitn(2, ['-', '_']);
    let prefix = parts.next().unwrap_or_default();
    let slug = parts.next().unwrap_or_default();
    let is_timestamped =
        prefix.len() >= 10 && prefix.chars().all(|character| character.is_ascii_digit()) && !slug.is_empty();
    if is_timestamped {
        return true;
    }
    is_humanized_codename(&humanize_tracker_title(trimmed))
}

fn descriptive_or_fallback(first_item_short: Option<&str>, fallback_default: &str) -> String {
    first_item_short
        .map(str::trim)
        .filter(|short| !short.is_empty())
        .map(|short| truncate_byte_budget(short, TRACKER_TITLE_ITEM_BUDGET, "…"))
        .unwrap_or_else(|| fallback_default.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn humanize_rewrites_timestamp_slugs_only() {
        assert_eq!(humanize_tracker_title("1789108823046-kind-lagoon"), "Kind Lagoon");
        assert_eq!(humanize_tracker_title("1789108823046-kind_lagoon"), "Kind Lagoon");
        assert_eq!(humanize_tracker_title("Release"), "Release");
        assert_eq!(humanize_tracker_title("2024-report"), "2024-report");
        assert_eq!(humanize_tracker_title("  Task tracker  "), "Task tracker");
        assert_eq!(humanize_tracker_title(""), "Task tracker");
    }

    #[test]
    fn resolve_prefers_user_titles_then_first_item_then_fallback() {
        assert_eq!(resolve_tracker_title(Some("Release"), Some("Write code"), "Tasks"), "Release");
        assert_eq!(resolve_tracker_title(Some("1789108823046-kind-lagoon"), Some("Write code"), "Tasks"), "Write code");
        assert_eq!(resolve_tracker_title(None, Some("Write code"), "Tasks"), "Write code");
        assert_eq!(resolve_tracker_title(None, None, "Tasks"), "Tasks");
        assert_eq!(resolve_tracker_title(Some("   "), Some("  "), "Tasks"), "Tasks");
    }

    #[test]
    fn resolve_bounds_first_item_fallback_to_header_budget() {
        let long = "Rewrite the authentication flow end to end with tests";
        let resolved = resolve_tracker_title(None, Some(long), "Tasks");
        assert!(resolved.len() <= TRACKER_TITLE_ITEM_BUDGET + "…".len(), "{resolved}");
        assert!(resolved.starts_with("Rewrite the authentication"), "{resolved}");
    }
}
