use std::fmt::Write as _;

use anyhow::{Context, Result, bail};
use toml::Value as TomlValue;

#[derive(Debug, Clone)]
pub(super) enum PathToken {
    Key(String),
    Index(usize),
}

pub(crate) fn parent_view_path(path: &str) -> Option<String> {
    if let Some(parent) = super::secrets::parent_view(path) {
        return Some(parent.to_string());
    }
    if path.is_empty() {
        return None;
    }

    if path == super::SETTINGS_ADVANCED_VIEW_PATH {
        return None;
    }

    if let Some(group_view) = path.strip_prefix(super::SETTINGS_GROUP_PREFIX) {
        let (group_id, nested_path) = group_view.split_once(':')?;
        return Some(
            parent_view_path(nested_path)
                .map(|parent| format!("{}{group_id}:{parent}", super::SETTINGS_GROUP_PREFIX))
                .unwrap_or_else(|| format!("{}{group_id}", super::SETTINGS_GROUP_PREFIX)),
        );
    }

    if let Some(nested_path) = path.strip_prefix(super::SETTINGS_ADVANCED_NESTED_PREFIX) {
        return Some(
            parent_view_path(nested_path)
                .map(|parent| format!("{}{parent}", super::SETTINGS_ADVANCED_NESTED_PREFIX))
                .unwrap_or_else(|| super::SETTINGS_ADVANCED_VIEW_PATH.to_string()),
        );
    }

    if path.starts_with("advanced.") {
        return Some(super::SETTINGS_ADVANCED_VIEW_PATH.to_string());
    }

    parent_setting_path(path)
}

pub(super) fn parse_path_tokens(path: &str) -> Result<Vec<PathToken>> {
    let mut tokens = Vec::new();

    let mut position = 0;
    while position < path.len() {
        if path.as_bytes()[position] == b'.' {
            position += 1;
            continue;
        }

        if path.as_bytes()[position] == b'[' {
            tokens.push(parse_bracket_token(path, &mut position)?);
            continue;
        }

        let start = position;
        while position < path.len() {
            let character = path[position..]
                .chars()
                .next()
                .expect("position should remain on a character boundary");
            if matches!(character, '.' | '[') {
                break;
            }
            position += character.len_utf8();
        }
        if position > start {
            tokens.push(PathToken::Key(path[start..position].to_string()));
        }
    }

    Ok(tokens)
}

fn parse_bracket_token(path: &str, position: &mut usize) -> Result<PathToken> {
    debug_assert_eq!(path.as_bytes()[*position], b'[');
    *position += 1;

    if path.as_bytes().get(*position) == Some(&b'"') {
        *position += 1;
        let mut key = String::new();
        while *position < path.len() {
            let character = path[*position..]
                .chars()
                .next()
                .expect("position should remain on a character boundary");
            *position += character.len_utf8();

            match character {
                '"' => {
                    if path.as_bytes().get(*position) != Some(&b']') {
                        bail!("Invalid quoted path key: missing closing bracket");
                    }
                    *position += 1;
                    return Ok(PathToken::Key(key));
                }
                '\\' => {
                    let escaped = path[*position..]
                        .chars()
                        .next()
                        .ok_or_else(|| anyhow::anyhow!("Invalid quoted path key: dangling escape"))?;
                    *position += escaped.len_utf8();
                    key.push(escaped);
                }
                _ => key.push(character),
            }
        }
        bail!("Invalid quoted path key: missing closing quote")
    }

    let start = *position;
    while *position < path.len() && path.as_bytes()[*position] != b']' {
        *position += 1;
    }
    let Some(index_text) = path.get(start..*position) else {
        bail!("Invalid array index in path");
    };
    if *position == path.len() {
        bail!("Invalid path: missing closing bracket");
    }
    *position += 1;

    let index = index_text
        .parse::<usize>()
        .with_context(|| format!("Invalid array index '{index_text}'"))?;
    Ok(PathToken::Index(index))
}

pub(super) fn path_with_key(parent: &str, key: &str) -> String {
    let mut path = parent.to_string();
    append_key_path(&mut path, key);
    path
}

fn append_key_path(path: &mut String, key: &str) {
    if is_bare_key(key) {
        if !path.is_empty() {
            path.push('.');
        }
        path.push_str(key);
        return;
    }

    path.push_str("[\"");
    for character in key.chars() {
        match character {
            '\\' | '"' => {
                path.push('\\');
                path.push(character);
            }
            _ => path.push(character),
        }
    }
    path.push_str("\"]");
}

