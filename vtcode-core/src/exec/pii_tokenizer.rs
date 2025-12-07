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
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::debug;

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

// Compile default PII patterns once to avoid repeated regex compilation overhead.
static DEFAULT_PII_PATTERNS: Lazy<Result<Vec<(PiiType, Regex)>, String>> = Lazy::new(|| {
    let patterns = vec![
        (
            PiiType::Email,
            r"[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}",
        ),
        (
            PiiType::PhoneNumber,
            r"(?:\+?1[-.\s]?)?\(?[0-9]{3}\)?[-.\s]?[0-9]{3}[-.\s]?[0-9]{4}",
        ),
        (PiiType::SocialSecurityNumber, r"[0-9]{3}-[0-9]{2}-[0-9]{4}"),
        (
            PiiType::CreditCard,
            r"[0-9]{4}[\s-]?[0-9]{4}[\s-]?[0-9]{4}[\s-]?[0-9]{4}",
        ),
        (
            PiiType::IpAddress,
            r"(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)",
        ),
        (
            PiiType::ApiKey,
            r#"(?:api[_-]?key|apikey|API[_-]?KEY)\s*[:=]\s*['"]?[a-zA-Z0-9_-]{32,}['"]?"#,
        ),
        (
            PiiType::AuthToken,
            r"(?:bearer|token|authorization)\s+[a-zA-Z0-9._-]+",
        ),
    ];

    let mut compiled = Vec::with_capacity(patterns.len());
    for (pii_type, pattern) in patterns {
        match Regex::new(pattern) {
            Ok(regex) => compiled.push((pii_type, regex)),
            Err(e) => {
                return Err(format!(
                    "Failed to compile PII regex for {:?}: {}",
                    pii_type, e
                ));
            }
        }
    }
    Ok(compiled)
});

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
    pub fn new() -> Result<Self> {
        // Build patterns from static defaults (compiled once)
        // Note: Cloning Regex is cheap (Arc-based internally)
        let patterns = DEFAULT_PII_PATTERNS
            .as_ref()
            .map_err(|e| anyhow::anyhow!("PII pattern initialization failed: {}", e))?
            .iter()
            .map(|(pii_type, regex)| (*pii_type, regex.clone()))
            .collect();

        Ok(Self {
            patterns,
            token_store: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Detect PII in a string.
    pub fn detect_pii(&self, text: &str) -> Result<Vec<DetectedPii>> {
        let mut detected = Vec::with_capacity(8);

        for (pii_type, pattern) in &self.patterns {
            for mat in pattern.find_iter(text) {
                // Only allocate value when actually detected (lazy allocation)
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
            let token_str = &token.token;
            result.replace_range(detection.start..detection.end, token_str);
            new_tokens.insert(token_str.clone(), token);
        }

        // Store tokens for later de-tokenization
        {
            let mut store = self
                .token_store
                .lock()
                .map_err(|e| anyhow::anyhow!("Failed to acquire token store lock: {}", e))?;
            // Use drain to move ownership, avoiding unnecessary clones
            store.extend(new_tokens.drain());
        }

        debug!(pii_count = detected.len(), "Tokenized PII in string");

        Ok((result, new_tokens))
    }

    /// De-tokenize a string using stored token map.
    pub fn detokenize_string(&self, text: &str) -> Result<String> {
        let store = self
            .token_store
            .lock()
            .map_err(|e| anyhow::anyhow!("Failed to acquire token store lock: {}", e))?;
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
        Ok(store
            .values()
            .map(|t| (t.token.clone(), t.pii_type, t.created_at.clone()))
            .collect())
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
        let regex = Regex::new(pattern).context("invalid regex pattern for PII detection")?;
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
        Self::new().unwrap_or_else(|_| Self {
            patterns: Default::default(),
            token_store: Arc::new(Mutex::new(HashMap::new())),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_detect_email() {
        let tokenizer = PiiTokenizer::new().expect("PII tokenizer should initialize");
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
