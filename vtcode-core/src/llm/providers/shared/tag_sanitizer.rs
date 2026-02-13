//! Stateful sanitizer for extracting reasoning tags from streaming content.
//!
//! This module provides a way to extract reasoning from content blocks
//! that use tags like `<think>`, `<thought>`, or `<reasoning>` during streaming.
//! Especially useful for Gemini 3 and GLM-5 models.

use crate::llm::provider::LLMStreamEvent;

/// Tags that indicate the start of a reasoning block.
const REASONING_TAGS: &[(&str, &str)] = &[
    ("<think>", "</think>"),
    ("<thought>", "</thought>"),
    ("<reasoning>", "</reasoning>"),
    ("<analysis>", "</analysis>"),
];

/// A stateful sanitizer that extracts reasoning from a stream of content chunks.
pub struct TagStreamSanitizer {
    in_reasoning: bool,
    active_tag_pair: Option<(&'static str, &'static str)>,
    // Store the partial opening tag if we see a '<' but haven't seen the whole tag yet.
    partial_tag: String,
}

impl Default for TagStreamSanitizer {
    fn default() -> Self {
        Self::new()
    }
}

impl TagStreamSanitizer {
    pub fn new() -> Self {
        Self {
            in_reasoning: false,
            active_tag_pair: None,
            partial_tag: String::new(),
        }
    }

    /// Processes a content chunk and returns a list of resulting stream events.
    /// This may return `LLMStreamEvent::Token` (for the actual content)
    /// and `LLMStreamEvent::Reasoning` (for extracted reasoning).
    pub fn process_chunk(&mut self, chunk: &str) -> Vec<LLMStreamEvent> {
        let mut events = Vec::new();
        let mut current_pos = 0;
        let chunk_str = format!("{}{}", self.partial_tag, chunk);
        self.partial_tag.clear();

        while current_pos < chunk_str.len() {
            if !self.in_reasoning {
                // Look for the start of any reasoning tag.
                let mut found_start: Option<(usize, &(&str, &str))> = None;
                for pair in REASONING_TAGS {
                    if let Some(pos) = chunk_str[current_pos..].find(pair.0) {
                        let absolute_pos = current_pos + pos;
                        if found_start.is_none() || absolute_pos < found_start.unwrap().0 {
                            found_start = Some((absolute_pos, pair));
                        }
                    }
                }

                // If no tag found, check if we have a partial tag at the end.
                if found_start.is_none() {
                    if let Some(bracket_pos) = chunk_str[current_pos..].rfind('<') {
                        let absolute_bracket = current_pos + bracket_pos;
                        let potential_partial = &chunk_str[absolute_bracket..];
                        // If it's a prefix of any possible start tag, save it.
                        let is_prefix = REASONING_TAGS
                            .iter()
                            .any(|(start, _)| start.starts_with(potential_partial));

                        if is_prefix {
                            // Yield everything before the partial tag as a regular token.
                            if absolute_bracket > current_pos {
                                events.push(LLMStreamEvent::Token {
                                    delta: chunk_str[current_pos..absolute_bracket].to_string(),
                                });
                            }
                            self.partial_tag = potential_partial.to_string();
                            return events;
                        }
                    }

                    // No tag or partial tag found, just yield the rest as a token.
                    events.push(LLMStreamEvent::Token {
                        delta: chunk_str[current_pos..].to_string(),
                    });
                    return events;
                }

                // Found a start tag!
                let (start_pos, pair) = found_start.unwrap();

                // Yield text before the tag.
                if start_pos > current_pos {
                    events.push(LLMStreamEvent::Token {
                        delta: chunk_str[current_pos..start_pos].to_string(),
                    });
                }

                self.in_reasoning = true;
                self.active_tag_pair = Some(*pair);
                current_pos = start_pos + pair.0.len();
            } else {
                // We are in a reasoning block. Look for the closing tag.
                let (_, close_tag) = self.active_tag_pair.unwrap();
                if let Some(pos) = chunk_str[current_pos..].find(close_tag) {
                    let absolute_pos = current_pos + pos;

                    // Yield text inside the tag as reasoning.
                    if absolute_pos > current_pos {
                        events.push(LLMStreamEvent::Reasoning {
                            delta: chunk_str[current_pos..absolute_pos].to_string(),
                        });
                    }

                    self.in_reasoning = false;
                    self.active_tag_pair = None;
                    current_pos = absolute_pos + close_tag.len();
                } else {
                    // No closing tag found in this chunk.
                    // Check for partial closing tag at the end.
                    if let Some(bracket_pos) = chunk_str[current_pos..].rfind('<') {
                        let absolute_bracket = current_pos + bracket_pos;
                        let potential_partial = &chunk_str[absolute_bracket..];
                        if close_tag.starts_with(potential_partial) {
                            // Yield everything before the partial closing tag as reasoning.
                            if absolute_bracket > current_pos {
                                events.push(LLMStreamEvent::Reasoning {
                                    delta: chunk_str[current_pos..absolute_bracket].to_string(),
                                });
                            }
                            self.partial_tag = potential_partial.to_string();
                            return events;
                        }
                    }

                    // Yield all as reasoning.
                    events.push(LLMStreamEvent::Reasoning {
                        delta: chunk_str[current_pos..].to_string(),
                    });
                    return events;
                }
            }
        }

        events
    }

