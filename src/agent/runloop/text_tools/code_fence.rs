//! Local code-fence block extraction for UI rendering.
//! Fence scanning and name hygiene live in `vtcode_commons::text_fence`.

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CodeFenceBlock {
    pub language: Option<String>,
    pub lines: Vec<String>,
}

pub(crate) use vtcode_commons::text_fence::find_unfenced_from;

pub(crate) fn extract_code_fence_blocks(text: &str) -> Vec<CodeFenceBlock> {
    // Estimate capacity: assume ~1 code block per 30 lines on average, cap at 20
    let estimated_blocks = text.lines().count() / 30 + 1;
    let mut blocks = Vec::with_capacity(estimated_blocks.min(20));
    let mut current_language: Option<String> = None;

    let estimated_lines = text.lines().count() / 5;
    let mut current_lines: Vec<String> = Vec::with_capacity(estimated_lines.min(1000));

    for raw_line in text.lines() {
        let trimmed_start = raw_line.trim_start();
        if let Some(rest) = trimmed_start.strip_prefix("```") {
            let rest_clean = rest.trim_matches('\r');
            let rest_trimmed = rest_clean.trim();
            if current_language.is_some() {
                if rest_trimmed.is_empty() {
                    let language = current_language.take().and_then(|lang| {
                        let cleaned = lang.trim_matches(|ch| matches!(ch, '"' | '\'' | '`'));
                        let cleaned = cleaned.trim();
                        if cleaned.is_empty() {
                            None
                        } else {
                            Some(cleaned.to_string())
                        }
                    });
                    let block_lines = std::mem::take(&mut current_lines);
                    blocks.push(CodeFenceBlock { language, lines: block_lines });
                    continue;
                }
            } else {
                let token = rest_trimmed.split_whitespace().next().unwrap_or_default();
                let normalized = token.trim_matches(|ch| matches!(ch, '"' | '\'' | '`')).trim();
                current_language = Some(normalized.to_ascii_lowercase());
                current_lines.clear();
                continue;
            }
        }

        if current_language.is_some() {
            current_lines.push(raw_line.trim_end_matches('\r').to_string());
        }
    }

    blocks
}
