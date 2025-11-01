use crate::config::CommandsConfig;
use regex::Regex;
use std::env;
use tracing::warn;

#[derive(Clone)]
pub struct CommandPolicyEvaluator {
    allow_prefixes: Vec<String>,
    deny_prefixes: Vec<String>,
    allow_regexes: Vec<Regex>,
    deny_regexes: Vec<Regex>,
    allow_glob_regexes: Vec<Regex>,
    deny_glob_regexes: Vec<Regex>,
    allow_regexes_empty: bool,
    allow_globs_empty: bool,
}

impl CommandPolicyEvaluator {
    pub fn from_config(config: &CommandsConfig) -> Self {
        let allow_prefixes = merge_patterns(&config.allow_list, "VTCODE_COMMANDS_ALLOW_LIST");
        let deny_prefixes = merge_patterns(&config.deny_list, "VTCODE_COMMANDS_DENY_LIST");

        let allow_regex_patterns =
            merge_patterns(&config.allow_regex, "VTCODE_COMMANDS_ALLOW_REGEX");
        let deny_regex_patterns = merge_patterns(&config.deny_regex, "VTCODE_COMMANDS_DENY_REGEX");

        let allow_glob_patterns = merge_patterns(&config.allow_glob, "VTCODE_COMMANDS_ALLOW_GLOB");
        let deny_glob_patterns = merge_patterns(&config.deny_glob, "VTCODE_COMMANDS_DENY_GLOB");

        let allow_regexes = compile_regexes(&allow_regex_patterns);
        let deny_regexes = compile_regexes(&deny_regex_patterns);
        let allow_glob_regexes = compile_globs(&allow_glob_patterns);
        let deny_glob_regexes = compile_globs(&deny_glob_patterns);

        Self {
            allow_prefixes,
            deny_prefixes,
            allow_regexes,
            deny_regexes,
            allow_glob_regexes,
            deny_glob_regexes,
            allow_regexes_empty: allow_regex_patterns.is_empty(),
            allow_globs_empty: allow_glob_patterns.is_empty(),
        }
    }

    pub fn allows(&self, command: &[String]) -> bool {
        if command.is_empty() {
            return false;
        }
        let command_text = command.join(" ");
        self.allows_text(&command_text)
    }

    pub fn allows_text(&self, command_text: &str) -> bool {
        let cmd = command_text.trim();
        if cmd.is_empty() {
            return false;
        }

        // Deny takes precedence
        if self.matches_prefix(cmd, &self.deny_prefixes)
            || Self::matches_any(&self.deny_regexes, cmd)
            || Self::matches_any(&self.deny_glob_regexes, cmd)
        {
            return false;
        }

        // If no allow rules defined, allow by default
        if self.allow_prefixes.is_empty() && self.allow_regexes_empty && self.allow_globs_empty {
            return true;
        }

        // Check allow rules
        self.matches_prefix(cmd, &self.allow_prefixes)
            || Self::matches_any(&self.allow_regexes, cmd)
            || Self::matches_any(&self.allow_glob_regexes, cmd)
    }

    fn matches_prefix(&self, value: &str, prefixes: &[String]) -> bool {
        prefixes
            .iter()
            .filter(|pattern| !pattern.is_empty())
            .any(|pattern| value.starts_with(pattern))
    }

    fn matches_any(regexes: &[Regex], value: &str) -> bool {
        regexes.iter().any(|re| re.is_match(value))
    }
}

fn merge_patterns(base: &[String], env_var: &str) -> Vec<String> {
    let mut combined: Vec<String> = base.iter().map(|entry| entry.trim().to_string()).collect();
    if let Ok(extra) = env::var(env_var) {
        combined.extend(
            extra
                .split(',')
                .map(|item| item.trim().to_string())
                .filter(|item| !item.is_empty()),
        );
    }
    combined
        .into_iter()
        .filter(|item| !item.is_empty())
        .collect()
}

fn compile_regexes(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|pattern| {
            Regex::new(pattern)
                .map_err(|error| {
                    warn!(%error, %pattern, "Ignoring invalid command regex pattern");
                    error
                })
                .ok()
        })
        .collect()
}

fn compile_globs(patterns: &[String]) -> Vec<Regex> {
    patterns
        .iter()
        .filter_map(|pattern| {
            let escaped = regex::escape(pattern);
            let glob_regex = format!("^{}$", escaped.replace(r"\*", ".*").replace(r"\?", "."));
            Regex::new(&glob_regex)
                .map_err(|error| {
                    warn!(%error, pattern = %pattern, "Ignoring invalid command glob pattern");
                    error
                })
                .ok()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::CommandsConfig;

    #[test]
    fn glob_allows_cargo_commands() {
        let mut config = CommandsConfig::default();
        config.allow_list.clear();
        config.allow_glob = vec!["cargo *".to_string()];
        let evaluator = CommandPolicyEvaluator::from_config(&config);
        assert!(evaluator.allows_text("cargo fmt"));
        assert!(evaluator.allows(&["cargo".into(), "check".into()]));
    }

    #[test]
    fn glob_supports_question_mark() {
        let mut config = CommandsConfig::default();
        config.allow_list.clear();
        config.allow_glob = vec!["go test ./pkg/?".to_string()];
        let evaluator = CommandPolicyEvaluator::from_config(&config);
        assert!(evaluator.allows_text("go test ./pkg/a"));
        assert!(!evaluator.allows_text("go test ./pkg/ab"));
    }

    #[test]
    fn glob_allows_node_ecosystem_commands() {
        let mut config = CommandsConfig::default();
        config.allow_list.clear();
        config.allow_glob = vec!["npm *".to_string(), "bun *".to_string()];
        let evaluator = CommandPolicyEvaluator::from_config(&config);
        assert!(evaluator.allows_text("npm install"));
        assert!(evaluator.allows_text("npm run build"));
        assert!(evaluator.allows_text("bun install"));
        assert!(evaluator.allows_text("bun run check"));
    }
}
