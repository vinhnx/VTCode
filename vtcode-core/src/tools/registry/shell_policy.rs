use anyhow::{Result, anyhow};
use regex::Regex;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use tracing::warn;

#[derive(Clone, Debug)]
pub struct ShellPolicyCacheEntry {
    pub signature: u64,
    pub deny_regexes: Vec<(String, Regex)>,
    pub deny_globs: Vec<(String, Regex)>,
}

pub struct ShellPolicyChecker {
    cache: Option<ShellPolicyCacheEntry>,
    commands_config: Option<crate::config::CommandsConfig>,
}

impl ShellPolicyChecker {
    pub fn new() -> Self {
        Self {
            cache: None,
            commands_config: None,
        }
    }
}

impl Default for ShellPolicyChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellPolicyChecker {
    pub fn set_commands_config(&mut self, commands_config: &crate::config::CommandsConfig) {
        self.commands_config = Some(commands_config.clone());
        self.reset_cache();
    }

    pub fn commands_config(&self) -> Option<&crate::config::CommandsConfig> {
        self.commands_config.as_ref()
    }

    pub fn check_command(
        &mut self,
        command: &str,
        agent_type: &str,
        deny_regex_patterns: &[String],
        deny_glob_patterns: &[String],
    ) -> Result<()> {
        let mut hasher = DefaultHasher::new();
        deny_regex_patterns.hash(&mut hasher);
        deny_glob_patterns.hash(&mut hasher);
        let signature = hasher.finish();

        let entry = if let Some(ref entry) = self.cache
            && entry.signature == signature
        {
            entry
        } else {
            let compiled_regexes = deny_regex_patterns
                .iter()
                .filter_map(|pattern| {
                    if pattern.is_empty() { return None; }
                    match Regex::new(pattern) {
                        Ok(re) => Some((pattern.clone(), re)),
                        Err(err) => {
                            warn!(agent = agent_type, pattern, error = %err, "Invalid deny regex pattern skipped");
                            None
                        }
                    }
                })
                .collect::<Vec<_>>();

            let compiled_globs = deny_glob_patterns
                .iter()
                .filter_map(|pattern| {
                    if pattern.is_empty() { return None; }
                    let re_pattern = format!("^{}$", regex::escape(pattern).replace(r"\\*", ".*"));
                    match Regex::new(&re_pattern) {
                        Ok(re) => Some((pattern.clone(), re)),
                        Err(err) => {
                            warn!(agent = agent_type, pattern, error = %err, "Invalid deny glob pattern skipped");
                            None
                        }
                    }
                })
                .collect::<Vec<_>>();

            let new_entry = ShellPolicyCacheEntry {
                signature,
                deny_regexes: compiled_regexes,
                deny_globs: compiled_globs,
            };
            self.cache = Some(new_entry);
            self.cache
                .as_ref()
                .ok_or_else(|| anyhow!("Failed to initialize shell policy cache entry"))?
        };

        for (pattern, compiled) in &entry.deny_regexes {
            if compiled.is_match(command) {
                return Err(anyhow!(
                    "Shell command denied by agent regex policy: {}",
                    pattern
                ));
            }
        }

        for (pattern, compiled) in &entry.deny_globs {
            if compiled.is_match(command) {
                return Err(anyhow!(
                    "Shell command denied by agent glob policy: {}",
                    pattern
                ));
            }
        }

        Ok(())
    }

    pub fn reset_cache(&mut self) {
        self.cache = None;
    }
}
