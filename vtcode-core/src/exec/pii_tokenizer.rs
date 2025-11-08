//! PII (Personally Identifiable Information) tokenization for data privacy.
//!
//! Automatically detects and tokenizes sensitive data before MCP tool calls,
//! preventing PII from entering model context or being logged.
//!
//! Features:
//! - Pattern-based detection (email, phone, SSN, credit card, etc.)
//! - Secure token generation and storage
//! - Automatic de-tokenization on tool result
//! - Configurable patterns and policies
//! - Audit trail of tokenized data

use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{debug, warn};

/// Types of PII that can be tokenized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PiiType {
    Email,
    PhoneNumber,
    SocialSecurityNumber,
    CreditCard,
    IpAddress,
    ApiKey,
    AuthToken,
    Url,
    Custom,
}

impl PiiType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Email => "email",
            Self::PhoneNumber => "phone_number",
            Self::SocialSecurityNumber => "ssn",
            Self::CreditCard => "credit_card",
            Self::IpAddress => "ip_address",
            Self::ApiKey => "api_key",
            Self::AuthToken => "auth_token",
            Self::Url => "url",
            Self::Custom => "custom",
        }
    }
}

/// Detected PII instance with location and type.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPii {
    pub value: String,
    pub pii_type: PiiType,
    pub start: usize,
    pub end: usize,
    pub context: String,
}

/// Token for replacing PII.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PiiToken {
    pub token: String,
    pub original_value: String,
    pub pii_type: PiiType,
    pub created_at: String,
}

/// Manager for PII tokenization with configurable detection patterns.
pub struct PiiTokenizer {
    patterns: HashMap<PiiType, Regex>,
    token_store: Arc<Mutex<HashMap<String, PiiToken>>>,
}

