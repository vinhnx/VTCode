#![allow(unused_imports)]

#[allow(unused_imports)]
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AstGrepFailureKind {
    PatternParse,
    LanguageSupport,
    Other,
}

/// Where the failure came from, passed explicitly by each call site so hint
/// routing never depends on the wording of the display prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum AstGrepFailureOrigin {
    /// Local pattern preflight rejected the pattern before invoking ast-grep.
    Preflight,
    /// Read-only invocation: search, scan, count, debug query, rule query, new.
    Search,
    /// Rewrite-family invocation: rewrite preview, FixConfig rewrite, apply.
    Rewrite,
}

pub(super) fn classify_ast_grep_failure(
    origin: AstGrepFailureOrigin,
    detail: &str,
) -> AstGrepFailureKind {
    let lowered = detail.to_ascii_lowercase();
    if language_support_markers_present(&lowered) {
        return AstGrepFailureKind::LanguageSupport;
    }
    if origin == AstGrepFailureOrigin::Preflight || pattern_parse_markers_present(&lowered) {
        return AstGrepFailureKind::PatternParse;
    }
    AstGrepFailureKind::Other
}

fn pattern_parse_markers_present(lowered: &str) -> bool {
    lowered.contains("not parseable")
        || lowered.contains("cannot parse")
        || lowered.contains("fail to parse")
        || lowered.contains("failed to parse")
        || lowered.contains("parse error")
        || lowered.contains("invalid pattern")
        || lowered.contains("error in pattern")
        || lowered.contains("contains an error")
        || lowered.contains("invalid rule")
        || lowered.contains("cannot parse rule")
}

#[cold]
pub(super) fn format_ast_grep_failure(
    origin: AstGrepFailureOrigin,
    prefix: &str,
    detail: String,
) -> String {
    let kind = classify_ast_grep_failure(origin, &detail);
    let mut message = format!("{prefix}: {detail}.");
    match kind {
        AstGrepFailureKind::PatternParse => {
            message.push(' ');
            message.push_str(AST_GREP_PATTERN_HINT);
        }
        AstGrepFailureKind::LanguageSupport => {
            message.push(' ');
            message.push_str(AST_GREP_PROJECT_CONFIG_HINT);
        }
        AstGrepFailureKind::Other => {}
    }
    if origin == AstGrepFailureOrigin::Rewrite {
        message.push(' ');
        message.push_str(AST_GREP_REWRITE_HINT);
    }
    message.push(' ');
    message.push_str(AST_GREP_GENERIC_TAIL);
    if !detail.contains(AST_GREP_INSTALL_COMMAND) {
        message.push(' ');
        message.push_str(&format!(
            "If the binary is missing, install it with `{AST_GREP_INSTALL_COMMAND}`."
        ));
    }
    message
}

fn language_support_markers_present(lowered: &str) -> bool {
    (lowered.contains("lang") || lowered.contains("language") || lowered.contains("extension"))
        && (lowered.contains("unsupported")
            || lowered.contains("invalid value")
            || lowered.contains("unknown")
            || lowered.contains("unrecognized")
            || lowered.contains("not built in")
            || lowered.contains("not supported"))
}

