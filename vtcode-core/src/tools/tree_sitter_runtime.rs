use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use crate::tools::ast_grep_language::AstGrepLanguage;

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

        if !ready
            .iter()
            .any(|current| current == language.display_name())
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

#[cfg_attr(not(test), allow(dead_code))]
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
    let grammar = grammar_for(language);
    parser
        .set_language(&grammar)
        .map_err(|err| format!("failed to load {} grammar: {err}", language.display_name()))?;
    Ok(parser)
}

fn grammar_for(language: AstGrepLanguage) -> tree_sitter::Language {
    match language {
        AstGrepLanguage::Rust => tree_sitter_rust::LANGUAGE.into(),
        AstGrepLanguage::Python => tree_sitter_python::LANGUAGE.into(),
        AstGrepLanguage::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        AstGrepLanguage::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        AstGrepLanguage::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        AstGrepLanguage::Go => tree_sitter_go::LANGUAGE.into(),
        AstGrepLanguage::Java => tree_sitter_java::LANGUAGE.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_source, prewarm_language, prewarm_workspace_languages};
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
}
