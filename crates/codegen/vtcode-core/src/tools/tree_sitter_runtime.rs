use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::tools::ast_grep_language::AstGrepLanguage;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SourceByteRange {
    pub start: usize,
    pub end: usize,
}

pub(crate) fn usage_node_kind_allowlist(
    language: AstGrepLanguage,
) -> Option<&'static [&'static str]> {
    match language {
        AstGrepLanguage::Rust => Some(&["identifier", "type_identifier", "field_identifier"]),
        AstGrepLanguage::Python => Some(&["identifier"]),
        AstGrepLanguage::JavaScript | AstGrepLanguage::TypeScript | AstGrepLanguage::Tsx => {
            Some(&[
                "identifier",
                "property_identifier",
                "shorthand_property_identifier",
                "shorthand_property_identifier_pattern",
                "type_identifier",
            ])
        }
        AstGrepLanguage::Go => Some(&["identifier", "field_identifier", "type_identifier"]),
        AstGrepLanguage::Java => Some(&["identifier", "type_identifier"]),
        AstGrepLanguage::C => Some(&["identifier", "type_identifier", "field_identifier"]),
        AstGrepLanguage::Cpp => Some(&[
            "identifier",
            "type_identifier",
            "field_identifier",
            "namespace_identifier",
        ]),
        AstGrepLanguage::Bash
        | AstGrepLanguage::Markdown
        | AstGrepLanguage::Csharp
        | AstGrepLanguage::Css
        | AstGrepLanguage::Html
        | AstGrepLanguage::Json
        | AstGrepLanguage::Yaml
        | AstGrepLanguage::Ruby
        | AstGrepLanguage::Php
        | AstGrepLanguage::Kotlin
        | AstGrepLanguage::Swift
        | AstGrepLanguage::Lua
        | AstGrepLanguage::Sql
        | AstGrepLanguage::Scala
        | AstGrepLanguage::Elixir
        | AstGrepLanguage::Dockerfile
        | AstGrepLanguage::Toml
        | AstGrepLanguage::Hcl
        | AstGrepLanguage::Dart
        | AstGrepLanguage::Zig
        | AstGrepLanguage::Protobuf
        | AstGrepLanguage::Haskell
        | AstGrepLanguage::Nix
        | AstGrepLanguage::Solidity => None,
    }
}

fn exact_named_node_for_range(
    tree: &tree_sitter::Tree,
    range: SourceByteRange,
) -> Option<tree_sitter::Node<'_>> {
    if range.start >= range.end {
        return None;
    }
    let mut node = tree
        .root_node()
        .descendant_for_byte_range(range.start, range.end.saturating_sub(1))?;
    loop {
        if node.is_named() && node.start_byte() == range.start && node.end_byte() == range.end {
            return Some(node);
        }
        node = node.parent()?;
    }
}

pub(crate) fn is_exact_usage_identifier(
    tree: &tree_sitter::Tree,
    language: AstGrepLanguage,
    range: SourceByteRange,
) -> bool {
    let Some(allowlist) = usage_node_kind_allowlist(language) else {
        return false;
    };
    exact_named_node_for_range(tree, range).is_some_and(|node| allowlist.contains(&node.kind()))
}

fn declaration_name_node_kind_allowlist(
    language: AstGrepLanguage,
) -> Option<&'static [&'static str]> {
    match language {
        AstGrepLanguage::Bash => Some(&["word"]),
        _ => usage_node_kind_allowlist(language),
    }
}

pub(crate) fn exact_declaration_name_range(
    tree: &tree_sitter::Tree,
    source: &str,
    language: AstGrepLanguage,
    declaration_range: SourceByteRange,
    outline_name: &str,
    query: &str,
) -> Option<SourceByteRange> {
    if declaration_range.start >= declaration_range.end {
        return None;
    }
    let declaration = tree.root_node().descendant_for_byte_range(
        declaration_range.start,
        declaration_range.end.saturating_sub(1),
    )?;
    let name = declaration.child_by_field_name("name")?;
    let allowlist = declaration_name_node_kind_allowlist(language)?;
    if !allowlist.contains(&name.kind()) {
        return None;
    }
    let name_text = name.utf8_text(source.as_bytes()).ok()?;
    let matches = if query.chars().any(char::is_uppercase) {
        name_text == outline_name
    } else {
        name_text.to_lowercase() == outline_name.to_lowercase()
    };
    matches.then_some(SourceByteRange { start: name.start_byte(), end: name.end_byte() })
}

static TREE_SITTER_PARSERS: OnceLock<
    Mutex<HashMap<AstGrepLanguage, Result<tree_sitter::Parser, String>>>,
> = OnceLock::new();

