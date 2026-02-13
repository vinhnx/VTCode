const OPEN_TAG: &str = "<proposed_plan>";
const CLOSE_TAG: &str = "</proposed_plan>";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProposedPlanExtraction {
    pub stripped_text: String,
    pub plan_text: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseMode {
    Normal,
    InPlan,
}

/// Streaming parser that removes `<proposed_plan>...</proposed_plan>` content from
/// assistant-visible text while collecting the plan body.
#[derive(Debug, Default)]
pub(crate) struct ProposedPlanStreamParser {
    mode: Option<ParseMode>,
    pending: String,
    plan_buffer: String,
    saw_plan_block: bool,
}

impl ProposedPlanStreamParser {
    pub(crate) fn new() -> Self {
        Self {
            mode: Some(ParseMode::Normal),
            pending: String::new(),
            plan_buffer: String::new(),
            saw_plan_block: false,
        }
    }

    /// Consume streamed text and return only content that should remain visible
    /// to the assistant transcript.
    pub(crate) fn consume(&mut self, chunk: &str) -> String {
        self.pending.push_str(chunk);
        let mut visible = String::new();

        loop {
            match self.mode.unwrap_or(ParseMode::Normal) {
                ParseMode::Normal => {
                    if let Some(index) = self.pending.find(OPEN_TAG) {
                        visible.push_str(&self.pending[..index]);
                        self.pending.drain(..index + OPEN_TAG.len());
                        self.mode = Some(ParseMode::InPlan);
                        self.saw_plan_block = true;
                        continue;
                    }

                    let keep_tail = OPEN_TAG.len().saturating_sub(1).min(self.pending.len());
                    let emit_len = self.pending.len().saturating_sub(keep_tail);
                    visible.push_str(&self.pending[..emit_len]);
                    self.pending.drain(..emit_len);
                    break;
                }
                ParseMode::InPlan => {
                    if let Some(index) = self.pending.find(CLOSE_TAG) {
                        self.plan_buffer.push_str(&self.pending[..index]);
                        self.pending.drain(..index + CLOSE_TAG.len());
                        self.mode = Some(ParseMode::Normal);
                        continue;
                    }

                    let keep_tail = CLOSE_TAG.len().saturating_sub(1).min(self.pending.len());
                    let append_len = self.pending.len().saturating_sub(keep_tail);
                    self.plan_buffer.push_str(&self.pending[..append_len]);
                    self.pending.drain(..append_len);
                    break;
                }
            }
        }

        visible
    }

    /// Finish parsing and return any remaining visible text plus optional plan.
    pub(crate) fn finish(&mut self) -> ProposedPlanExtraction {
        let mut trailing_visible = String::new();
        match self.mode.unwrap_or(ParseMode::Normal) {
            ParseMode::Normal => {
                trailing_visible.push_str(&self.pending);
            }
            ParseMode::InPlan => {
                // Unterminated block: treat the remainder as plan content.
                self.plan_buffer.push_str(&self.pending);
            }
        }
        self.pending.clear();
        self.mode = Some(ParseMode::Normal);

        ProposedPlanExtraction {
            stripped_text: trailing_visible,
            plan_text: finalize_plan_text(self.saw_plan_block, &self.plan_buffer),
        }
    }
}

pub(crate) fn extract_proposed_plan(text: &str) -> ProposedPlanExtraction {
    let mut parser = ProposedPlanStreamParser::new();
    let mut stripped = parser.consume(text);
    let trailing = parser.finish();
    stripped.push_str(&trailing.stripped_text);

    ProposedPlanExtraction {
        stripped_text: stripped,
        plan_text: trailing.plan_text,
    }
}

fn finalize_plan_text(saw_plan_block: bool, raw: &str) -> Option<String> {
    if !saw_plan_block {
        return None;
    }
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::{ProposedPlanStreamParser, extract_proposed_plan};

    #[test]
    fn extracts_single_proposed_plan_block() {
        let extraction =
            extract_proposed_plan("Intro\n<proposed_plan>\n- A\n- B\n</proposed_plan>\nOutro");
        assert_eq!(extraction.stripped_text, "Intro\n\nOutro");
        assert_eq!(extraction.plan_text.as_deref(), Some("- A\n- B"));
    }

    #[test]
    fn keeps_text_when_no_plan_block_exists() {
        let extraction = extract_proposed_plan("No plan here");
        assert_eq!(extraction.stripped_text, "No plan here");
        assert!(extraction.plan_text.is_none());
    }

    #[test]
    fn handles_unterminated_plan_block() {
        let extraction = extract_proposed_plan("Before<proposed_plan>\n- Step 1\n- Step 2");
        assert_eq!(extraction.stripped_text, "Before");
        assert_eq!(extraction.plan_text.as_deref(), Some("- Step 1\n- Step 2"));
    }

    #[test]
    fn supports_streaming_chunks_with_split_tags() {
        let mut parser = ProposedPlanStreamParser::new();
        let mut visible = String::new();
        visible.push_str(&parser.consume("Intro\n<propo"));
        visible.push_str(&parser.consume("sed_plan>\n- Step"));
        visible.push_str(&parser.consume(" 1\n</proposed_plan>\nOutro"));
        let trailing = parser.finish();
        visible.push_str(&trailing.stripped_text);

        assert_eq!(visible, "Intro\n\nOutro");
        assert_eq!(trailing.plan_text.as_deref(), Some("- Step 1"));
    }
}
