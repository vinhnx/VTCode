//! Generic, comment-preserving updates for TOML value trees.
//!
//! Callers such as the slash-command config helpers mutate a whole
//! [`toml::Value`] tree and then need to persist it. Re-serializing that tree
//! rewrites every key and discards the user's comments, key order and spacing —
//! the same waste as reformatting every unchanged record in a sparse file
//! rewrite. This module diffs the mutated tree against the bytes on disk and
//! applies only the changed leaves, retaining unchanged items and their formatting.

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use anyhow::{Context, Result, anyhow, bail};
use toml::Value as TomlValue;
use vtcode_commons::VtCodePaths;

/// Read a configuration snapshot for editing, or return `None` for a missing file.
///
/// # Errors
///
/// Rejects symlinks, non-regular files, invalid UTF-8 and other read failures.
pub fn read_config_toml_for_edit(path: &Path) -> Result<Option<String>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if !metadata.is_file() => bail!("Config path is not a regular file: {}", path.display()),
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("Failed to inspect config {}", path.display())),
    }

    let content = String::from_utf8(VtCodePaths::read_file_no_follow(path)?)
        .with_context(|| format!("Configuration file {} is not valid UTF-8", path.display()))?;
    Ok(Some(content))
}

/// Re-render `original_content` with only the leaves that differ from
/// `updated` replaced, preserving unchanged TOML items and their decorations.
///
/// # Errors
///
/// Returns an error if either the original content or a patched value cannot
/// be parsed as TOML, or if a patch path conflicts with a non-table value.
pub fn merge_toml_value_preserving_comments(original_content: &str, updated: &TomlValue) -> Result<String> {
    render_toml_edit(original_content, updated, |_| false)
}

pub(super) fn render_toml_edit(
    original_content: &str,
    updated: &TomlValue,
    clean: impl FnOnce(&mut toml_edit::DocumentMut) -> bool,
) -> Result<String> {
    let original: toml::Table = toml::from_str(original_content).context("Failed to parse original TOML")?;
    let updated = updated.as_table().context("Updated TOML root is not a table")?;
    let mut document: toml_edit::DocumentMut =
        original_content.parse().context("Failed to parse original TOML for editing")?;

    let changed = merge_table(document.as_table_mut(), &original, updated, false)?;
    if !clean(&mut document) && !changed {
        return Ok(original_content.to_owned());
    }

    let rendered = document.to_string();
    if original_content.contains("\r\n")
        && original_content
            .split_inclusive('\n')
            .all(|line| !line.ends_with('\n') || line.ends_with("\r\n"))
    {
        // toml_edit normalizes line endings while rendering.
        Ok(rendered.replace('\n', "\r\n"))
    } else {
        Ok(rendered)
    }
}

fn merge_table(
    table: &mut dyn toml_edit::TableLike,
    original: &toml::Table,
    updated: &toml::Table,
    inline: bool,
) -> Result<bool> {
    let mut changed = false;
    for (key, value) in updated {
        let Some(existing) = table.get_mut(key) else {
            table.insert(key, value_to_item(value)?);
            changed = true;
            continue;
        };
        let previous = original
            .get(key)
            .with_context(|| format!("Missing original TOML key {key:?}"))?;
        if let (Some(previous), Some(value)) = (previous.as_table(), value.as_table()) {
            let inline = existing.is_inline_table();
            let child = existing
                .as_table_like_mut()
                .with_context(|| format!("TOML key {key:?} is not a table"))?;
            changed |= merge_table(child, previous, value, inline)?;
        } else if !same_value(previous, value) {
            if merge_single_table_array_entry(existing, previous, value)? {
                changed = true;
                continue;
            }
            let mut replacement = value_to_item(value)?;
            if inline {
                replacement.make_value();
            }
            replace_value_preserving_decor(existing, replacement);
            changed = true;
        }
    }

    for key in original.keys().filter(|key| !updated.contains_key(*key)) {
        table.remove(key);
        changed = true;
    }
    Ok(changed)
}

