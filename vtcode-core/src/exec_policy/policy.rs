//! Policy types for execution control.

use serde::{Deserialize, Serialize};
use std::default::Default;

/// Decision made by a policy rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
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
    pub pattern: Vec<String>,

    /// The decision when the pattern matches.
    pub decision: Decision,
}

impl PrefixRule {
    /// Create a new prefix rule.
    pub fn new(pattern: Vec<String>, decision: Decision) -> Self {
        Self { pattern, decision }
    }

    /// Check if a command matches this rule.
    pub fn matches(&self, command: &[String]) -> bool {
        if command.len() < self.pattern.len() {
            return false;
        }
        self.pattern
            .iter()
            .zip(command.iter())
            .all(|(pattern, cmd)| pattern == cmd)
    }
}

/// Result of matching a command against a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleMatch {
    /// Matched a prefix rule.
    PrefixRuleMatch {
        rule: PrefixRule,
        decision: Decision,
    },

    /// Matched via heuristics (no explicit rule).
    HeuristicsRuleMatch { decision: Decision },
}

impl RuleMatch {
    /// Get the decision from the match.
    pub fn decision(&self) -> Decision {
        match self {
            Self::PrefixRuleMatch { decision, .. } => *decision,
            Self::HeuristicsRuleMatch { decision } => *decision,
        }
    }

    /// Check if this match came from an explicit policy rule.
    pub fn is_policy_match(&self) -> bool {
        matches!(self, Self::PrefixRuleMatch { .. })
    }
}

/// Result of evaluating multiple commands against a policy.
#[derive(Debug, Clone)]
pub struct PolicyEvaluation {
    /// The overall decision.
    pub decision: Decision,

    /// All rules that matched.
    pub matched_rules: Vec<RuleMatch>,
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
        Self {
            prefix_rules: Vec::new(),
        }
    }

    /// Add a prefix rule to the policy.
    pub fn add_prefix_rule(
        &mut self,
        pattern: &[String],
        decision: Decision,
    ) -> anyhow::Result<()> {
        self.prefix_rules
            .push(PrefixRule::new(pattern.to_vec(), decision));
        Ok(())
    }

    /// Check a single command against the policy.
    pub fn check(&self, command: &[String]) -> RuleMatch {
        for rule in &self.prefix_rules {
            if rule.matches(command) {
                return RuleMatch::PrefixRuleMatch {
                    rule: rule.clone(),
                    decision: rule.decision,
                };
            }
        }

        // No explicit rule matched - use heuristics
        RuleMatch::HeuristicsRuleMatch {
            decision: Decision::Prompt,
        }
    }

    /// Check multiple commands against the policy.
    pub fn check_multiple<'a, I, F>(&self, commands: I, heuristics_fallback: &F) -> PolicyEvaluation
    where
        I: Iterator<Item = &'a Vec<String>>,
        F: Fn(&[String]) -> Decision,
    {
        let mut matched_rules = Vec::new();
        let mut overall_decision = Decision::Allow;

        for command in commands {
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

        PolicyEvaluation {
            decision: overall_decision,
            matched_rules,
        }
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
    fn test_prefix_rule_matching() {
        let rule = PrefixRule::new(
            vec!["cargo".to_string(), "build".to_string()],
            Decision::Allow,
        );

        assert!(rule.matches(&["cargo".to_string(), "build".to_string()]));
        assert!(rule.matches(&[
            "cargo".to_string(),
            "build".to_string(),
            "--release".to_string()
        ]));
        assert!(!rule.matches(&["cargo".to_string(), "test".to_string()]));
        assert!(!rule.matches(&["cargo".to_string()]));
    }

    #[test]
    fn test_policy_check() {
        let mut policy = Policy::empty();
        policy
            .add_prefix_rule(&["cargo".to_string(), "build".to_string()], Decision::Allow)
            .unwrap();
        policy
            .add_prefix_rule(&["rm".to_string()], Decision::Forbidden)
            .unwrap();

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
        policy
            .add_prefix_rule(&["echo".to_string()], Decision::Allow)
            .unwrap();
        policy
            .add_prefix_rule(&["rm".to_string()], Decision::Forbidden)
            .unwrap();

        let commands = vec![
            vec!["echo".to_string(), "hello".to_string()],
            vec!["rm".to_string(), "-rf".to_string()],
        ];

        let evaluation = policy.check_multiple(commands.iter(), &|_| Decision::Prompt);

        // Should be forbidden because one command is forbidden
        assert_eq!(evaluation.decision, Decision::Forbidden);
    }
}