pub(crate) fn prewarm_workspace_languages<'a>(
    languages: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let mut ready = Vec::new();

    for language_name in languages {
        let Some(language) = AstGrepLanguage::from_workspace_language(language_name) else {
            continue;
        };

        if !ready.iter().any(|current| current == language.display_name())
            && prewarm_language(language).is_ok()
        {
            ready.push(language.display_name().to_string());
        }
    }

    ready
}

pub(crate) fn prewarm_language(language: AstGrepLanguage) -> Result<(), String> {
    with_parser(language, |_| Ok(()))
}

pub(crate) fn parse_source(
    language: AstGrepLanguage,
    source: &str,
) -> Result<tree_sitter::Tree, String> {
    with_parser(language, |parser| {
        parser
            .parse(source, None)
            .ok_or_else(|| format!("failed to parse {} source", language.display_name()))
    })
}

fn with_parser<T>(
    language: AstGrepLanguage,
    op: impl FnOnce(&mut tree_sitter::Parser) -> Result<T, String>,
) -> Result<T, String> {
    let cache = TREE_SITTER_PARSERS.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cache
        .lock()
        .map_err(|err| format!("tree-sitter parser cache poisoned: {err}"))?;
    let parser = guard
        .entry(language)
        .or_insert_with(|| build_parser(language))
        .as_mut()
        .map_err(|err| err.clone())?;

    op(parser)
}

fn build_parser(language: AstGrepLanguage) -> Result<tree_sitter::Parser, String> {
    let mut parser = tree_sitter::Parser::new();
    let grammar = grammar_for(language).ok_or_else(|| {
        format!(
            "no local tree-sitter parser for {}; structural queries delegate to the ast-grep binary",
            language.display_name()
        )
    })?;
    parser
        .set_language(&grammar)
        .map_err(|err| format!("failed to load {} grammar: {err}", language.display_name()))?;
    Ok(parser)
}

