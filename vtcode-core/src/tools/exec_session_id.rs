use std::borrow::Borrow;
use std::fmt;
use std::ops::Deref;

use serde::Deserialize;
use serde::Serialize;

/// Logical identifier for a VTCode exec session.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExecSessionId(String);

impl ExecSessionId {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl Deref for ExecSessionId {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        self.as_str()
    }
}

impl Borrow<str> for ExecSessionId {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for ExecSessionId {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for ExecSessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

impl From<String> for ExecSessionId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for ExecSessionId {
    fn from(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl From<&String> for ExecSessionId {
    fn from(value: &String) -> Self {
        Self(value.clone())
    }
}

impl From<ExecSessionId> for String {
    fn from(value: ExecSessionId) -> Self {
        value.0
    }
}