// A single in-place edit cannot be a reorder. Keep each existing header and
// only regenerate the array when its shape or multiple entries changed.
fn merge_single_table_array_entry(
    item: &mut toml_edit::Item,
    previous: &TomlValue,
    updated: &TomlValue,
) -> Result<bool> {
    let (Some(tables), Some(previous), Some(updated)) =
        (item.as_array_of_tables_mut(), previous.as_array(), updated.as_array())
    else {
        return Ok(false);
    };
    if tables.len() != previous.len() || previous.len() != updated.len() {
        return Ok(false);
    }
    let mut edits = previous
        .iter()
        .zip(updated)
        .enumerate()
        .filter(|(_, (old, new))| !same_value(old, new));
    let Some((index, (TomlValue::Table(old), TomlValue::Table(new)))) = edits.next() else {
        return Ok(false);
    };
    if edits.next().is_some() {
        return Ok(false);
    }
    let table = tables.get_mut(index).context("Missing original TOML array entry")?;
    merge_table(table, old, new, false)
}

// NaN compares unequal to itself, which would rewrite otherwise untouched values.
// Compare float representations to also preserve the distinction between signed zeros.
fn same_value(left: &TomlValue, right: &TomlValue) -> bool {
    match (left, right) {
        (TomlValue::Float(left), TomlValue::Float(right)) => left.to_bits() == right.to_bits(),
        (TomlValue::Array(left), TomlValue::Array(right)) => {
            left.len() == right.len() && left.iter().zip(right).all(|(left, right)| same_value(left, right))
        }
        (TomlValue::Table(left), TomlValue::Table(right)) => {
            left.len() == right.len()
                && left
                    .iter()
                    .all(|(key, value)| right.get(key).is_some_and(|other| same_value(value, other)))
        }
        _ => left == right,
    }
}

/// Convert a `toml::Value` into an editable item via a round-trip through the
/// TOML serializer. This keeps string quoting, datetimes, arrays and nested
/// tables correct without duplicating `toml_edit`'s value conversion.
fn value_to_item(value: &TomlValue) -> Result<toml_edit::Item> {
    const WRAPPER_KEY: &str = "__vtcode_patch_value";
    let mut wrapper = toml::map::Map::new();
    wrapper.insert(WRAPPER_KEY.to_string(), value.clone());
    let serialized = toml::to_string(&TomlValue::Table(wrapper)).context("Failed to serialize patched TOML value")?;
    let mut document: toml_edit::DocumentMut = serialized.parse().context("Failed to parse patched TOML value")?;
    let mut item = document
        .as_table_mut()
        .remove(WRAPPER_KEY)
        .ok_or_else(|| anyhow!("Serialized TOML patch did not contain the patched value"))?;
    // Positions belong to the temporary document. Inherit placement from
    // the destination so a nested table follows its own array entry.
    clear_item_positions(&mut item);
    Ok(item)
}

fn clear_item_positions(item: &mut toml_edit::Item) {
    match item {
        toml_edit::Item::Table(table) => clear_table_positions(table),
        toml_edit::Item::ArrayOfTables(tables) => {
            for table in tables.iter_mut() {
                clear_table_positions(table);
            }
        }
        _ => {}
    }
}

fn clear_table_positions(table: &mut toml_edit::Table) {
    table.set_position(None);
    for (_, item) in table.iter_mut() {
        clear_item_positions(item);
    }
}

