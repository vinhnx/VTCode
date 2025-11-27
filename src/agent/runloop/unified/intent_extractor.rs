/// Extract user intent/action suggestion from a user prompt
/// Used to display dynamic spinner messages instead of hardcoded "Thinking..."
use std::collections::HashMap;

/// Action verb mappings with word boundary awareness
struct ActionMatcher {
    verbs: HashMap<&'static str, &'static str>,
}

impl ActionMatcher {
    fn new() -> Self {
        let mut verbs = HashMap::new();

        // Core actions - prioritized by likehood
        verbs.insert("find", "Finding");
        verbs.insert("search", "Searching");
        verbs.insert("grep", "Searching");
        verbs.insert("look for", "Finding");
        verbs.insert("locate", "Finding");

        verbs.insert("create", "Creating");
        verbs.insert("make", "Creating");
        verbs.insert("write", "Writing");
        verbs.insert("add", "Adding");
        verbs.insert("generate", "Generating");

        verbs.insert("delete", "Deleting");
        verbs.insert("remove", "Removing");
        verbs.insert("rm", "Removing");

        verbs.insert("edit", "Editing");
        verbs.insert("modify", "Modifying");
        verbs.insert("change", "Changing");
        verbs.insert("update", "Updating");

        verbs.insert("read", "Reading");
        verbs.insert("cat", "Reading");
        verbs.insert("open", "Opening");
        verbs.insert("show", "Showing");
        verbs.insert("display", "Displaying");
        verbs.insert("list", "Listing");
        verbs.insert("ls", "Listing");

        verbs.insert("run", "Running");
        verbs.insert("execute", "Executing");
        verbs.insert("start", "Starting");

        verbs.insert("test", "Testing");
        verbs.insert("check", "Checking");
        verbs.insert("validate", "Validating");

        verbs.insert("build", "Building");
        verbs.insert("compile", "Compiling");
        verbs.insert("make build", "Building");

        verbs.insert("format", "Formatting");
        verbs.insert("lint", "Linting");
        verbs.insert("fix", "Fixing");
        verbs.insert("refactor", "Refactoring");

        verbs.insert("explain", "Explaining");
        verbs.insert("understand", "Understanding");
        verbs.insert("analyze", "Analyzing");
        verbs.insert("review", "Reviewing");

        verbs.insert("compare", "Comparing");
        verbs.insert("diff", "Comparing");

        verbs.insert("merge", "Merging");
        verbs.insert("rebase", "Rebasing");
        verbs.insert("commit", "Committing");
        verbs.insert("push", "Pushing");
        verbs.insert("pull", "Pulling");
        verbs.insert("clone", "Cloning");

        verbs.insert("deploy", "Deploying");
        verbs.insert("install", "Installing");
        verbs.insert("upgrade", "Upgrading");

        verbs.insert("debug", "Debugging");
        verbs.insert("trace", "Tracing");
        verbs.insert("profile", "Profiling");

        verbs.insert("help", "Helping");
        verbs.insert("optimize", "Optimizing");
        verbs.insert("summarize", "Summarizing");

        Self { verbs }
    }

    /// Extract action from text using word boundary matching
    fn extract(&self, text: &str) -> Option<&'static str> {
        let lower = text.to_lowercase();

        let mut best_match: Option<(&'static str, usize)> = None;
        let mut _idx = 0usize;
        let mut iter = lower.split_whitespace().peekable();
        while let Some(word) = iter.next() {
            // Try exact word matches
            if let Some(&action) = self.verbs.get(word) {
                let match_len = word.len();
                if best_match.is_none() || match_len > best_match.unwrap().1 {
                    best_match = Some((action, match_len));
                }
            }

            // Try two-word phrases using peek (avoids allocating a Vec of words)
            if let Some(&next) = iter.peek() {
                let phrase = format!("{} {}", word, next);
                if let Some(&action) = self.verbs.get(phrase.as_str()) {
                    let match_len = phrase.len();
                    if best_match.is_none() || match_len > best_match.unwrap().1 {
                        best_match = Some((action, match_len));
                    }
                }
            }

            _idx += 1;
        }

        best_match.map(|(action, _)| action)
    }
}