    /// Finalizes the sanitizer, returning any leftover content.
    pub fn finalize(self) -> Vec<LLMStreamEvent> {
        let mut events = Vec::new();
        if !self.partial_tag.is_empty() {
            if self.in_reasoning {
                events.push(LLMStreamEvent::Reasoning {
                    delta: self.partial_tag,
                });
            } else {
                events.push(LLMStreamEvent::Token {
                    delta: self.partial_tag,
                });
            }
        }
        events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitizer_basic() {
        let mut sanitizer = TagStreamSanitizer::new();
        let events = sanitizer.process_chunk("Hello <think>reasoning</think> world");
        assert_eq!(events.len(), 3);
        match &events[0] {
            LLMStreamEvent::Token { delta } => assert_eq!(delta, "Hello "),
            _ => panic!("Expected Token"),
        }
        match &events[1] {
            LLMStreamEvent::Reasoning { delta } => assert_eq!(delta, "reasoning"),
            _ => panic!("Expected Reasoning"),
        }
        match &events[2] {
            LLMStreamEvent::Token { delta } => assert_eq!(delta, " world"),
            _ => panic!("Expected Token"),
        }
    }

    #[test]
    fn test_sanitizer_split_chunk() {
        let mut sanitizer = TagStreamSanitizer::new();
        let e1 = sanitizer.process_chunk("Hello <thi");
        assert_eq!(e1.len(), 1);
        match &e1[0] {
            LLMStreamEvent::Token { delta } => assert_eq!(delta, "Hello "),
            _ => panic!("Expected Token"),
        }

        let e2 = sanitizer.process_chunk("nk>reason");
        assert_eq!(e2.len(), 1);
        match &e2[0] {
            LLMStreamEvent::Reasoning { delta } => assert_eq!(delta, "reason"),
            _ => panic!("Expected Reasoning"),
        }

        let e3 = sanitizer.process_chunk("ing</thi");
        assert_eq!(e3.len(), 1);
        match &e3[0] {
            LLMStreamEvent::Reasoning { delta } => assert_eq!(delta, "ing"),
            _ => panic!("Expected Reasoning"),
        }

        let e4 = sanitizer.process_chunk("nk> after");
        assert_eq!(e4.len(), 1);
        match &e4[0] {
            LLMStreamEvent::Token { delta } => assert_eq!(delta, " after"),
            _ => panic!("Expected Token"),
        }
    }
}
