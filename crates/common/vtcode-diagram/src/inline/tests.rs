use std::collections::HashMap;

use super::*;

fn theme(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs.iter().map(|(key, value)| (key.to_string(), value.to_string())).collect()
}

#[test]
fn nested_fallback_resolves_inside_out() {
    let svg = r##"<rect fill="var(--a, var(--b, #fff))"/>"##;
    // Inner defined: inner wins over the outer fallback literal.
    let (out, unresolved) = inline_css_vars(svg, &theme(&[("--b", "#111")]));
    assert_eq!(out, r##"<rect fill="#111"/>"##, "{out}");
    assert_eq!(unresolved, 0);
    // Neither defined: innermost literal survives.
    let (out, unresolved) = inline_css_vars(svg, &theme(&[]));
    assert_eq!(out, r##"<rect fill="#fff"/>"##, "{out}");
    assert_eq!(unresolved, 0);
    // Outer defined: fallbacks never consulted.
    let (out, _) = inline_css_vars(svg, &theme(&[("--a", "#222"), ("--b", "#111")]));
    assert_eq!(out, r##"<rect fill="#222"/>"##, "{out}");
}

#[test]
fn text_node_definition_does_not_poison_map() {
    let svg = concat!(
        r##"<svg style="--_fg: blue;">"##,
        r##"<text>--_fg: red; var(--_fg)</text>"##,
        r##"<rect fill="var(--_fg)"/>"##,
        r##"</svg>"##,
    );
    let (out, unresolved) = inline_css_vars(svg, &theme(&[]));
    assert!(out.contains(r##"<text>--_fg: red; var(--_fg)</text>"##), "text byte-identical: {out}");
    assert!(out.contains(r##"<rect fill="blue"/>"##), "tag uses real def: {out}");
    assert!(!out.contains("red\""), "poison never substituted: {out}");
    assert_eq!(unresolved, 0);
}

#[test]
fn quoted_gt_keeps_tag_mode() {
    let svg = r##"<rect title="a>b" fill="var(--c, #123)"/>"##;
    let (out, unresolved) = inline_css_vars(svg, &theme(&[]));
    assert_eq!(out, r##"<rect title="a>b" fill="#123"/>"##, "{out}");
    assert_eq!(unresolved, 0);
    // Single quotes behave the same.
    let svg = r##"<rect title='a>b' fill="var(--c, #123)"/>"##;
    let (out, _) = inline_css_vars(svg, &theme(&[]));
    assert!(out.contains("fill=\"#123\""), "{out}");
}

#[test]
fn unbalanced_paren_does_not_panic_and_keeps_literal() {
    // Debug overflow checks are on for tests: an underflow would panic.
    let svg = r##"<svg style="--x: a)b;"><rect fill="var(--x, #000)"/></svg>"##;
    let (out, unresolved) = inline_css_vars(svg, &theme(&[]));
    assert!(out.contains("fill=\"a)b\""), "{out}");
    assert_eq!(unresolved, 0);
}

#[test]
fn pele_alias_chain_resolves_to_theme() {
    let svg = concat!(
        r##"<svg style="--_fg:var(--pele-fg,#1f1f1f);" fill="var(--_fg)">"##,
        r##"<rect fill="var(--_fg)"/>"##,
        r##"</svg>"##,
    );
    let (out, unresolved) = inline_css_vars(svg, &theme(&[("--pele-fg", "#e8e8e8")]));
    assert!(!out.contains("var("), "fully resolved: {out}");
    assert!(out.contains("fill=\"#e8e8e8\""), "{out}");
    assert_eq!(unresolved, 0);
}

#[test]
fn unknown_var_without_fallback_counts_unresolved_and_stays_literal() {
    let svg = r##"<rect fill="var(--nope)" stroke="var(--flat, none)"/>"##;
    let (out, unresolved) = inline_css_vars(svg, &theme(&[]));
    assert_eq!(unresolved, 1, "{out}");
    assert!(out.contains("var(--nope)"), "kept literally: {out}");
    assert!(out.contains("stroke=\"none\""), "sibling still resolves: {out}");
}