/// Replacing a scalar should not erase the comment/decoration attached to its
/// original line when the value itself changed.
fn replace_value_preserving_decor(existing: &mut toml_edit::Item, replacement: toml_edit::Item) {
    match replacement {
        toml_edit::Item::Value(mut new) => {
            if let toml_edit::Item::Value(old) = &*existing {
                *new.decor_mut() = old.decor().clone();
            }
            *existing = toml_edit::Item::Value(new);
        }
        other => *existing = other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_preserves_unrelated_comments_order_and_whitespace() {
        let original = "# header\n\n[features]\nmemories = false # old value\n\n[agent]\ntheme = \"ansi\"\n";
        let mut updated: TomlValue = toml::from_str(original).expect("parse");
        updated["features"]["memories"] = TomlValue::Boolean(true);

        let rendered = merge_toml_value_preserving_comments(original, &updated).expect("merge");
        assert!(rendered.contains("# header"), "{rendered}");
        assert!(rendered.contains("theme = \"ansi\""), "{rendered}");
        assert!(rendered.contains("memories = true # old value"), "{rendered}");
        assert!(rendered.find("[features]").expect("features") < rendered.find("[agent]").expect("agent"));
    }

    #[test]
    fn patch_preserves_crlf_document_style() {
        let original = "# user config\r\n[features]\r\nmemories = false\r\n";
        let mut updated: TomlValue = toml::from_str(original).expect("parse");
        updated["features"]["memories"] = TomlValue::Boolean(true);
        assert_eq!(
            merge_toml_value_preserving_comments(original, &updated).expect("merge"),
            "# user config\r\n[features]\r\nmemories = true\r\n"
        );
    }

    #[test]
    fn patch_replaces_inline_scalar_with_nested_table() {
        let original = "agent = { persistent_memory = false, theme = 'ciapre' }\n";
        let updated = toml::from_str("agent = { persistent_memory = { enabled = true }, theme = 'ciapre' }")
            .expect("parse updated");
        let rendered = merge_toml_value_preserving_comments(original, &updated).expect("patch");
        assert_eq!(toml::from_str::<TomlValue>(&rendered).expect("parse output"), updated);
        assert!(rendered.contains("theme = 'ciapre'"), "{rendered}");
    }

    #[test]
    fn patch_preserves_untouched_nan_and_no_op_bytes() {
        let original = "config = [{ value = +nan }] # keep spelling\nother = false";
        let mut updated: TomlValue = toml::from_str(original).expect("parse");
        assert_eq!(merge_toml_value_preserving_comments(original, &updated).expect("no-op"), original);
        updated["other"] = TomlValue::Boolean(true);
        let rendered = merge_toml_value_preserving_comments(original, &updated).expect("patch");
        assert!(rendered.contains("config = [{ value = +nan }] # keep spelling"), "{rendered}");
        assert!(rendered.contains("other = true"), "{rendered}");
    }

    #[test]
    fn patch_drops_removed_keys_without_touching_others() {
        let original = "[agent.persistent_memory]\ndirectory_override = \"/tmp/x\"\nstartup_line_limit = 20\n";
        let mut updated: TomlValue = toml::from_str(original).expect("parse");
        updated["agent"]["persistent_memory"]
            .as_table_mut()
            .expect("table")
            .remove("directory_override");

        let rendered = merge_toml_value_preserving_comments(original, &updated).expect("merge");
        assert!(!rendered.contains("directory_override"), "{rendered}");
        assert!(rendered.contains("startup_line_limit = 20"), "{rendered}");
    }

    #[test]
    fn patch_updates_one_array_of_tables_entry_preserving_other_entries() {
        let original = "# providers\n[[custom_providers]]\nname = \"a\" # renamed\n\n# keep provider C\n[[custom_providers]]\nname = 'c'\n";
        let mut updated: TomlValue = toml::from_str(original).expect("parse");
        let providers = updated
            .get_mut("custom_providers")
            .and_then(TomlValue::as_array_mut)
            .expect("array");
        let mut replacement = toml::map::Map::new();
        replacement.insert("name".to_string(), TomlValue::String("b".to_string()));
        providers[0] = TomlValue::Table(replacement);

        let rendered = merge_toml_value_preserving_comments(original, &updated).expect("merge");
        assert_eq!(toml::from_str::<TomlValue>(&rendered).expect("parse output"), updated);
        assert_eq!(rendered, original.replace("name = \"a\"", "name = \"b\""));
    }

    #[test]
    fn patch_places_nested_table_under_the_edited_array_entry() {
        let original = "# first\n[[providers]]\nname = 'a'\n\n# second\n[[providers]]\nname = 'b'\n";
        let updated = toml::from_str(&format!("{original}\n[providers.profile.limits]\nmax_tokens = 250\n"))
            .expect("parse desired document");
        let rendered = merge_toml_value_preserving_comments(original, &updated).expect("merge");
        assert_eq!(toml::from_str::<TomlValue>(&rendered).expect("parse output"), updated);
        assert!(rendered.starts_with(original), "{rendered}");
    }

    #[test]
    fn patch_adds_new_keys_under_existing_tables() {
        let original = "# note\n[features]\nmemories = false\n";
        let mut updated: TomlValue = toml::from_str(original).expect("parse");
        updated["features"]
            .as_table_mut()
            .expect("table")
            .insert("new_flag".to_string(), TomlValue::Boolean(true));

        let rendered = merge_toml_value_preserving_comments(original, &updated).expect("merge");
        assert!(rendered.contains("# note"), "{rendered}");
        assert!(rendered.contains("new_flag = true"), "{rendered}");
        assert!(rendered.contains("memories = false"), "{rendered}");
    }
}