/// Extract action suggestion from user prompt with intelligent matching
pub fn extract_action_suggestion(prompt: &str) -> String {
    if prompt.trim().is_empty() {
        return "Processing".to_string();
    }

    let lower = prompt.to_lowercase();

    // Check question patterns first (higher priority than verb matching)
    if lower.starts_with("how ") || lower.starts_with("what ") || lower.starts_with("why ") {
        return "Analyzing".to_string();
    }

    if lower.starts_with("can you ") || lower.starts_with("could you ") {
        return "Thinking".to_string();
    }

    let matcher = ActionMatcher::new();

    // Try to extract action verb from the prompt
    if let Some(action) = matcher.extract(prompt) {
        return action.to_string();
    }

    // Fallback to intelligent guessing based on prompt characteristics
    if lower.contains("?") {
        return "Answering".to_string();
    }

    // Default fallback
    "Processing".to_string()
}

/// Extract action suggestion from a message
/// Looks at the last user message in the history
pub fn extract_action_from_messages(messages: &[vtcode_core::llm::provider::Message]) -> String {
    // Find the last user message
    let last_user_msg = messages
        .iter()
        .rev()
        .find(|msg| msg.role == vtcode_core::llm::provider::MessageRole::User);

    if let Some(msg) = last_user_msg {
        let text = msg.content.as_text();
        extract_action_suggestion(&text)
    } else {
        "Processing".to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exact_word_matching() {
        assert_eq!(extract_action_suggestion("find all files"), "Finding");
        assert_eq!(
            extract_action_suggestion("search the codebase"),
            "Searching"
        );
        assert_eq!(
            extract_action_suggestion("create a new function"),
            "Creating"
        );
    }

    #[test]
    fn test_word_boundaries() {
        // Should not match "use" inside "because"
        assert_eq!(
            extract_action_suggestion("because this is important"),
            "Processing"
        );

        // Should match standalone "use"
        // Note: "use" is not in our list, so it should return default
        assert_eq!(extract_action_suggestion("use the library"), "Processing");
    }

    #[test]
    fn test_phrase_matching() {
        assert_eq!(extract_action_suggestion("look for the bug"), "Finding");
        assert_eq!(
            extract_action_suggestion("make build for production"),
            "Building"
        );
    }

    #[test]
    fn test_case_insensitive() {
        assert_eq!(extract_action_suggestion("FIND THE BUG"), "Finding");
        assert_eq!(extract_action_suggestion("Create a New File"), "Creating");
    }

    #[test]
    fn test_question_patterns() {
        // Questions starting with how/what/why should analyze, even if they contain other verbs
        assert_eq!(
            extract_action_suggestion("how can I fix this?"),
            "Analyzing"
        );
        assert_eq!(
            extract_action_suggestion("what does this code do?"),
            "Analyzing"
        );
        assert_eq!(
            extract_action_suggestion("why is this failing?"),
            "Analyzing"
        );
        assert_eq!(extract_action_suggestion("can you help me?"), "Thinking");
    }

    #[test]
    fn test_question_mark_fallback() {
        assert_eq!(
            extract_action_suggestion("implement this feature?"),
            "Answering"
        );
    }

    #[test]
    fn test_empty_prompt() {
        assert_eq!(extract_action_suggestion(""), "Processing");
        assert_eq!(extract_action_suggestion("   "), "Processing");
    }

    #[test]
    fn test_longest_match_wins() {
        // "delete" should match over shorter alternatives
        assert_eq!(extract_action_suggestion("delete the file"), "Deleting");
    }

    #[test]
    fn test_common_commands() {
        assert_eq!(extract_action_suggestion("build the project"), "Building");
        // "test" as standalone word will match
        assert_eq!(extract_action_suggestion("test the code"), "Testing");
        assert_eq!(extract_action_suggestion("list all files"), "Listing");
        assert_eq!(extract_action_suggestion("read the config"), "Reading");
    }

    #[test]
    fn test_git_operations() {
        assert_eq!(extract_action_suggestion("commit my changes"), "Committing");
        assert_eq!(extract_action_suggestion("push to main"), "Pushing");
        assert_eq!(extract_action_suggestion("pull latest changes"), "Pulling");
        assert_eq!(extract_action_suggestion("merge feature branch"), "Merging");
    }

    #[test]
    fn test_development_operations() {
        assert_eq!(extract_action_suggestion("debug the issue"), "Debugging");
        assert_eq!(
            extract_action_suggestion("analyze the performance"),
            "Analyzing"
        );
        assert_eq!(
            extract_action_suggestion("refactor this code"),
            "Refactoring"
        );
        assert_eq!(
            extract_action_suggestion("optimize the query"),
            "Optimizing"
        );
    }
}
