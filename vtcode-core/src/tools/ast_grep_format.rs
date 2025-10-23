use serde::Deserialize;
use serde_json::{Map, Value, json};
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
#[serde(untagged)]
enum AstGrepMatchLines {
    Object { text: String },
    Text(String),
}

impl AstGrepMatchLines {
    fn as_str(&self) -> &str {
        match self {
            AstGrepMatchLines::Object { text } => text,
            AstGrepMatchLines::Text(text) => text,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AstGrepMatchRecord {
    file: String,
    #[serde(default)]
    lines: Option<AstGrepMatchLines>,
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
                    .as_ref()
                    .map(|lines| lines.as_str())
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

pub(crate) fn extract_matches_with_metadata(
    value: Option<&Value>,
) -> (Vec<Value>, Map<String, Value>) {
    fn inner(value: Option<&Value>, metadata: &mut Map<String, Value>) -> Vec<Value> {
        if let Some(value) = value {
            match value {
                Value::Array(array) => return array.clone(),
                Value::Object(object) => {
                    for (key, val) in object {
                        if key != "results" && key != "matches" {
                            metadata.insert(key.clone(), val.clone());
                        }
                    }

                    if let Some(results) = object.get("results") {
                        let matches = inner(Some(results), metadata);
                        if !matches.is_empty() {
                            return matches;
                        }
                    }

                    if let Some(matches_value) = object.get("matches") {
                        let matches = inner(Some(matches_value), metadata);
                        if !matches.is_empty() {
                            return matches;
                        }
                    }
                }
                _ => {}
            }
        }
        Vec::new()
    }

    let mut metadata = Map::new();
    let matches = inner(value, &mut metadata);
    (matches, metadata)
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
