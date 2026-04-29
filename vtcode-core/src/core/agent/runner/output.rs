use super::AgentRunner;
use crate::core::agent::types::AgentType;
use crate::utils::colors::style;
use std::io::IsTerminal;

impl AgentRunner {
    pub(super) fn should_print_final_message_to_stdout(
        stdout_is_terminal: bool,
        stderr_is_terminal: bool,
    ) -> bool {
        !(stdout_is_terminal && stderr_is_terminal)
    }

    #[allow(clippy::print_stdout)]
    fn print_final_message_to_stdout(text: &str) {
        if text.trim().is_empty() {
            return;
        }
        if text.ends_with('\n') {
            print!("{text}");
        } else {
            println!("{text}");
        }
    }

    fn print_compact_response(agent: &AgentType, text: &str) {
        const MAX_CHARS: usize = 1200;
        const HEAD_CHARS: usize = 800;
        const TAIL_CHARS: usize = 200;
        let clean = text.trim();
        if clean.chars().count() <= MAX_CHARS {
            println!(
                "{} [{}]: {}",
                style("[RESPONSE]").cyan().bold(),
                agent,
                clean
            );
            return;
        }
        let mut out = String::new();
        for (count, ch) in clean.chars().enumerate() {
            if count >= HEAD_CHARS {
                break;
            }
            out.push(ch);
        }
        out.push_str("\n…\n");
        let total = clean.chars().count();
        let start_tail = total.saturating_sub(TAIL_CHARS);
        let tail: String = clean.chars().skip(start_tail).collect();
        out.push_str(&tail);
        println!("{} [{}]: {}", style("[RESPONSE]").cyan().bold(), agent, out);
        println!(
            "{} truncated long response ({} chars).",
            style("[NOTE]").dim(),
            total
        );
    }

    pub(super) fn emit_final_assistant_message(&self, agent: &AgentType, text: &str) {
        if self.quiet {
            return;
        }

        let trimmed = text.trim();
        if trimmed.is_empty() {
            return;
        }

        if Self::should_print_final_message_to_stdout(
            std::io::stdout().is_terminal(),
            std::io::stderr().is_terminal(),
        ) {
            Self::print_final_message_to_stdout(text);
            return;
        }

        Self::print_compact_response(agent, trimmed);
    }
}

#[cfg(test)]
mod tests {
    use super::AgentRunner;

    #[test]
    fn suppresses_stdout_message_when_both_streams_are_terminals() {
        assert!(!AgentRunner::should_print_final_message_to_stdout(
            true, true
        ));
    }

    #[test]
    fn prints_stdout_message_when_stdout_is_not_terminal() {
        assert!(AgentRunner::should_print_final_message_to_stdout(
            false, true
        ));
    }

    #[test]
    fn prints_stdout_message_when_stderr_is_not_terminal() {
        assert!(AgentRunner::should_print_final_message_to_stdout(
            true, false
        ));
    }
}
