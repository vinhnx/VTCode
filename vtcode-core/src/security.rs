use std::collections::HashSet;

use std::time::SystemTime;

use anyhow::{Result, ensure};
use serde_json::Value;

/// Zero-trust context that guards cross-component requests.
#[derive(Debug, Clone)]
pub struct ZeroTrustContext {
    allowed_identities: HashSet<String>,
    integrity_salt: String,
}

impl ZeroTrustContext {
    pub fn new(allowed_identities: HashSet<String>, integrity_salt: impl Into<String>) -> Self {
        Self {
            allowed_identities,
            integrity_salt: integrity_salt.into(),
        }
    }

    pub fn authorize(&self, identity: &str) -> Result<()> {
        ensure!(
            self.allowed_identities.contains(identity),
            "principal {} not authorized under zero-trust policy",
            identity
        );
        Ok(())
    }

    pub fn wrap(&self, payload: Value) -> PayloadEnvelope {
        let integrity = IntegrityTag::new(&payload, &self.integrity_salt);
        PayloadEnvelope {
            payload,
            integrity,
            issued_at: SystemTime::now(),
        }
    }
}

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use ring::hmac;

/// Integrity tag that can be recomputed to detect tampering.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IntegrityTag(String);

impl IntegrityTag {
    pub fn new(payload: &Value, salt: &str) -> Self {
        let key = hmac::Key::new(hmac::HMAC_SHA256, salt.as_bytes());
        let signature = hmac::sign(&key, payload.to_string().as_bytes());
        IntegrityTag(STANDARD.encode(signature.as_ref()))
    }

    pub fn verify(&self, payload: &Value, salt: &str) -> bool {
        let key = hmac::Key::new(hmac::HMAC_SHA256, salt.as_bytes());
        let expected_signature_bytes = match STANDARD.decode(&self.0) {
            Ok(bytes) => bytes,
            Err(_) => return false,
        };

        hmac::verify(
            &key,
            payload.to_string().as_bytes(),
            &expected_signature_bytes,
        )
        .is_ok()
    }
}

/// Encrypted or integrity-protected payload wrapper.
#[derive(Debug, Clone)]
pub struct PayloadEnvelope {
    pub payload: Value,
    pub integrity: IntegrityTag,
    pub issued_at: SystemTime,
}

impl PayloadEnvelope {
    pub fn validate(&self, salt: &str) -> Result<()> {
        ensure!(
            self.integrity.verify(&self.payload, salt),
            "payload integrity check failed"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::iter::FromIterator;

    #[test]
    fn rejects_unknown_identity() {
        let ctx = ZeroTrustContext::new(HashSet::from_iter(["node-a".to_string()]), "salt");
        let err = ctx.authorize("node-b").unwrap_err();
        assert!(err.to_string().contains("not authorized"));
    }

    #[test]
    fn detects_tampering() {
        let ctx = ZeroTrustContext::new(HashSet::from_iter(["node-a".to_string()]), "salt");
        let mut envelope = ctx.wrap(serde_json::json!({"a": 1}));
        envelope.payload = serde_json::json!({"a": 2});
        let err = envelope.validate("salt").unwrap_err();
        assert!(err.to_string().contains("integrity"));
    }
}
