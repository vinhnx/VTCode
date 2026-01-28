//! Token usage statistics for Open Responses.
//!
//! Provides a unified usage model that can bridge from VT Code's internal
//! usage tracking to the Open Responses specification.

use serde::{Deserialize, Serialize};

/// Token usage statistics for a response.
///
/// This struct follows the Open Responses specification for usage reporting
/// and can be converted from VT Code's internal usage types.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenUsage {
    /// Number of input tokens processed.
    pub input_tokens: u64,

    /// Number of output tokens generated.
    pub output_tokens: u64,

    /// Total number of tokens used (input + output).
    pub total_tokens: u64,

    /// Detailed breakdown of input token usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_tokens_details: Option<InputTokensDetails>,

    /// Detailed breakdown of output token usage.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_tokens_details: Option<OutputTokensDetails>,
}

/// Detailed breakdown of input token usage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputTokensDetails {
    /// Number of cached tokens reused from previous requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_tokens: Option<u64>,

    /// Number of tokens used for audio input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_tokens: Option<u64>,

    /// Number of tokens used for text input.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_tokens: Option<u64>,
}

/// Detailed breakdown of output token usage.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutputTokensDetails {
    /// Number of tokens used for reasoning/thinking.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_tokens: Option<u64>,

    /// Number of tokens used for audio output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_tokens: Option<u64>,

    /// Number of tokens used for text output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_tokens: Option<u64>,
}

impl OpenUsage {
    /// Creates a new usage instance with the given token counts.
    pub fn new(input_tokens: u64, output_tokens: u64) -> Self {
        Self {
            input_tokens,
            output_tokens,
            total_tokens: input_tokens + output_tokens,
            input_tokens_details: None,
            output_tokens_details: None,
        }
    }

    /// Creates usage from VT Code's internal LLM usage type.
    pub fn from_llm_usage(usage: &crate::llm::provider::Usage) -> Self {
        let mut details = InputTokensDetails::default();
        if let Some(cached) = usage.cache_read_tokens {
            details.cached_tokens = Some(cached as u64);
        }

        Self {
            input_tokens: usage.prompt_tokens as u64,
            output_tokens: usage.completion_tokens as u64,
            total_tokens: usage.total_tokens as u64,
            input_tokens_details: if details.cached_tokens.is_some() {
                Some(details)
            } else {
                None
            },
            output_tokens_details: None,
        }
    }

    /// Creates usage from VT Code's exec events usage type.
    pub fn from_exec_usage(usage: &vtcode_exec_events::Usage) -> Self {
        let input_details = if usage.cached_input_tokens > 0 {
            Some(InputTokensDetails {
                cached_tokens: Some(usage.cached_input_tokens),
                audio_tokens: None,
                text_tokens: None,
            })
        } else {
            None
        };

        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.input_tokens + usage.output_tokens,
            input_tokens_details: input_details,
            output_tokens_details: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_new() {
        let usage = OpenUsage::new(100, 50);
        assert_eq!(usage.input_tokens, 100);
        assert_eq!(usage.output_tokens, 50);
        assert_eq!(usage.total_tokens, 150);
    }

    #[test]
    fn test_from_exec_usage() {
        let exec_usage = vtcode_exec_events::Usage {
            input_tokens: 1000,
            cached_input_tokens: 500,
            output_tokens: 200,
        };
        let usage = OpenUsage::from_exec_usage(&exec_usage);
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 200);
        assert_eq!(usage.total_tokens, 1200);
        assert_eq!(
            usage.input_tokens_details.unwrap().cached_tokens,
            Some(500)
        );
    }
}
