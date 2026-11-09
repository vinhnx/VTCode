use std::time::Duration;

use super::*;
use crate::{MAX_DIAGRAM_SOURCE_CHARS, PELE_BUNDLE_JS};

fn sandbox() -> DiagramSandbox {
    DiagramSandbox::new(PELE_BUNDLE_JS).expect("real bundle loads")
}

#[test]
fn sandbox_renders_flowchart_geometry() {
    let rendered = sandbox()
        .render("flowchart TD\n  A[Mermaid] --> B[Pele]", Duration::from_secs(10))
        .expect("valid source renders");
    assert!(rendered.svg.contains("<svg"), "svg element present");
    assert!(rendered.width > 0.0 && rendered.height > 0.0, "nonzero size");
    assert!(!rendered.svg.contains("<a "), "links:false drops anchors");
}

#[test]
fn sandbox_reports_pele_errors_as_js() {
    let error = sandbox()
        .render("flowchart TD\n  A[unclosed", Duration::from_secs(10))
        .expect_err("invalid source fails");
    assert!(matches!(error, RenderError::Js(_)), "typed as JS: {error:?}");
    assert!(error.to_string().contains("Parse error"), "message preserved: {error}");
}

#[test]
fn sandbox_rejects_oversized_source_at_the_utf16_boundary() {
    let sandbox = sandbox();
    let too_large = "x".repeat(MAX_DIAGRAM_SOURCE_CHARS + 1);
    assert_eq!(sandbox.render(&too_large, Duration::from_secs(10)), Err(RenderError::TooLarge));
    // Exactly at the limit the JS engine runs: garbage fails fast in the
    // parser instead of tripping the size gate.
    let at_limit = "x".repeat(MAX_DIAGRAM_SOURCE_CHARS);
    assert!(matches!(sandbox.render(&at_limit, Duration::from_secs(10)), Err(RenderError::Js(_))));
}

/// 1200-node chain ≈ 40KB: under the source gate, slow enough (tens
/// of milliseconds even in release) that a 1ms budget always trips.
fn big_chain() -> String {
    let mut source = String::from("flowchart LR\n");
    for index in 0..1200 {
        use std::fmt::Write;
        writeln!(source, "  N{index}[node {index}] --> N{}[node {}]", index + 1, index + 1).expect("fixture builds");
    }
    source
}

#[test]
fn sandbox_enforces_the_time_budget() {
    let source = big_chain();
    let sandbox = sandbox();
    // Control first: the same source renders under a generous budget.
    let _control = sandbox.render(&source, Duration::from_secs(30)).expect("control renders");
    assert!(matches!(sandbox.render(&source, Duration::from_millis(1)), Err(RenderError::Js(_))));
}

#[test]
fn sandbox_rejects_a_bundle_without_pele() {
    let error = match DiagramSandbox::new("var notPele = 1;") {
        Ok(_) => panic!("bundle without pele must fail"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("pele.render"), "names the contract: {error}");
}

#[test]
fn real_bundle_defines_the_globals_both_consumers_call() {
    assert!(PELE_BUNDLE_JS.contains("var pele="), "global present");
    assert!(PELE_BUNDLE_JS.contains("mount:"), "mount exported for the browser preview");
    assert!(PELE_BUNDLE_JS.contains("render:"), "render exported for the sandbox");
}

#[test]
fn real_bundle_has_no_script_breakout_sequences() {
    // Consumers inline the bundle into a <script> block, so a future
    // bundle containing these sequences would break page parsing.
    assert!(!PELE_BUNDLE_JS.to_lowercase().contains("</script"), "no script close");
    assert!(!PELE_BUNDLE_JS.contains("<!--"), "no comment open");
}
