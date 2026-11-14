/// Default maximum number of turns in full-auto execution.
pub const DEFAULT_FULL_AUTO_MAX_TURNS: usize = 100;
/// Default conversation-turn retention limit. This is a context-retention
/// limit, not a tool-execution budget.
pub const DEFAULT_MAX_CONVERSATION_TURNS: usize = 150;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_automation_and_retention_budgets_are_moderate_and_bounded() {
        assert_eq!(DEFAULT_FULL_AUTO_MAX_TURNS, 100);
        assert_eq!(DEFAULT_MAX_CONVERSATION_TURNS, 150);
    }
}
