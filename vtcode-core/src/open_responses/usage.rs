//! Token usage statistics for Open Responses.
//!
//! Provides a unified usage model that can bridge from VT Code's internal
//! usage tracking to the Open Responses specification.

use serde::{Deserialize, Deserializer, Serialize};

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
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boxed_input_tokens_details_opt"
    )]
    pub input_tokens_details: Option<Box<InputTokensDetails>>,

    /// Detailed breakdown of output token usage.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "deserialize_boxed_output_tokens_details_opt"
    )]
    pub output_tokens_details: Option<Box<OutputTokensDetails>>,
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

impl InputTokensDetails {
    fn is_empty(&self) -> bool {
        self.cached_tokens.is_none() && self.audio_tokens.is_none() && self.text_tokens.is_none()
    }

    fn into_boxed_if_non_empty(self) -> Option<Box<Self>> {
        (!self.is_empty()).then_some(Box::new(self))
    }
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

impl OutputTokensDetails {
    fn is_empty(&self) -> bool {
        self.reasoning_tokens.is_none() && self.audio_tokens.is_none() && self.text_tokens.is_none()
    }

    fn into_boxed_if_non_empty(self) -> Option<Box<Self>> {
        (!self.is_empty()).then_some(Box::new(self))
    }
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
        let cached = usage.cache_read_tokens_or_fallback();
        if cached > 0 {
            details.cached_tokens = Some(cached as u64);
        }

        Self {
            input_tokens: usage.prompt_tokens as u64,
            output_tokens: usage.completion_tokens as u64,
            total_tokens: usage.total_tokens as u64,
            input_tokens_details: if details.cached_tokens.is_some() {
                Some(Box::new(details))
            } else {
                None
            },
            output_tokens_details: None,
        }
    }

    /// Creates usage from VT Code's exec events usage type.
    pub fn from_exec_usage(usage: &vtcode_exec_events::Usage) -> Self {
        let input_details = if usage.cached_input_tokens > 0 {
            Some(Box::new(InputTokensDetails {
                cached_tokens: Some(usage.cached_input_tokens),
                audio_tokens: None,
                text_tokens: None,
            }))
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

fn deserialize_boxed_input_tokens_details_opt<'de, D>(
    deserializer: D,
) -> Result<Option<Box<InputTokensDetails>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<InputTokensDetails>::deserialize(deserializer)
        .map(|value| value.and_then(InputTokensDetails::into_boxed_if_non_empty))
}

fn deserialize_boxed_output_tokens_details_opt<'de, D>(
    deserializer: D,
) -> Result<Option<Box<OutputTokensDetails>>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<OutputTokensDetails>::deserialize(deserializer)
        .map(|value| value.and_then(OutputTokensDetails::into_boxed_if_non_empty))
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
            cache_creation_tokens: 0,
            output_tokens: 200,
        };
        let usage = OpenUsage::from_exec_usage(&exec_usage);
        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 200);
        assert_eq!(usage.total_tokens, 1200);
        assert_eq!(usage.input_tokens_details.unwrap().cached_tokens, Some(500));
    }

    #[test]
    fn test_from_llm_usage_falls_back_to_cached_prompt_tokens() {
        let usage = OpenUsage::from_llm_usage(&crate::llm::provider::Usage {
            prompt_tokens: 1000,
            completion_tokens: 250,
            total_tokens: 1250,
            cached_prompt_tokens: Some(400),
            cache_creation_tokens: None,
            cache_read_tokens: None,
        });

        assert_eq!(usage.input_tokens, 1000);
        assert_eq!(usage.output_tokens, 250);
        assert_eq!(
            usage
                .input_tokens_details
                .and_then(|details| details.cached_tokens),
            Some(400)
        );
    }

    #[test]
    fn empty_details_deserialize_to_none() {
        let usage: OpenUsage = serde_json::from_str(
            r#"{
                "input_tokens": 1,
                "output_tokens": 2,
                "total_tokens": 3,
                "input_tokens_details": {},
                "output_tokens_details": {}
            }"#,
        )
        .unwrap();

        assert!(usage.input_tokens_details.is_none());
        assert!(usage.output_tokens_details.is_none());
    }

    #[test]
    fn boxed_details_are_smaller_than_inline_options() {
        use std::mem::size_of;

        assert!(
            size_of::<Option<Box<InputTokensDetails>>>() < size_of::<Option<InputTokensDetails>>()
        );
        assert!(
            size_of::<Option<Box<OutputTokensDetails>>>()
                < size_of::<Option<OutputTokensDetails>>()
        );
    }
}
