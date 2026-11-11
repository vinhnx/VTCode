//! Bash AST access owned by `vtcode-safety`.
//!
//! The bash grammar is a security boundary, so `tree-sitter` and
//! `tree-sitter-bash` live only in this crate. Callers outside
//! `vtcode-safety` use the [`BashTree`] / [`BashNode`] wrappers instead of
//! raw tree-sitter types, which keeps the grammar dependency out of
//! `vtcode-core` and makes every AST consumer fail closed by construction:
//! a parse failure is an `Err`, never a partially-valid tree.
//!
//! The wrappers deliberately expose only what the shell-classification and
//! redirection-validation call sites need. Anything richer (arbitrary node
//! kinds, byte offsets) is not part of the contract, so the surface cannot
//! grow into a general-purpose AST API.

use std::num::NonZeroUsize;
use std::sync::Mutex;
use std::sync::OnceLock;

use lru::LruCache;
use rustc_hash::FxBuildHasher;

/// Upper bound on cached parsed bash trees.
const SHELL_TREE_CACHE_CAPACITY: usize = 256;

/// Process-wide LRU of parsed bash trees keyed by source text.
///
/// Command classification parses the *same* static script several times per
/// validation: `contains_dynamic_find_syntax` (command extraction),
/// `validate_redirection_paths` (AST walk), and `parse_shell_commands`
/// (command extraction again) each run the grammar independently. A single
/// `tree_sitter::Tree` is a cheap refcounted handle (`Clone` bumps a refcount,
/// not a re-parse), so parsing once and sharing the tree across every pass
/// removes N-1 tree-sitter runs per command — the same "compute the
/// representation once" shape that unblocked the symbol-interner storm in the
/// clippy `nonstandard_macro_braces` fix. The tree is a pure function of the
/// source text, so caching cannot change any safety verdict; the cache only
/// ever returns the exact grammar output for the exact input.
static SHELL_TREE_CACHE: OnceLock<Mutex<LruCache<String, tree_sitter::Tree, FxBuildHasher>>> = OnceLock::new();

/// Returns the shared parsed-tree cache, initializing it on first use.
///
/// A poisoned lock is recovered rather than propagated: a stale cache entry is
/// a pure performance concern and must never turn into a fail-open safety path
/// or a hard error on an otherwise classifiable command.
fn shell_tree_cache() -> &'static Mutex<LruCache<String, tree_sitter::Tree, FxBuildHasher>> {
    SHELL_TREE_CACHE.get_or_init(|| {
        let capacity = NonZeroUsize::new(SHELL_TREE_CACHE_CAPACITY).unwrap_or(NonZeroUsize::MIN);
        Mutex::new(LruCache::with_hasher(capacity, FxBuildHasher))
    })
}

/// A parsed bash script together with the source text it was parsed from.
///
/// Owning the source alongside the tree keeps every [`BashNode::text`] result
/// a borrow of the same buffer, so AST consumers cannot accidentally read
/// text from a different script.
#[derive(Debug, Clone)]
pub struct BashTree<'a> {
    tree: tree_sitter::Tree,
    source: &'a str,
}

impl<'a> BashTree<'a> {
    /// Parses `script` into a bash AST.
    ///
    /// Returns `Err` when the grammar cannot be loaded or the script cannot be
    /// parsed at all. A script that parses *with* syntax errors still yields a
    /// tree; callers that must reject malformed input check
    /// [`BashTree::has_error`].
    pub fn parse(script: &'a str) -> Result<Self, String> {
        let (tree, _) = parse_bash_tree(script)?;
        Ok(Self { tree, source: script })
    }

    /// Returns the root node of the parsed script.
    #[must_use]
    pub fn root(&self) -> BashNode<'_> {
        BashNode { node: self.tree.root_node(), source: self.source }
    }

    /// Whether the grammar reported a syntax error anywhere in the script.
    ///
    /// Tree-sitter applies error recovery instead of failing, so malformed
    /// scripts parse into a tree whose error nodes must be rejected explicitly.
    #[must_use]
    pub fn has_error(&self) -> bool {
        self.tree.root_node().has_error()
    }
}

/// A node in a parsed bash script.
#[derive(Debug, Clone, Copy)]
pub struct BashNode<'a> {
    node: tree_sitter::Node<'a>,
    source: &'a str,
}

