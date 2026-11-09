//! Test-quality signal for agentic testing (Dan Luu `agentic-testing`).
//!
//! `pass@k` measures whether runs pass. This module measures whether the
//! *tests themselves* are high-value: asymmetric/boundary coverage, structured
//! property-based testing with shrinking, and avoidance of panic-only or
//! identical-fixture traps.
//!
//! Heuristics are intentionally cheap string analyses over test sources so they
//! can run in CI without executing mutations. They are a triage signal, not a
//! proof of correctness.

use serde::{Deserialize, Serialize};

/// Aggregate facts about a test source file or inline test module.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct TestQualitySummary {
    /// Number of test cases (`#[test]`-family attributes).
    pub total_tests: u64,
    /// Tests with no oracle (`assert*`, `prop_assert*`, `insta::*`).
    pub panic_only_tests: u64,
    /// Tests exercising asymmetric, boundary, order-swap, or reversal cases.
    pub asymmetric_tests: u64,
    /// Property tests using structured strategies (`prop_oneof`, `prop_compose`, `Just`, `strategy`).
    pub structured_pbt_tests: u64,
}

impl TestQualitySummary {
    /// Fraction of tests with no value oracle. Lower is better.
    pub fn panic_only_ratio(&self) -> f64 {
        if self.total_tests == 0 {
            return 0.0;
        }
        self.panic_only_tests as f64 / self.total_tests as f64
    }

    /// Fraction of tests covering asymmetric/boundary behavior. Higher is better.
    pub fn asymmetric_coverage(&self) -> f64 {
        if self.total_tests == 0 {
            return 0.0;
        }
        self.asymmetric_tests as f64 / self.total_tests as f64
    }

    /// Fraction of tests using structured PBT. Higher is better.
    pub fn structured_pbt_ratio(&self) -> f64 {
        if self.total_tests == 0 {
            return 0.0;
        }
        self.structured_pbt_tests as f64 / self.total_tests as f64
    }

    /// Luu-inspired bar: mostly oracle-backed, some asymmetric coverage.
    pub fn meets_agentic_testing_bar(&self) -> bool {
        self.total_tests > 0 && self.panic_only_ratio() <= 0.5 && self.asymmetric_coverage() >= 0.2
    }

    /// Merge another summary into this aggregate.
    pub fn merge(&mut self, other: &Self) {
        self.total_tests = self.total_tests.saturating_add(other.total_tests);
        self.panic_only_tests = self.panic_only_tests.saturating_add(other.panic_only_tests);
        self.asymmetric_tests = self.asymmetric_tests.saturating_add(other.asymmetric_tests);
        self.structured_pbt_tests = self.structured_pbt_tests.saturating_add(other.structured_pbt_tests);
    }
}

/// Analyze a Rust test source string with cheap heuristics.
pub fn analyze_test_source(source: &str) -> TestQualitySummary {
    let mut summary = TestQualitySummary::default();
    for block in split_test_blocks(source) {
        summary.total_tests = summary.total_tests.saturating_add(1);
        if is_panic_only(block) {
            summary.panic_only_tests = summary.panic_only_tests.saturating_add(1);
        }
        if is_asymmetric(block) {
            summary.asymmetric_tests = summary.asymmetric_tests.saturating_add(1);
        }
        if is_structured_pbt(block) {
            summary.structured_pbt_tests = summary.structured_pbt_tests.saturating_add(1);
        }
    }
    summary
}

fn split_test_blocks(source: &str) -> Vec<&str> {
    let mut blocks = Vec::new();
    let mut rest = source;
    loop {
        // Prefer `#[test]`-family markers so a `proptest!` header is never
        // counted as its own test; fall back to bare `proptest!` only when no
        // inner test attribute exists.
        let attr_pos = ["#[test]", "#[tokio::test]", "#[rstest]"]
            .iter()
            .filter_map(|marker| rest.find(marker))
            .min();
        let pos = match attr_pos {
            Some(pos) => pos,
            None => match rest.find("proptest!") {
                Some(pos) => pos,
                None => break,
            },
        };
        let next_attr = ["#[test]", "#[tokio::test]", "#[rstest]"]
            .iter()
            .filter_map(|marker| rest.get(pos + 1..)?.find(marker).map(|next| pos + 1 + next))
            .min();
        let next_prop = rest
            .get(pos + 1..)
            .and_then(|tail| tail.find("proptest!").map(|next| pos + 1 + next));
        let next_test = match (next_attr, next_prop) {
            (Some(attr), _) => Some(attr),
            (None, prop) => prop,
        };
        let end = next_test.unwrap_or(rest.len());
        let Some(block) = rest.get(pos..end) else { break };
        blocks.push(block);
        if next_test.is_none() {
            break;
        }
        let Some(tail) = rest.get(end..) else { break };
        rest = tail;
    }
    blocks
}

