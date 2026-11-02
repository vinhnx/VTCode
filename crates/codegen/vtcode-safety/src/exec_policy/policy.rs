//! Policy types for execution control.

use serde::{Deserialize, Serialize};
use std::{collections::HashSet, default::Default};

/// Decision made by a policy rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
#[must_use = "execution policy decisions must be enforced"]
pub enum Decision {
    /// Allow the command to execute.
    Allow,

    /// Require user confirmation before executing.
    #[default]
    Prompt,

    /// Forbid the command from executing.
    Forbidden,
}

/// A prefix-based rule for matching commands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrefixRule {
    /// The command pattern to match.
    pub(crate) pattern: Vec<String>,

    /// The decision when the pattern matches.
    pub(crate) decision: Decision,
}

impl PrefixRule {
    /// Create a new prefix rule.
    pub fn new(pattern: Vec<String>, decision: Decision) -> Self {
        Self { pattern, decision }
    }

    /// Check if a command matches this rule. Zero-token patterns never match.
    #[must_use]
    pub fn matches(&self, command: &[String]) -> bool {
        matches_prefix(&self.pattern, command)
    }
}

/// Empty token conjunctions must never authorize a command.
pub(super) fn matches_prefix(pattern: &[String], command: &[String]) -> bool {
    !pattern.is_empty() && command.starts_with(pattern)
}

/// Result of matching a command against a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleMatch {
    /// Matched a prefix rule.
    PrefixRuleMatch { rule: PrefixRule, decision: Decision },

    /// Matched via heuristics (no explicit rule).
    HeuristicsRuleMatch { decision: Decision },
}

impl RuleMatch {
    /// Get the decision from the match.
    fn decision(&self) -> Decision {
        match self {
            Self::PrefixRuleMatch { decision, .. } => *decision,
            Self::HeuristicsRuleMatch { decision } => *decision,
        }
    }

    /// Check if this match came from an explicit policy rule.
    fn is_policy_match(&self) -> bool {
        matches!(self, Self::PrefixRuleMatch { .. })
    }
}

/// Result of evaluating multiple commands against a policy.
#[derive(Debug, Clone)]
pub struct PolicyEvaluation {
    /// The overall decision.
    decision: Decision,

    /// All rules that matched.
    matched_rules: Vec<RuleMatch>,
}

/// Execution policy containing rules for command authorization.
#[derive(Debug, Clone, Default)]
pub struct Policy {
    /// Prefix rules in order of priority (first match wins).
    prefix_rules: Vec<PrefixRule>,
}

impl Policy {
    /// Create an empty policy.
    pub fn empty() -> Self {
        Self { prefix_rules: Vec::new() }
    }

    /// Add a prefix rule to the policy.
    ///
    /// # Errors
    /// Returns an error for a zero-token pattern without changing the policy.
    pub fn add_prefix_rule(&mut self, pattern: &[String], decision: Decision) -> anyhow::Result<()> {
        anyhow::ensure!(!pattern.is_empty(), "prefix pattern cannot be empty");
        self.prefix_rules.push(PrefixRule::new(pattern.to_vec(), decision));
        Ok(())
    }

    /// The prefix rules in evaluation order (first match wins).
    pub fn rules(&self) -> &[PrefixRule] {
        &self.prefix_rules
    }

    /// Prepend a higher-precedence layer of rules.
    ///
    /// Used when merging rule-file layers (workspace rules over user rules):
    /// a pattern that already exists — either in this policy or earlier within
    /// `rules` — keeps its first occurrence, so the highest-precedence layer
    /// wins and evaluation order (`check`) stays deterministic.
    pub fn prepend_layer(&mut self, rules: impl IntoIterator<Item = PrefixRule>) {
        let rules: Vec<_> = rules.into_iter().collect();
        if rules.is_empty() {
            return;
        }
        // Membership is separate from evaluation order; the vector still defines precedence.
        let mut seen: HashSet<&[String]> = self.prefix_rules.iter().map(|rule| rule.pattern.as_slice()).collect();
        let keep: Vec<_> = rules.iter().map(|rule| seen.insert(rule.pattern.as_slice())).collect();
        drop(seen);
        let new_rules: Vec<_> = rules
            .into_iter()
            .zip(keep)
            .filter_map(|(rule, keep)| keep.then_some(rule))
            .collect();
        drop(self.prefix_rules.splice(0..0, new_rules));
    }

    /// Check a single command against the policy.
    pub fn check(&self, command: &[String]) -> RuleMatch {
        for rule in &self.prefix_rules {
            if rule.matches(command) {
                return RuleMatch::PrefixRuleMatch { rule: rule.clone(), decision: rule.decision };
            }
        }

        // No explicit rule matched - use heuristics
        RuleMatch::HeuristicsRuleMatch { decision: Decision::Prompt }
    }

