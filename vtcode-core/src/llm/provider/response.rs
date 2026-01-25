use serde::{Deserialize, Serialize};
use std::pin::Pin;

use super::{LLMError, ToolCall};

/// Universal LLM response
#[derive(Debug, Clone)]
pub struct LLMResponse {
    pub content: Option<String>,
    pub tool_calls: Option<Vec<ToolCall>>,
    pub usage: Option<Usage>,
    pub finish_reason: FinishReason,
    pub reasoning: Option<String>,
    pub reasoning_details: Option<Vec<serde_json::Value>>,
    /// Tool references discovered via Anthropic's tool search feature
    /// These tool names should be expanded (defer_loading=false) in the next request
    pub tool_references: Vec<String>,
    /// Global request ID for tracing
    pub request_id: Option<String>,
    /// Organization ID associated with the request
    pub organization_id: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub cached_prompt_tokens: Option<u32>,
    pub cache_creation_tokens: Option<u32>,
    pub cache_read_tokens: Option<u32>,
}

impl Usage {
    #[inline]
    pub fn cache_hit_rate(&self) -> Option<f64> {
        let read = self.cache_read_tokens? as f64;
        let creation = self.cache_creation_tokens? as f64;
        let total = read + creation;
        if total > 0.0 {
            Some((read / total) * 100.0)
        } else {
            None
        }
    }

    #[inline]
    pub fn is_cache_hit(&self) -> Option<bool> {
        Some(self.cache_read_tokens? > 0)
    }

    #[inline]
    pub fn is_cache_miss(&self) -> Option<bool> {
        Some(self.cache_creation_tokens? > 0 && self.cache_read_tokens? == 0)
    }

    #[inline]
    pub fn total_cache_tokens(&self) -> u32 {
        let read = self.cache_read_tokens.unwrap_or(0);
        let creation = self.cache_creation_tokens.unwrap_or(0);
        read + creation
    }

    #[inline]
    pub fn cache_savings_ratio(&self) -> Option<f64> {
        let read = self.cache_read_tokens? as f64;
        let prompt = self.prompt_tokens as f64;
        if prompt > 0.0 {
            Some(read / prompt)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod usage_tests {
    use super::Usage;

    #[test]
    fn test_cache_hit_rate_calculates_correctly() {
        let usage = Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cache_creation_tokens: Some(200),
            cache_read_tokens: Some(600),
            cached_prompt_tokens: Some(600),
        };
        // 600 / (200 + 600) * 100 = 75%
        assert_eq!(usage.cache_hit_rate(), Some(75.0));
    }

    #[test]
    fn test_cache_hit_rate_none_when_no_cache_tokens() {
        let usage = Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cache_creation_tokens: None,
            cache_read_tokens: None,
            cached_prompt_tokens: None,
        };
        assert_eq!(usage.cache_hit_rate(), None);
    }

    #[test]
    fn test_cache_hit_rate_none_when_zero_total_cache() {
        let usage = Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cache_creation_tokens: Some(0),
            cache_read_tokens: Some(0),
            cached_prompt_tokens: Some(0),
        };
        assert_eq!(usage.cache_hit_rate(), None);
    }

    #[test]
    fn test_is_cache_hit() {
        let hit = Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cache_creation_tokens: Some(100),
            cache_read_tokens: Some(500),
            cached_prompt_tokens: Some(500),
        };
        assert_eq!(hit.is_cache_hit(), Some(true));

        let miss = Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cache_creation_tokens: Some(100),
            cache_read_tokens: Some(0),
            cached_prompt_tokens: Some(0),
        };
        assert_eq!(miss.is_cache_hit(), Some(false));
    }

    #[test]
    fn test_is_cache_miss() {
        let miss = Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cache_creation_tokens: Some(100),
            cache_read_tokens: Some(0),
            cached_prompt_tokens: Some(0),
        };
        assert_eq!(miss.is_cache_miss(), Some(true));

        let hit = Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cache_creation_tokens: Some(100),
            cache_read_tokens: Some(500),
            cached_prompt_tokens: Some(500),
        };
        assert_eq!(hit.is_cache_miss(), Some(false));
    }

    #[test]
    fn test_total_cache_tokens() {
        let usage = Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cache_creation_tokens: Some(200),
            cache_read_tokens: Some(600),
            cached_prompt_tokens: Some(600),
        };
        assert_eq!(usage.total_cache_tokens(), 800);
    }

    #[test]
    fn test_cache_savings_ratio() {
        let usage = Usage {
            prompt_tokens: 1000,
            completion_tokens: 500,
            total_tokens: 1500,
            cache_creation_tokens: Some(200),
            cache_read_tokens: Some(800),
            cached_prompt_tokens: Some(800),
        };
        // 800 / 1000 = 0.8
        assert_eq!(usage.cache_savings_ratio(), Some(0.8));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    Pause,
    Refusal,
    Error(String),
}

#[derive(Debug, Clone)]
pub enum LLMStreamEvent {
    Token { delta: String },
    Reasoning { delta: String },
    Completed { response: LLMResponse },
}

pub type LLMStream = Pin<Box<dyn futures::Stream<Item = Result<LLMStreamEvent, LLMError>> + Send>>;
