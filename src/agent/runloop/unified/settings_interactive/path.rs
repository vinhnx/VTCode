use anyhow::{Context, Result, bail};
use toml::Value as TomlValue;

#[derive(Debug, Clone)]
pub(super) enum PathToken {
    Key(String),
    Index(usize),
}

pub(crate) fn parent_view_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    if path.ends_with(']')
        && let Some(start) = path.rfind('[')
    {
        let parent = &path[..start];
        return (!parent.is_empty()).then(|| parent.to_string());
    }

    path.rfind('.').map(|idx| path[..idx].to_string())
}

pub(super) fn parse_path_tokens(path: &str) -> Result<Vec<PathToken>> {
    let mut tokens = Vec::new();

    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }

        let mut rest = segment;
        loop {
            if let Some(index_start) = rest.find('[') {
                let key = &rest[..index_start];
                if !key.is_empty() {
                    tokens.push(PathToken::Key(key.to_string()));
                }

                let after_start = &rest[index_start + 1..];
                let Some(index_end) = after_start.find(']') else {
                    bail!(
                        "Invalid path segment '{}': missing closing bracket",
                        segment
                    );
                };

                let index_text = &after_start[..index_end];
                let index = index_text
                    .parse::<usize>()
                    .with_context(|| format!("Invalid array index '{}'", index_text))?;
                tokens.push(PathToken::Index(index));

                rest = &after_start[index_end + 1..];
                if rest.is_empty() {
                    break;
                }
            } else {
                tokens.push(PathToken::Key(rest.to_string()));
                break;
            }
        }
    }

    Ok(tokens)
}

pub(super) fn get_node<'a>(root: &'a TomlValue, path: &str) -> Option<&'a TomlValue> {
    let tokens = parse_path_tokens(path).ok()?;
    let mut current = root;

    for token in tokens {
        match token {
            PathToken::Key(key) => {
                let TomlValue::Table(table) = current else {
                    return None;
                };
                current = table.get(&key)?;
            }
            PathToken::Index(index) => {
                let TomlValue::Array(entries) = current else {
                    return None;
                };
                current = entries.get(index)?;
            }
        }
    }

    Some(current)
}

pub(super) fn get_node_mut<'a>(root: &'a mut TomlValue, path: &str) -> Option<&'a mut TomlValue> {
    let tokens = parse_path_tokens(path).ok()?;
    let mut current = root;

    for token in tokens {
        match token {
            PathToken::Key(key) => {
                let TomlValue::Table(table) = current else {
                    return None;
                };
                current = table.get_mut(&key)?;
            }
            PathToken::Index(index) => {
                let TomlValue::Array(entries) = current else {
                    return None;
                };
                current = entries.get_mut(index)?;
            }
        }
    }

    Some(current)
}