    /// Check multiple commands against the policy, accepting borrowed slices
    /// as well as references to command vectors without copying arguments.
    pub fn check_multiple<I, F>(&self, commands: I, heuristics_fallback: &F) -> PolicyEvaluation
    where
        I: Iterator,
        I::Item: AsRef<[String]>,
        F: Fn(&[String]) -> Decision,
    {
        let mut matched_rules = Vec::new();
        let mut overall_decision = Decision::Allow;

        for command in commands {
            let command = command.as_ref();
            let rule_match = self.check(command);

            // Apply heuristics for non-policy matches
            let decision = match &rule_match {
                RuleMatch::PrefixRuleMatch { decision, .. } => *decision,
                RuleMatch::HeuristicsRuleMatch { .. } => heuristics_fallback(command),
            };

            // Track the most restrictive decision
            overall_decision = match (overall_decision, decision) {
                (Decision::Forbidden, _) | (_, Decision::Forbidden) => Decision::Forbidden,
                (Decision::Prompt, _) | (_, Decision::Prompt) => Decision::Prompt,
                (Decision::Allow, Decision::Allow) => Decision::Allow,
            };

            matched_rules.push(rule_match);
        }

        PolicyEvaluation { decision: overall_decision, matched_rules }
    }

    /// Get all prefix rules.
    pub fn prefix_rules(&self) -> &[PrefixRule] {
        &self.prefix_rules
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_prefix_never_matches() {
        let rule = PrefixRule::new(Vec::new(), Decision::Allow);
        assert!(!rule.matches(&[]));
        assert!(!rule.matches(&["untrusted".into()]));
    }

    #[test]
    fn empty_deserialized_rule_and_layer_cannot_match() {
        let rule: PrefixRule = serde_json::from_str(r#"{"pattern":[],"decision":"allow"}"#).unwrap();
        let mut policy = Policy::empty();
        policy.prepend_layer([rule]);
        for command in [vec![], vec!["untrusted".into()]] {
            assert_eq!(policy.check(&command), RuleMatch::HeuristicsRuleMatch { decision: Decision::Prompt });
        }
    }

    #[test]
    fn rejected_insertion_preserves_rules() {
        let mut policy = Policy::empty();
        policy.add_prefix_rule(&["git".into()], Decision::Forbidden).unwrap();
        let before = policy.rules().to_vec();
        for decision in [Decision::Allow, Decision::Prompt, Decision::Forbidden] {
            assert!(policy.add_prefix_rule(&[], decision).is_err());
            assert_eq!(policy.rules(), before);
        }
    }

    #[test]
    fn layer_deduplication_matches_pairwise_reference() {
        let choices = [
            PrefixRule::new(vec!["a".into()], Decision::Allow),
            PrefixRule::new(vec!["b".into()], Decision::Prompt),
            PrefixRule::new(vec!["a".into()], Decision::Forbidden),
            PrefixRule::new(vec!["a".into(), "b".into()], Decision::Forbidden),
            PrefixRule::new(vec![], Decision::Allow),
        ];
        for existing in &choices {
            for first in &choices {
                for second in &choices {
                    let mut policy = Policy { prefix_rules: vec![existing.clone()] };
                    let incoming = [first.clone(), second.clone(), first.clone()];
                    let mut expected = Vec::<PrefixRule>::new();
                    for rule in &incoming {
                        if !expected.iter().chain(policy.rules()).any(|prior| prior.pattern == rule.pattern) {
                            expected.push(rule.clone());
                        }
                    }
                    expected.push(existing.clone());
                    policy.prepend_layer(incoming);
                    assert_eq!(policy.rules(), expected);
                    for command in [vec![], vec!["a".into()], vec!["b".into()], vec!["a".into(), "b".into()]] {
                        let reference = Policy { prefix_rules: expected.clone() };
                        assert_eq!(policy.check(&command), reference.check(&command));
                    }
                }
            }
        }
    }

    #[test]
    fn layer_deduplication_preserves_order_and_existing_duplicates() {
        let mut policy = Policy::empty();
        policy.add_prefix_rule(&["git".into()], Decision::Prompt).unwrap();
        policy.add_prefix_rule(&["git".into()], Decision::Forbidden).unwrap();
        let existing = policy.rules().to_vec();
        let first = PrefixRule::new(vec!["cargo".into()], Decision::Allow);
        policy.prepend_layer([
            first.clone(),
            PrefixRule::new(vec!["git".into()], Decision::Allow),
            PrefixRule::new(vec!["cargo".into()], Decision::Forbidden),
            PrefixRule::new(vec!["cargo".into(), "build".into()], Decision::Forbidden),
        ]);
        assert_eq!(policy.rules().len(), 4);
        assert_eq!(policy.rules()[0], first);
        assert_eq!(&policy.rules()[2..], existing);
        assert_eq!(policy.check(&["cargo".into(), "build".into()]).decision(), Decision::Allow);
        assert_eq!(policy.check(&["git".into()]).decision(), Decision::Prompt);
        let before = policy.rules().to_vec();
        policy.prepend_layer([]);
        assert_eq!(policy.rules(), before);
    }

    #[test]
    fn nonempty_matching_agrees_with_prefix_oracle() {
        // Enumerate asymmetric tokens, including an empty-string argument.
        let mut vectors = vec![Vec::new()];
        for _ in 0..3 {
            let previous = vectors.clone();
            for prefix in previous {
                for token in ["git", "push", ""] {
                    let mut next = prefix.clone();
                    next.push(token.to_string());
                    vectors.push(next);
                }
            }
        }
        for pattern in &vectors {
            let rule = PrefixRule::new(pattern.clone(), Decision::Allow);
            for command in &vectors {
                let expected = !pattern.is_empty() && command.get(..pattern.len()) == Some(pattern.as_slice());
                assert_eq!(rule.matches(command), expected, "{pattern:?} vs {command:?}");
            }
        }
    }

    #[test]
    fn borrowed_commands_preserve_rule_precedence_and_fallback_arguments() {
        let mut policy = Policy::empty();
        let first = PrefixRule::new(vec!["git".into()], Decision::Prompt);
        policy.add_prefix_rule(&first.pattern, first.decision).unwrap();
        policy.add_prefix_rule(&["git".into(), "push".into()], Decision::Allow).unwrap();
        let known = ["git".to_owned(), "push".to_owned()];
        let unknown = ["custom".to_owned(), "--dangerous".to_owned()];
        let fallback_calls = std::cell::Cell::new(0);
        let evaluation = policy.check_multiple([known.as_slice(), unknown.as_slice()].into_iter(), &|command| {
            assert!(std::ptr::eq(command, unknown.as_slice()), "heuristics must borrow the original argv");
            fallback_calls.set(fallback_calls.get() + 1);
            Decision::Forbidden
        });
        assert_eq!(fallback_calls.get(), 1);
        assert_eq!(evaluation.decision, Decision::Forbidden);
        assert_eq!(
            evaluation.matched_rules,
            vec![
                RuleMatch::PrefixRuleMatch { rule: first, decision: Decision::Prompt },
                RuleMatch::HeuristicsRuleMatch { decision: Decision::Prompt },
            ]
        );
        let empty = policy.check_multiple(std::iter::empty::<&[String]>(), &|_| panic!("no commands"));
        assert_eq!(empty.decision, Decision::Allow);
        assert!(empty.matched_rules.is_empty());
    }

    #[test]
    fn first_match_and_restrictive_aggregation_are_preserved() {
        let mut policy = Policy::empty();
        policy.add_prefix_rule(&["git".into()], Decision::Prompt).unwrap();
        policy.add_prefix_rule(&["git".into(), "push".into()], Decision::Allow).unwrap();
        policy.add_prefix_rule(&["deny".into()], Decision::Forbidden).unwrap();
        policy.add_prefix_rule(&["allow".into()], Decision::Allow).unwrap();
        let mut commands = vec![vec!["allow".into()]];
        assert_eq!(policy.check_multiple(commands.iter(), &|_| Decision::Allow).decision, Decision::Allow);
        commands.push(vec!["git".into(), "push".into()]);
        assert_eq!(policy.check_multiple(commands.iter(), &|_| Decision::Allow).decision, Decision::Prompt);
        commands.push(vec!["deny".into()]);
        assert_eq!(policy.check_multiple(commands.iter(), &|_| Decision::Allow).decision, Decision::Forbidden);
        let unknown = [vec!["unknown".into()]];
        assert_eq!(policy.check_multiple(unknown.iter(), &|_| Decision::Forbidden).decision, Decision::Forbidden);
    }

    #[test]
    fn test_prefix_rule_matching() {
        let rule = PrefixRule::new(vec!["cargo".to_string(), "build".to_string()], Decision::Allow);

        assert!(rule.matches(&["cargo".to_string(), "build".to_string()]));
        assert!(rule.matches(&["cargo".to_string(), "build".to_string(), "--release".to_string()]));
        assert!(!rule.matches(&["cargo".to_string(), "test".to_string()]));
        assert!(!rule.matches(&["cargo".to_string()]));
    }

    #[test]
    fn test_policy_check() {
        let mut policy = Policy::empty();
        policy
            .add_prefix_rule(&["cargo".to_string(), "build".to_string()], Decision::Allow)
            .unwrap();
        policy.add_prefix_rule(&["rm".to_string()], Decision::Forbidden).unwrap();

        let allow = policy.check(&["cargo".to_string(), "build".to_string()]);
        assert_eq!(allow.decision(), Decision::Allow);
        assert!(allow.is_policy_match());

        let forbidden = policy.check(&["rm".to_string(), "-rf".to_string()]);
        assert_eq!(forbidden.decision(), Decision::Forbidden);

        let heuristics = policy.check(&["unknown".to_string()]);
        assert!(!heuristics.is_policy_match());
    }

    #[test]
    fn test_policy_evaluation() {
        let mut policy = Policy::empty();
        policy.add_prefix_rule(&["echo".to_string()], Decision::Allow).unwrap();
        policy.add_prefix_rule(&["rm".to_string()], Decision::Forbidden).unwrap();

        let commands = [
            vec!["echo".to_string(), "hello".to_string()],
            vec!["rm".to_string(), "-rf".to_string()],
        ];

        let evaluation = policy.check_multiple(commands.iter(), &|_| Decision::Prompt);

        // Should be forbidden because one command is forbidden
        assert_eq!(evaluation.decision, Decision::Forbidden);
    }
}
