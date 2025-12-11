use std::collections::hash_map::DefaultHasher;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};
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

/// Integrity tag that can be recomputed to detect tampering.
#[derive(Debug, Clone)]
pub struct IntegrityTag(u64);

impl IntegrityTag {
    pub fn new(payload: &Value, salt: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        payload.to_string().hash(&mut hasher);
        salt.hash(&mut hasher);
        IntegrityTag(hasher.finish())
    }

    pub fn verify(&self, payload: &Value, salt: &str) -> bool {
        let expected = IntegrityTag::new(payload, salt);
        expected.0 == self.0
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