fn is_panic_only(block: &str) -> bool {
    // `prop_assume!` filters inputs and `panic!` fails explicitly; neither is a
    // value oracle. `assert!`/`assert_*` covers `assert_*`/`prop_assert*` while
    // avoiding prose matches like `assertion`.
    !(block.contains("assert!") || block.contains("assert_") || block.contains("insta::") || block.contains("expect("))
}

fn is_asymmetric(block: &str) -> bool {
    let lowered = block.to_ascii_lowercase();
    if lowered.contains("both sides") {
        return true;
    }
    // Match whole tokens so `bright`/`leftover`/`border` do not count as
    // `right`/`left`/`order` coverage.
    let tokens: Vec<&str> = lowered
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|token| !token.is_empty())
        .collect();
    const STEMS: &[&str] = &[
        "asymmetr",
        "boundary",
        "boundaries",
        "revers",
        "swap",
        "transpos",
        "palindrom",
        "identical",
    ];
    const WORDS: &[&str] = &["order", "left", "right"];
    tokens
        .iter()
        .any(|token| STEMS.iter().any(|stem| token.contains(stem)) || WORDS.contains(token))
}

fn is_structured_pbt(block: &str) -> bool {
    block.contains("prop_oneof")
        || block.contains("prop_compose")
        || block.contains("Just(")
        || block.contains("strategy")
        || block.contains("prop::")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_oracle_backed_and_asymmetric_tests() {
        let source = r#"
            #[test]
            fn transposed_order_is_rejected_asymmetric_boundary() {
                assert_ne!(vec!["a", "b"], vec!["b", "a"]);
            }
            #[test]
            fn never_panics_smoke() {
                let _value = vec![1, 2, 3].len();
            }
        "#;
        let summary = analyze_test_source(source);
        assert_eq!(summary.total_tests, 2);
        assert_eq!(summary.panic_only_tests, 1);
        assert_eq!(summary.asymmetric_tests, 1);
        assert!((summary.panic_only_ratio() - 0.5).abs() < f64::EPSILON);
        assert!((summary.asymmetric_coverage() - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn detects_structured_proptest_strategies() {
        let source = r#"
            proptest! {
                #[test]
                fn order_swap_uses_structured_strategy(a in prop::option::of(Just(1))) {
                    prop_assert!(a.is_some());
                }
            }
        "#;
        let summary = analyze_test_source(source);
        assert_eq!(summary.total_tests, 1);
        assert_eq!(summary.panic_only_tests, 0);
        assert_eq!(summary.structured_pbt_tests, 1);
        assert!(summary.meets_agentic_testing_bar());
    }

    #[test]
    fn empty_source_meets_no_bar_but_reports_zero_ratios() {
        let summary = analyze_test_source("");
        assert_eq!(summary.total_tests, 0);
        assert_eq!(summary.panic_only_ratio(), 0.0);
        assert_eq!(summary.asymmetric_coverage(), 0.0);
        assert!(!summary.meets_agentic_testing_bar());
    }

    #[test]
    fn prop_assume_and_panic_are_not_oracles() {
        let assume_only = "#[test] fn filtered() { prop_assume!(true); }";
        assert_eq!(analyze_test_source(assume_only).panic_only_tests, 1);
        let panic_only = "#[test] fn todo() { panic!(\"todo\"); }";
        assert_eq!(analyze_test_source(panic_only).panic_only_tests, 1);
        let asserted = "#[test] fn checked() { assert!(true); }";
        assert_eq!(analyze_test_source(asserted).panic_only_tests, 0);
    }

    #[test]
    fn asymmetric_matching_uses_word_boundaries() {
        assert_eq!(analyze_test_source("#[test] fn b() { let brightness = 1; assert!(true); }").asymmetric_tests, 0);
        assert_eq!(analyze_test_source("#[test] fn b() { let leftover = 1; assert!(true); }").asymmetric_tests, 0);
        assert_eq!(analyze_test_source("#[test] fn b() { fn render_border() {} assert!(true); }").asymmetric_tests, 0);
        assert_eq!(
            analyze_test_source("#[test] fn swapped() { assert_ne!(vec![\"a\"], vec![\"b\"]); }").asymmetric_tests,
            1
        );
    }

    #[test]
    fn counts_tokio_tests() {
        let source = "#[tokio::test] async fn async_case() { assert!(true); }";
        assert_eq!(analyze_test_source(source).total_tests, 1);
    }

    #[test]
    fn merge_sums_counters() {
        let mut left = analyze_test_source("#[test] fn a() { assert!(true); }");
        let right = analyze_test_source("#[test] fn b() { let _x = 1; }");
        left.merge(&right);
        assert_eq!(left.total_tests, 2);
        assert_eq!(left.panic_only_tests, 1);
    }
}