fn grammar_for(language: AstGrepLanguage) -> Option<tree_sitter::Language> {
    Some(match language {
        AstGrepLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        AstGrepLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        AstGrepLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        AstGrepLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        AstGrepLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        AstGrepLanguage::Go => tree_sitter_go::LANGUAGE.into(),
        AstGrepLanguage::Java => tree_sitter_java::LANGUAGE.into(),
        AstGrepLanguage::Bash => tree_sitter_bash::LANGUAGE.into(),
        AstGrepLanguage::C => tree_sitter_c::LANGUAGE.into(),
        AstGrepLanguage::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        // Languages without bundled local parsers delegate to the ast-grep binary,
        // which has its own built-in tree-sitter parsers for all supported languages.
        AstGrepLanguage::Markdown
        | AstGrepLanguage::Csharp
        | AstGrepLanguage::Css
        | AstGrepLanguage::Html
        | AstGrepLanguage::Json
        | AstGrepLanguage::Yaml
        | AstGrepLanguage::Ruby
        | AstGrepLanguage::Php
        | AstGrepLanguage::Kotlin
        | AstGrepLanguage::Swift
        | AstGrepLanguage::Lua
        | AstGrepLanguage::Sql
        | AstGrepLanguage::Scala
        | AstGrepLanguage::Elixir
        | AstGrepLanguage::Dockerfile
        | AstGrepLanguage::Toml
        | AstGrepLanguage::Hcl
        | AstGrepLanguage::Dart
        | AstGrepLanguage::Zig
        | AstGrepLanguage::Protobuf
        | AstGrepLanguage::Haskell
        | AstGrepLanguage::Nix
        | AstGrepLanguage::Solidity => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        SourceByteRange, exact_declaration_name_range, is_exact_usage_identifier, parse_source,
        prewarm_language, prewarm_workspace_languages, usage_node_kind_allowlist,
    };
    use crate::tools::ast_grep_language::AstGrepLanguage;

    #[test]
    fn prewarm_workspace_languages_only_returns_supported_languages() {
        let ready = prewarm_workspace_languages(["Rust", "Swift", "TypeScript", "Rust"]);

        assert_eq!(ready, vec!["Rust".to_string(), "TypeScript".to_string()]);
    }

    #[test]
    fn prewarm_language_initializes_all_supported_parsers() {
        for language in [
            AstGrepLanguage::Rust,
            AstGrepLanguage::Python,
            AstGrepLanguage::JavaScript,
            AstGrepLanguage::TypeScript,
            AstGrepLanguage::Tsx,
            AstGrepLanguage::Go,
            AstGrepLanguage::Java,
        ] {
            assert!(prewarm_language(language).is_ok(), "{language:?}");
        }
    }

    #[test]
    fn parse_source_returns_a_tree_for_rust_code() {
        let tree = parse_source(AstGrepLanguage::Rust, "fn main() -> usize { 1 }\n")
            .expect("rust source should parse");

        assert!(!tree.root_node().has_error());
    }

    #[test]
    fn tree_sitter_usage_allowlist_is_frozen() {
        let expected = [
            (AstGrepLanguage::Rust, &["identifier", "type_identifier", "field_identifier"][..]),
            (AstGrepLanguage::Python, &["identifier"][..]),
            (
                AstGrepLanguage::JavaScript,
                &[
                    "identifier",
                    "property_identifier",
                    "shorthand_property_identifier",
                    "shorthand_property_identifier_pattern",
                    "type_identifier",
                ][..],
            ),
            (
                AstGrepLanguage::TypeScript,
                &[
                    "identifier",
                    "property_identifier",
                    "shorthand_property_identifier",
                    "shorthand_property_identifier_pattern",
                    "type_identifier",
                ][..],
            ),
            (
                AstGrepLanguage::Tsx,
                &[
                    "identifier",
                    "property_identifier",
                    "shorthand_property_identifier",
                    "shorthand_property_identifier_pattern",
                    "type_identifier",
                ][..],
            ),
            (AstGrepLanguage::Go, &["identifier", "field_identifier", "type_identifier"][..]),
            (AstGrepLanguage::Java, &["identifier", "type_identifier"][..]),
            (AstGrepLanguage::C, &["identifier", "type_identifier", "field_identifier"][..]),
            (
                AstGrepLanguage::Cpp,
                &[
                    "identifier",
                    "type_identifier",
                    "field_identifier",
                    "namespace_identifier",
                ][..],
            ),
        ];
        for (language, kinds) in expected {
            assert_eq!(usage_node_kind_allowlist(language), Some(kinds));
            assert!(!kinds.contains(&"string_literal"));
        }
        assert_eq!(usage_node_kind_allowlist(AstGrepLanguage::Bash), None);
    }

    #[test]
    fn tree_sitter_excludes_only_exact_declaration_name() {
        let source = "fn Widget() { Widget(); }\n";
        let tree = parse_source(AstGrepLanguage::Rust, source).expect("Rust parses");
        let declaration = SourceByteRange { start: 0, end: 25 };
        let name = exact_declaration_name_range(
            &tree,
            source,
            AstGrepLanguage::Rust,
            declaration,
            "Widget",
            "Widget",
        )
        .expect("name field should resolve");
        assert_eq!(name, SourceByteRange { start: 3, end: 9 });
        assert!(is_exact_usage_identifier(
            &tree,
            AstGrepLanguage::Rust,
            SourceByteRange { start: 14, end: 20 },
        ));
        assert!(!is_exact_usage_identifier(
            &tree,
            AstGrepLanguage::Rust,
            SourceByteRange { start: 3, end: 8 },
        ));
        assert!(
            exact_declaration_name_range(
                &tree,
                source,
                AstGrepLanguage::Rust,
                declaration,
                "Different",
                "Widget",
            )
            .is_none()
        );
        assert!(
            exact_declaration_name_range(
                &tree,
                source,
                AstGrepLanguage::Rust,
                SourceByteRange { start: 12, end: 23 },
                "Widget",
                "Widget",
            )
            .is_none()
        );
    }

    #[test]
    fn tree_sitter_definition_name_smart_case_supports_unicode_lowercase_queries() {
        let source = "fn Éclair() { Éclair(); }\n";
        let tree = parse_source(AstGrepLanguage::Rust, source).expect("Rust parses");
        let declaration = SourceByteRange { start: 0, end: source.len() - 1 };
        let name = exact_declaration_name_range(
            &tree,
            source,
            AstGrepLanguage::Rust,
            declaration,
            "Éclair",
            "éclair",
        )
        .expect("Unicode lower-case smart-case name");
        assert_eq!(&source[name.start..name.end], "Éclair");
        assert!(
            exact_declaration_name_range(
                &tree,
                source,
                AstGrepLanguage::Rust,
                declaration,
                "éclair",
                "Éclair",
            )
            .is_none()
        );
    }

    #[test]
    fn bash_declaration_names_are_valid_without_enabling_bash_usages() {
        let source = "function Widget() { echo Widget; }\n";
        let tree = parse_source(AstGrepLanguage::Bash, source).expect("Bash parses");
        let name = exact_declaration_name_range(
            &tree,
            source,
            AstGrepLanguage::Bash,
            SourceByteRange { start: 0, end: source.len() },
            "Widget",
            "Widget",
        )
        .expect("Bash function name");

        assert_eq!(&source[name.start..name.end], "Widget");
        assert!(!is_exact_usage_identifier(
            &tree,
            AstGrepLanguage::Bash,
            SourceByteRange { start: 25, end: 31 },
        ));
    }
}