impl<'a> BashNode<'a> {
    /// The grammar's node kind, such as `command`, `word`, or `file_redirect`.
    #[must_use]
    pub fn kind(&self) -> &'a str {
        self.node.kind()
    }

    /// The source text covered by this node.
    ///
    /// Returns an empty string when the node's span is not valid UTF-8, which
    /// keeps AST walks total instead of panicking on malformed input.
    #[must_use]
    pub fn text(&self) -> &'a str {
        self.node.utf8_text(self.source.as_bytes()).unwrap_or("")
    }

    /// The byte offset where this node starts in the source text.
    #[must_use]
    pub fn start_byte(&self) -> usize {
        self.node.start_byte()
    }

    /// Whether the grammar considers this node named.
    ///
    /// Anonymous nodes carry punctuation (`&&`, `|`, `;`), so a walk that must
    /// reason about structure rather than operators checks this first.
    #[must_use]
    pub fn is_named(&self) -> bool {
        self.node.is_named()
    }

    /// Returns the child bound to `field`, such as a command's `name`.
    #[must_use]
    pub fn field(&self, field: &str) -> Option<Self> {
        self.node
            .child_by_field_name(field)
            .map(|node| Self { node, source: self.source })
    }

    /// Returns every child node, named and anonymous.
    #[must_use]
    pub fn children(&self) -> Vec<Self> {
        let mut cursor = self.node.walk();
        self.node
            .children(&mut cursor)
            .map(|node| Self { node, source: self.source })
            .collect()
    }

    /// Returns every named child node.
    #[must_use]
    pub fn named_children(&self) -> Vec<Self> {
        let mut cursor = self.node.walk();
        self.node
            .named_children(&mut cursor)
            .map(|node| Self { node, source: self.source })
            .collect()
    }

    /// Returns every child node bound to `field`.
    ///
    /// A node can bind the same field more than once, so this returns a list
    /// rather than a single node.
    #[must_use]
    pub fn field_children(&self, field: &str) -> Vec<Self> {
        let mut cursor = self.node.walk();
        self.node
            .children_by_field_name(field, &mut cursor)
            .map(|node| Self { node, source: self.source })
            .collect()
    }
}

/// Parses `script` into a tree-sitter bash tree, reusing the cached tree when
/// the exact source text was parsed before.
///
/// Returns the tree together with whether the grammar reported a syntax error,
/// so callers keep their original `has_error` semantics. Tree-sitter applies
/// error recovery instead of returning `None`, so the split between "did not
/// parse at all" (`Err`) and "parsed with errors" (`Ok((.., true))`) is
/// preserved for callers that must fail closed on malformed input.
pub(crate) fn parse_bash_tree(script: &str) -> Result<(tree_sitter::Tree, bool), String> {
    if let Some(tree) = {
        let mut cache = shell_tree_cache().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        cache.get(script).cloned()
    } {
        let has_error = tree.root_node().has_error();
        return Ok((tree, has_error));
    }

    let (tree, has_error) = {
        let parser_guard = get_bash_parser()?;
        let mut parser = parser_guard.lock().map_err(|e| format!("Failed to lock parser: {e}"))?;
        let tree = parser.parse(script, None).ok_or_else(|| "Failed to parse script".to_string())?;
        let has_error = tree.root_node().has_error();
        (tree, has_error)
    };

    let mut cache = shell_tree_cache().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    drop(cache.put(script.to_string(), tree.clone()));
    Ok((tree, has_error))
}

/// Lazy-initialized tree-sitter bash parser (wrapped in Mutex for mutation)
static BASH_PARSER: OnceLock<Result<Mutex<tree_sitter::Parser>, String>> = OnceLock::new();

/// Gets or initializes the bash parser
fn get_bash_parser() -> Result<&'static Mutex<tree_sitter::Parser>, String> {
    BASH_PARSER
        .get_or_init(|| {
            let mut parser = tree_sitter::Parser::new();
            let lang: tree_sitter::Language = tree_sitter_bash::LANGUAGE.into();
            parser
                .set_language(&lang)
                .map_err(|e| format!("Failed to load bash grammar: {e}"))?;
            Ok(Mutex::new(parser))
        })
        .as_ref()
        .map_err(Clone::clone)
}

/// Ensures the bash tree-sitter parser is initialized.
pub fn prewarm_bash_parser() -> Result<(), String> {
    let _ = get_bash_parser()?;
    Ok(())
}