fn is_bare_key(key: &str) -> bool {
    !key.is_empty()
        && key
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
}

fn parent_setting_path(path: &str) -> Option<String> {
    let tokens = parse_path_tokens(path).ok()?;
    (tokens.len() > 1).then(|| format_path_tokens(&tokens[..tokens.len() - 1]))
}

fn format_path_tokens(tokens: &[PathToken]) -> String {
    let mut path = String::new();
    for token in tokens {
        match token {
            PathToken::Key(key) => append_key_path(&mut path, key),
            PathToken::Index(index) => {
                let _ = write!(path, "[{index}]");
            }
        }
    }
    path
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

pub(super) fn set_node(root: &mut TomlValue, path: &str, value: TomlValue) -> Result<()> {
    let tokens = parse_path_tokens(path)?;
    if tokens.is_empty() {
        bail!("Settings path '{path}' was not found");
    }

    set_node_tokens(root, &tokens, value, path)
}

fn set_node_tokens(current: &mut TomlValue, tokens: &[PathToken], value: TomlValue, path: &str) -> Result<()> {
    let Some(token) = tokens.first() else {
        *current = value;
        return Ok(());
    };

    match token {
        PathToken::Key(key) => {
            let TomlValue::Table(table) = current else {
                bail!("Settings path '{path}' traverses a non-table value");
            };

            if tokens.len() == 1 {
                table.insert(key.clone(), value);
                return Ok(());
            }

            let child = table.entry(key.clone()).or_insert_with(|| match tokens[1] {
                PathToken::Key(_) => TomlValue::Table(toml::map::Map::new()),
                PathToken::Index(_) => TomlValue::Array(Vec::new()),
            });
            set_node_tokens(child, &tokens[1..], value, path)
        }
        PathToken::Index(index) => {
            let TomlValue::Array(entries) = current else {
                bail!("Settings path '{path}' traverses a non-array value");
            };
            let entry = entries
                .get_mut(*index)
                .ok_or_else(|| anyhow::anyhow!("Settings path '{path}' references a missing array item"))?;
            set_node_tokens(entry, &tokens[1..], value, path)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_node_updates_nested_array_value() {
        let mut root: TomlValue = toml::from_str(
            r#"
            [[hooks.lifecycle.pre_tool_use]]
            hooks = ["before"]
            "#,
        )
        .expect("valid TOML");

        set_node(&mut root, "hooks.lifecycle.pre_tool_use[0].hooks[0]", TomlValue::String("after".to_string()))
            .expect("nested array value should be updated");

        assert_eq!(
            get_node(&root, "hooks.lifecycle.pre_tool_use[0].hooks[0]").and_then(TomlValue::as_str),
            Some("after")
        );
    }

    #[test]
    fn set_node_creates_missing_tables() {
        let mut root = TomlValue::Table(toml::Table::new());

        set_node(&mut root, "agent.small_model.model", TomlValue::String("small-model".to_string()))
            .expect("missing tables should be created");

        assert_eq!(get_node(&root, "agent.small_model.model").and_then(TomlValue::as_str), Some("small-model"));
    }

    #[test]
    fn quoted_map_keys_round_trip_through_path_operations() {
        let mut root: TomlValue = toml::from_str(
            r#"
            [profiles."gpt-5.4"]
            temperature = 0.2
            "#,
        )
        .expect("valid TOML");

        let path = r#"profiles["gpt-5.4"].temperature"#;
        assert_eq!(get_node(&root, path).and_then(TomlValue::as_float), Some(0.2));
        assert_eq!(
            parent_setting_path(r#"profiles["gpt-5.4"].temperature"#),
            Some(r#"profiles["gpt-5.4"]"#.to_string())
        );

        set_node(&mut root, path, TomlValue::Float(0.7)).expect("quoted map key should be writable");
        assert_eq!(get_node(&root, path).and_then(TomlValue::as_float), Some(0.7));

        let unicode_tokens = parse_path_tokens("profiles.模型").expect("unicode path should parse");
        assert!(matches!(
            unicode_tokens.as_slice(),
            [PathToken::Key(root), PathToken::Key(key)] if root == "profiles" && key == "模型"
        ));
    }
}
