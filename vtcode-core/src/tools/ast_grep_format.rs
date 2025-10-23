use serde::Deserialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AstGrepPosition {
    line: usize,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AstGrepRange {
    start: AstGrepPosition,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AstGrepMatchRecord {
    file: String,
    #[serde(default)]
    lines: Option<String>,
    #[serde(default)]
    text: Option<String>,
    range: AstGrepRange,
}

/// Convert raw ast-grep matches into a concise representation with normalized
/// paths and 1-based line numbers.
pub(crate) fn matches_to_concise(matches: &[Value], workspace_root: &Path) -> Vec<Value> {
    let mut out = Vec::new();
    for value in matches {
        match serde_json::from_value::<AstGrepMatchRecord>(value.clone()) {
            Ok(record) => {
                let snippet_source = record
                    .lines
                    .as_deref()
                    .or_else(|| record.text.as_deref())
                    .unwrap_or("");

                out.push(json!({
                    "path": normalize_match_path(&record.file, workspace_root),
                    "line_number": record.range.start.line + 1,
                    "text": snippet_source.trim_end_matches(['\r', '\n']),
                }));
            }
            Err(err) => {
                out.push(json!({
                    "raw": value,
                    "error": format!("Failed to parse ast-grep match: {}", err),
                }));
            }
        }
    }
    out
}

fn normalize_match_path(original: &str, workspace_root: &Path) -> String {
    let original_path = Path::new(original);
    if original_path.is_absolute() {
        if let Ok(stripped) = original_path.strip_prefix(workspace_root) {
            return stripped.to_string_lossy().to_string();
        }
    }

    // Ensure relative paths retain consistent separators for downstream use.
    PathBuf::from(original)
        .components()
        .collect::<PathBuf>()
        .to_string_lossy()
        .to_string()
}