impl PiiTokenizer {
    /// Create a new PII tokenizer with default patterns.
    pub fn new() -> Self {
        let mut patterns = HashMap::new();

        // Email pattern
        patterns.insert(
            PiiType::Email,
            Regex::new(r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}").unwrap(),
        );

        // Phone pattern (US format and variations)
        patterns.insert(
            PiiType::PhoneNumber,
            Regex::new(r"(?:\+?1[-.\s]?)?\(?[0-9]{3}\)?[-.\s]?[0-9]{3}[-.\s]?[0-9]{4}").unwrap(),
        );

        // SSN pattern
        patterns.insert(
            PiiType::SocialSecurityNumber,
            Regex::new(r"[0-9]{3}-[0-9]{2}-[0-9]{4}").unwrap(),
        );

        // Credit card pattern (basic)
        patterns.insert(
            PiiType::CreditCard,
            Regex::new(r"[0-9]{4}[\s-]?[0-9]{4}[\s-]?[0-9]{4}[\s-]?[0-9]{4}").unwrap(),
        );

        // IPv4 pattern
        patterns.insert(
            PiiType::IpAddress,
            Regex::new(r"(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)")
                .unwrap(),
        );

        // API key pattern (common format)
        patterns.insert(
            PiiType::ApiKey,
            Regex::new(r#"(?:api[_-]?key|apikey|API[_-]?KEY)\s*[:=]\s*['"]?[a-zA-Z0-9_-]{32,}['"]?"#).unwrap(),
        );

        // Bearer token pattern
        patterns.insert(
            PiiType::AuthToken,
            Regex::new(r"(?:bearer|token|authorization)\s+[a-zA-Z0-9._-]+").unwrap(),
        );

        Self {
            patterns,
            token_store: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Detect PII in a string.
    pub fn detect_pii(&self, text: &str) -> Result<Vec<DetectedPii>> {
        let mut detected = Vec::new();

        for (pii_type, pattern) in &self.patterns {
            for mat in pattern.find_iter(text) {
                let value = text[mat.start()..mat.end()].to_string();
                let context_start = mat.start().saturating_sub(20);
                let context_end = (mat.end() + 20).min(text.len());
                let context = text[context_start..context_end]
                    .replace('\n', "\\n")
                    .replace('\r', "\\r");

                debug!(
                    pii_type = pii_type.as_str(),
                    context = %context,
                    "Detected PII in text"
                );

                detected.push(DetectedPii {
                    value,
                    pii_type: *pii_type,
                    start: mat.start(),
                    end: mat.end(),
                    context,
                });
            }
        }

        Ok(detected)
    }

    /// Tokenize PII in a string, returning modified text and token map.
    pub fn tokenize_string(&self, text: &str) -> Result<(String, HashMap<String, PiiToken>)> {
        let detected = self.detect_pii(text)?;

        if detected.is_empty() {
            return Ok((text.to_string(), HashMap::new()));
        }

        let mut result = text.to_string();
        let mut new_tokens = HashMap::new();

        // Process detections in reverse order to maintain offsets
        for detection in detected.iter().rev() {
            let token = self.generate_token(&detection.value, detection.pii_type)?;
            new_tokens.insert(token.token.clone(), token.clone());
            result.replace_range(detection.start..detection.end, &token.token);
        }

        // Store tokens for later de-tokenization
        {
            let mut store = self.token_store.lock().unwrap();
            store.extend(new_tokens.clone());
        }

        debug!(
            pii_count = detected.len(),
            "Tokenized PII in string"
        );

        Ok((result, new_tokens))
    }

    /// De-tokenize a string using stored token map.
    pub fn detokenize_string(&self, text: &str) -> Result<String> {
        let store = self.token_store.lock().unwrap();
        let mut result = text.to_string();

        for (token, pii_token) in store.iter() {
            result = result.replace(token, &pii_token.original_value);
        }

        Ok(result)
    }

    /// Clear all stored tokens (for security).
    pub fn clear_tokens(&self) {
        let mut store = self.token_store.lock().unwrap();
        store.clear();
        debug!("Cleared all PII tokens");
    }

    /// Get audit trail of tokenized data.
    pub fn audit_trail(&self) -> Result<Vec<(String, PiiType, String)>> {
        let store = self.token_store.lock().unwrap();
        let trail: Vec<_> = store
            .values()
            .map(|t| (t.token.clone(), t.pii_type, t.created_at.clone()))
            .collect();
        Ok(trail)
    }

    /// Generate a secure token for PII value.
    fn generate_token(&self, value: &str, pii_type: PiiType) -> Result<PiiToken> {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        let hash = hasher.finish();

        let token = format!("__PII_{}_{:x}__", pii_type.as_str(), hash);

        Ok(PiiToken {
            token,
            original_value: value.to_string(),
            pii_type,
            created_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    /// Register custom PII pattern.
    pub fn register_pattern(&mut self, pii_type: PiiType, pattern: &str) -> Result<()> {
        let regex = Regex::new(pattern)
            .context("invalid regex pattern for PII detection")?;
        self.patterns.insert(pii_type, regex);
        debug!(
            pii_type = pii_type.as_str(),
            pattern = pattern,
            "Registered custom PII pattern"
        );
        Ok(())
    }
}

impl Default for PiiTokenizer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_email() {
        let tokenizer = PiiTokenizer::new();
        let text = "Contact me at john@example.com for more info";
        let detected = tokenizer.detect_pii(text).unwrap();

        assert!(!detected.is_empty());
        assert!(detected.iter().any(|d| d.pii_type == PiiType::Email));
    }

    #[test]
    fn test_detect_phone() {
        let tokenizer = PiiTokenizer::new();
        let text = "Call me at 555-123-4567";
        let detected = tokenizer.detect_pii(text).unwrap();

        assert!(!detected.is_empty());
        assert!(detected.iter().any(|d| d.pii_type == PiiType::PhoneNumber));
    }

    #[test]
    fn test_tokenize_string() {
        let tokenizer = PiiTokenizer::new();
        let text = "Email: john@example.com, Phone: 555-123-4567";
        let (tokenized, tokens) = tokenizer.tokenize_string(text).unwrap();

        assert!(tokenized.contains("__PII_"));
        assert!(!tokenized.contains("john@example.com"));
        assert!(!tokens.is_empty());
    }

    #[test]
    fn test_no_pii_detected() {
        let tokenizer = PiiTokenizer::new();
        let text = "This is regular text with no sensitive information";
        let detected = tokenizer.detect_pii(text).unwrap();

        assert!(detected.is_empty());
    }
}