/// Returns true when a Go pattern looks like a bare function call that
/// tree-sitter-go would parse as a type conversion (e.g. `fmt.Println($A)`
/// or `json.Unmarshal($$$)`). These patterns need `context` + `selector`
/// to disambiguate.
pub(super) fn looks_like_go_call_pattern(pattern: &str) -> bool {
    // Match patterns like `pkg.Func($$$)`, `Func($$$)`, or
    // `expr.Method($$$)` where the pattern starts with an identifier
    // chain followed by parenthesized arguments. Metavariable prefixes
    // (`$`, `$$`, `$$$`) are stripped before checking identifier validity
    // so patterns like `$A.$B($$$)` are recognized as call patterns.
    let trimmed = pattern.trim();
    let Some(paren) = trimmed.find('(') else {
        return false;
    };
    if paren == 0 || !trimmed.ends_with(')') {
        return false;
    }
    let callee = &trimmed[..paren];
    // Callee must look like an identifier chain: `Func`, `pkg.Func`,
    // `pkg.Sub.Method`, etc. Strip metavariable prefixes so `$A.$B`
    // is treated like `A.B`.
    callee.split('.').all(|part| {
        let stripped = part.trim_start_matches('$');
        !stripped.is_empty()
            && stripped
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

pub(super) fn looks_like_html_attribute_pattern(pattern: &str) -> bool {
    // Match patterns like `class=$VAL`, `id=$ID`, `href=$URL` where the
    // pattern looks like an HTML attribute assignment without surrounding
    // element context.
    let trimmed = pattern.trim();
    if trimmed.contains('<') || trimmed.contains('>') {
        return false;
    }
    let Some(eq) = trimmed.find('=') else {
        return false;
    };
    let attr_name = &trimmed[..eq];
    // Attribute name must be a valid HTML attribute name (letters, digits,
    // hyphens, underscores, colons for namespaced attrs).
    !attr_name.is_empty()
        && attr_name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == ':')
}

pub(super) fn looks_like_html_tag_pattern(pattern: &str) -> bool {
    // Match patterns like `<$TAG>`, `<div>`, `<$TAG $$$ATTRS>` that look
    // like HTML opening tags without a closing tag or body.
    let trimmed = pattern.trim();
    trimmed.starts_with('<')
        && trimmed.contains('>')
        && !trimmed.contains("</")
        && !trimmed.ends_with("/>")
}

/// Returns true when a Java pattern looks like a bare type-qualified
/// identifier or field declaration fragment that tree-sitter-java would
/// fail to parse as standalone code. Common examples:
/// - `$MOD String $F` (modifier + type + name without surrounding class)
/// - `@Annotation` (bare annotation without surrounding declaration)
/// - `$TYPE $VAR;` fragments that need class-body context
pub(super) fn looks_like_java_declaration_fragment(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    // Bare annotation: `@Foo` or `@Foo($$$)`
    if trimmed.starts_with('@') {
        return true;
    }
    // Patterns with semicolons that look like field/variable declarations
    // without class context: `String $F;`, `private $TYPE $NAME;`
    if trimmed.ends_with(';') {
        // Contains a type-like identifier followed by a metavariable
        let inner = trimmed.trim_end_matches(';').trim();
        let parts: Vec<&str> = inner.split_whitespace().collect();
        if parts.len() >= 2 {
            // Last part should look like a metavariable or identifier
            let last = parts.last().expect("parts.len() >= 2 guarantees non-empty");
            if last.starts_with('$') || last.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return true;
            }
        }
    }
    false
}

/// Returns true when a pattern looks like a CSS selector fragment that
/// tree-sitter-css would fail to parse as standalone code. Common examples:
/// - `.class-name` (leading dot, class selector without braces)
/// - `#id-name` (leading hash, ID selector without braces)
pub(super) fn looks_like_css_selector_fragment(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    if trimmed.starts_with('.') && trimmed.len() > 1 && !trimmed.contains('{') {
        return true;
    }
    if trimmed.starts_with('#') && trimmed.len() > 1 && !trimmed.contains('{') {
        return true;
    }
    false
}

pub(super) fn looks_like_python_decorator_fragment(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    if trimmed.starts_with('@') && trimmed.len() > 1 && !trimmed.contains('\n') {
        let rest = &trimmed[1..];
        return rest
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_alphabetic() || c == '_');
    }
    false
}

pub(super) fn looks_like_ruby_block_fragment(pattern: &str) -> bool {
    let trimmed = pattern.trim();

    // Bare symbol-to-proc: `&:method_name`
    if trimmed.starts_with('&') && trimmed.len() > 1 {
        let after = &trimmed[1..];
        if after.starts_with(':') && after.len() > 1 {
            return true;
        }
    }

    // Bare pipe block: `{ |$V| ... }` or `do |$V| ... end`
    if trimmed.starts_with('{') && trimmed.contains('|') {
        return true;
    }
    if trimmed.starts_with("do") && trimmed.contains('|') {
        return true;
    }

    // Bare block body starting with pipe: `| $V | $V.$METHOD`
    if trimmed.starts_with('|') {
        return true;
    }

    false
}

/// Detect patterns that look like Rust method calls without a receiver,
/// e.g. `unwrap_or($T::default())`, `map_err($E)`, `and_then($C)`.
/// These fail tree-sitter preflight because `.method()` calls require a
/// receiver in Rust syntax. The correct ast-grep form is `$X.method($A)`.
///
/// This only fires when the full pattern contains metavariables, because
/// plain `foo()` parses fine as a function call and never reaches this
/// code path.
pub(super) fn looks_like_rust_method_call_fragment(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    // Must not start with `$` — that would already be a receiver.
    if trimmed.starts_with('$') {
        return false;
    }
    // Must end with `)` to look like a call.
    if !trimmed.ends_with(')') {
        return false;
    }
    // Must contain a metavariable — otherwise it's a plain expression
    // that tree-sitter can parse and this code path is never reached.
    if !trimmed.contains('$') {
        return false;
    }
    let Some(paren) = trimmed.find('(') else {
        return false;
    };
    if paren == 0 {
        return false;
    }
    let callee = &trimmed[..paren];
    // Callee must be a simple identifier — no dots (receiver.method or
    // path::method) and no colons (associated function Type::method).
    !callee.is_empty()
        && !callee.contains('.')
        && !callee.contains("::")
        && callee
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Returns `true` when a Rust pattern looks like a function declaration
/// (`fn $NAME(...)` or `pub fn $NAME(...)`) that is missing a return type.
/// Under `strictness=signature` with `selector=function_item`, such patterns
/// only match functions that have NO return type, which is usually not what
/// the caller intends.
pub(super) fn looks_like_rust_fn_missing_return(pattern: &str) -> bool {
    let trimmed = pattern.trim();
    // Strip optional visibility modifiers (pub, pub(crate), pub(super), etc.)
    let after_vis = if let Some(rest) = trimmed.strip_prefix("pub") {
        let rest = rest.trim_start();
        if let Some(rest) = rest.strip_prefix('(') {
            // pub(crate), pub(super), etc. -- skip to closing paren
            let rest = rest.trim_start_matches(|c: char| c != ')');
            rest.strip_prefix(')').unwrap_or("").trim_start()
        } else {
            rest
        }
    } else {
        trimmed
    };
    // Strip optional qualifiers (async, const, unsafe, extern) before `fn`
    let after_qualifiers = after_vis
        .trim_start_matches("async ")
        .trim_start_matches("const ")
        .trim_start_matches("unsafe ")
        .trim_start_matches("extern ");
    // Must start with `fn `
    let after_fn = match after_qualifiers.strip_prefix("fn ") {
        Some(rest) => rest,
        None => return false,
    };
    // Must contain `(` to have parameters
    if !after_fn.contains('(') {
        return false;
    }
    // Must NOT contain `->` (no return type)
    !after_fn.contains("->")
}

pub(super) fn fragment_pattern_hint(
    request: &StructuralSearchRequest,
    language: AstGrepLanguage,
) -> String {
    let Some(trimmed) = request.pattern() else {
        return format!(
            "Pattern is required for {} syntax queries.",
            language.display_name()
        );
    };
    let mut message = format!(
        "Pattern looks like a code fragment, not standalone parseable {} syntax for `action='structural'`.",
        language.display_name()
    );

    if language == AstGrepLanguage::Rust
        && (trimmed.starts_with("Result<")
            || trimmed.starts_with("-> Result<")
            || trimmed.contains("-> Result<"))
    {
        message.push_str(
            " For Result return-type queries, anchor it in a full signature like `fn $NAME($$ARGS) -> Result<$T> { $$BODY }`.",
        );
    } else if language == AstGrepLanguage::Rust
        && looks_like_rust_fn_missing_return(trimmed)
        && matches!(request.strictness, Some(StructuralStrictness::Signature))
    {
        message.push_str(
            " With `strictness=signature` and `selector=function_item`, the pattern must include the full signature including the return type. \
             Use `pub fn $NAME($$$ARGS) -> $RET { $$$BODY }` to match all public functions, or drop `strictness` to match functions regardless of return type.",
        );
    } else if language == AstGrepLanguage::Rust && looks_like_rust_method_call_fragment(trimmed) {
        message.push_str(
            " In Rust, method calls like `unwrap_or($T)` need a receiver. \
             Use `$X.unwrap_or($T::default())` to match method calls on any receiver, \
             where `$X` captures the receiver expression. \
             For associated functions like `Type::method($A)`, use the full qualified path in the pattern.",
        );
    } else if language == AstGrepLanguage::Go && looks_like_go_call_pattern(trimmed) {
        message.push_str(
            " In Go, tree-sitter parses bare call-like fragments (e.g. `fmt.Println($A)`) as type conversions, not call expressions. \
             Wrap the call in surrounding parseable code like `func t() { fmt.Println($A) }` and use `selector: call_expression` to match only function calls. \
             Note: contextual patterns with `context` + `selector` require the CLI skill path via `unified_exec`.",
        );
    } else if language == AstGrepLanguage::Html && looks_like_html_attribute_pattern(trimmed) {
        message.push_str(
            " In HTML, bare attribute expressions like `class=$VAL` are not standalone parseable code. \
             Use `kind: attribute_name` to match attribute names, `kind: attribute_value` for values, \
             or `kind: element` with `has` to match elements containing specific attributes. \
             For example, to match elements with a specific attribute, use `kind: element` with \
             `has: { kind: attribute_name, regex: \"^class$\" }`.",
        );
    } else if language == AstGrepLanguage::Html && looks_like_html_tag_pattern(trimmed) {
        message.push_str(
            " In HTML, tree-sitter parses tag structures as `element` nodes with `tag_name` and `attribute` children. \
             Bare tag fragments like `<$TAG>` are not standalone code. Use `kind: element` with \
             `has: { field: tag_name, pattern: $TAG }` to match elements by tag name, \
             or `kind: tag_name` to match tag name nodes directly.",
        );
    } else if language == AstGrepLanguage::Java && looks_like_java_declaration_fragment(trimmed) {
        message.push_str(
            " In Java, bare type declarations, annotations, and field fragments are not standalone parseable code. \
             For field or variable declarations with modifiers/annotations, use `kind: field_declaration` with \
             `has: { field: type, regex: \"^TypeName$\" }` to match by type regardless of modifiers. \
             For annotations, use `kind: marker_annotation` or `kind: annotation` with `inside` to scope \
             to the declaration you care about. Wrap bare fragments in a full class body like \
             `class _ { $TYPE $VAR; }` and use `selector` to target the inner node.",
        );
    } else if language == AstGrepLanguage::Ruby && looks_like_ruby_block_fragment(trimmed) {
        message.push_str(
            " In Ruby, bare block fragments like `{ |$V| $V.$METHOD }` or `do |$V| $V.$METHOD end` are not \
             standalone parseable code. Wrap the block in a method call like `$LIST.select { |$V| $V.$METHOD }` \
             and use `selector: call` to match the outer call. For symbol-to-proc patterns, match the enclosing \
             method call directly with `$LIST.$ITER(&:$METHOD)`. Key Ruby tree-sitter node kinds: `call` for \
             method calls, `method_call` for keyword-style calls, `block` for `{{ }}` blocks, `do_block` for \
             `do...end` blocks, `symbol` for `:name` literals, `assignment` for variable assignments.",
        );
    } else if language == AstGrepLanguage::Css && looks_like_css_selector_fragment(trimmed) {
        message.push_str(
            " In CSS, bare selectors like `.class` or `#id` are not standalone parseable code. \
             Use `kind: rule_set` with `has` to match rule sets containing specific selectors, or \
             `kind: selector` to match selector nodes.",
        );
    } else if language == AstGrepLanguage::Python && looks_like_python_decorator_fragment(trimmed) {
        message.push_str(
            " In Python, bare decorators like `@property` are not standalone parseable code. \
             Wrap with the decorated definition and use `selector: decorated_definition` to match.",
        );
    } else if language == AstGrepLanguage::Bash && !trimmed.contains(';') {
        message.push_str(
            " In Bash, bare command fragments need script context. Use `kind: command` with `has` to match specific commands.",
        );
    } else {
        message.push_str(
            " Wrap the target in surrounding parseable code, then use `selector` only to focus the real subnode inside that larger pattern.",
        );
    }

    message.push_str(" Retry `unified_search` with `action='structural'` using a larger parseable pattern before switching tools. Do not retry the same fragment with grep if syntax matters.");
    message
}
