use std::borrow::Cow;
use std::fmt;

use anyhow::{Error, Result};

// File operation errors
pub const ERR_READ_FILE: &str = "failed to read file";
pub const ERR_WRITE_FILE: &str = "failed to write file";
pub const ERR_READ_DIR: &str = "failed to read directory";
pub const ERR_CREATE_DIR: &str = "failed to create directory";
pub const ERR_REMOVE_FILE: &str = "failed to remove file";
pub const ERR_REMOVE_DIR: &str = "failed to remove directory";
pub const ERR_READ_DIR_ENTRY: &str = "failed to read directory entry";
pub const ERR_GET_FILE_TYPE: &str = "failed to read file type";
pub const ERR_GET_METADATA: &str = "failed to read file metadata";
pub const ERR_CANONICALIZE_PATH: &str = "failed to canonicalize path";
pub const ERR_READ_SYMLINK: &str = "failed to read symlink";

// Skill/Tool errors
pub const ERR_CREATE_SKILLS_DIR: &str = "failed to create skills directory";
pub const ERR_CREATE_SKILL_DIR: &str = "failed to create skill directory";
pub const ERR_READ_SKILL_CODE: &str = "failed to read skill code";
pub const ERR_WRITE_SKILL_CODE: &str = "failed to write skill code";
pub const ERR_READ_SKILL_METADATA: &str = "failed to read skill metadata";
pub const ERR_WRITE_SKILL_METADATA: &str = "failed to write skill metadata";
pub const ERR_PARSE_SKILL_METADATA: &str = "failed to parse skill metadata";
pub const ERR_WRITE_SKILL_DOCS: &str = "failed to write skill documentation";
pub const ERR_DELETE_SKILL: &str = "failed to delete skill";
pub const ERR_READ_SKILLS_DIR: &str = "failed to read skills directory";
pub const ERR_TOOL_DENIED: &str = "tool denied or unavailable by policy";

// Audit/Logging errors
pub const ERR_CREATE_AUDIT_DIR: &str = "Failed to create audit directory";
pub const ERR_WRITE_AUDIT_LOG: &str = "failed to write audit log";

// Checkpoint/Snapshot errors
pub const ERR_CREATE_CHECKPOINT_DIR: &str = "failed to create checkpoint directory";
pub const ERR_WRITE_CHECKPOINT: &str = "failed to write checkpoint";
pub const ERR_READ_CHECKPOINT: &str = "failed to read checkpoint";

// Policy errors
pub const ERR_CREATE_POLICY_DIR: &str = "Failed to create directory for tool policy config";
pub const ERR_CREATE_WORKSPACE_POLICY_DIR: &str = "Failed to create workspace policy directory";

// Serialization errors
pub const ERR_SERIALIZE_METADATA: &str = "failed to serialize skill metadata";
pub const ERR_SERIALIZE_STATE: &str = "failed to serialize state";
pub const ERR_DESERIALIZE: &str = "failed to deserialize data";

// IPC/SDK errors
pub const ERR_CREATE_IPC_DIR: &str = "failed to create IPC directory";
pub const ERR_READ_REQUEST_FILE: &str = "failed to read request file";
pub const ERR_READ_REQUEST_JSON: &str = "failed to read request JSON";
pub const ERR_PARSE_REQUEST_JSON: &str = "failed to parse request JSON";
pub const ERR_PARSE_ARGS: &str = "failed to parse tokenized args";
pub const ERR_PARSE_RESULT: &str = "failed to parse de-tokenized result";

/// Helper macro for file operation errors with context
/// Usage: file_err!("path", "read") -> "failed to read path"
#[macro_export]
macro_rules! file_err {
    ($path:expr, read) => {
        format!("failed to read {}", $path)
    };
    ($path:expr, write) => {
        format!("failed to write {}", $path)
    };
    ($path:expr, delete) => {
        format!("failed to delete {}", $path)
    };
    ($path:expr, create) => {
        format!("failed to create {}", $path)
    };
}

/// Helper macro for context errors
/// Usage: ctx_err!(operation, context) -> "operation context"
#[macro_export]
macro_rules! ctx_err {
    ($op:expr, $ctx:expr) => {
        format!("{}: {}", $op, $ctx)
    };
}

/// Formats an error into a user-facing description. This allows extracted
/// components to present consistent error messaging without depending on the
/// CLI presentation layer.
pub trait ErrorFormatter: Send + Sync {
    /// Render the error into a user-facing string.
    fn format_error(&self, error: &Error) -> Cow<'_, str>;
}

/// Reports non-fatal errors to an observability backend.
pub trait ErrorReporter: Send + Sync {
    /// Capture the provided error for later inspection.
    fn capture(&self, error: &Error) -> Result<()>;

    /// Convenience helper to capture a simple message.
    fn capture_message(&self, message: impl Into<Cow<'static, str>>) -> Result<()> {
        let message: Cow<'static, str> = message.into();
        self.capture(&Error::msg(message))
    }
}

/// Error reporting implementation that drops every event. Useful for tests or
/// when a consumer does not yet integrate with error monitoring.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopErrorReporter;

impl ErrorReporter for NoopErrorReporter {
    fn capture(&self, _error: &Error) -> Result<()> {
        Ok(())
    }
}

/// Default formatter that surfaces the error's display output.
#[derive(Debug, Default, Clone, Copy)]
pub struct DisplayErrorFormatter;

impl ErrorFormatter for DisplayErrorFormatter {
    fn format_error(&self, error: &Error) -> Cow<'_, str> {
        Cow::Owned(format!("{error}"))
    }
}

/// A collection of errors that enables continuing work while collecting failures.
///
/// This type implements the "error parameter" pattern: instead of short-circuiting
/// on the first error, processing continues and errors are accumulated. The caller
/// can inspect the collection afterward to determine whether all operations
/// succeeded.
///
/// # Ergonomic Result handling
///
/// [`collect_result`](MultiErrors::collect_result) lets you process a `Result<T, E>`
/// while keeping the happy path dominant:
///
/// ```rust
/// use vtcode_commons::MultiErrors;
/// let mut errors: MultiErrors<String> = MultiErrors::new();
/// let value: Option<i32> = errors.collect_result("42".parse::<i32>().map_err(|e| e.to_string()));
/// assert_eq!(value, Some(42));
/// ```
///
/// # Composing with traditional error handling
///
/// Use [`ok`](MultiErrors::ok) or [`to_anyhow`](MultiErrors::to_anyhow) to convert
/// back into a traditional `Result`.
#[derive(Debug, Clone)]
pub struct MultiErrors<E = Error> {
    errors: Vec<E>,
}

impl<E> MultiErrors<E> {
    /// Create an empty error collection.
    pub fn new() -> Self {
        Self { errors: Vec::new() }
    }

    /// Add a single error to the collection.
    pub fn push(&mut self, error: E) {
        self.errors.push(error);
    }

    /// Extend the collection with multiple errors.
    fn extend(&mut self, iter: impl IntoIterator<Item = E>) {
        self.errors.extend(iter);
    }

    /// Returns `true` if no errors have been collected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.errors.is_empty()
    }

    /// Returns the number of collected errors.
    #[must_use]
    pub fn len(&self) -> usize {
        self.errors.len()
    }

    /// Consume the collector and return the underlying error vector.
    #[must_use]
    fn into_inner(self) -> Vec<E> {
        self.errors
    }

    /// Returns a slice of all collected errors.
    #[must_use]
    pub fn as_slice(&self) -> &[E] {
        &self.errors
    }

    /// Returns an iterator over the collected errors.
    pub fn iter(&self) -> std::slice::Iter<'_, E> {
        self.errors.iter()
    }

    /// Convert into `Result<()>` — succeeds if no errors were collected.
    fn ok(self) -> std::result::Result<(), Self> {
        if self.errors.is_empty() { Ok(()) } else { Err(self) }
    }

    /// Remove all errors from the collection.
    pub fn clear(&mut self) {
        self.errors.clear();
    }

    /// Process a `Result`, returning the success value or collecting the error.
    ///
    /// This is the key ergonomic method — it keeps the happy path as the primary
    /// flow while silently collecting errors for later inspection.
    pub fn collect_result<T, F>(&mut self, result: std::result::Result<T, F>) -> Option<T>
    where
        F: Into<E>,
    {
        match result {
            Ok(val) => Some(val),
            Err(e) => {
                self.errors.push(e.into());
                None
            }
        }
    }

    /// Convert into an [`anyhow::Error`] for use with traditional error handling.
    #[must_use]
    fn to_anyhow(&self) -> Error
    where
        E: fmt::Display,
    {
        Error::msg(self.to_string())
    }
}

impl<E> Default for MultiErrors<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E> From<Vec<E>> for MultiErrors<E> {
    fn from(errors: Vec<E>) -> Self {
        Self { errors }
    }
}

impl<E> IntoIterator for MultiErrors<E> {
    type Item = E;
    type IntoIter = std::vec::IntoIter<E>;

    fn into_iter(self) -> Self::IntoIter {
        self.errors.into_iter()
    }
}

impl<E: serde::Serialize> serde::Serialize for MultiErrors<E> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.errors.serialize(serializer)
    }
}

impl<'de, E: serde::Deserialize<'de>> serde::Deserialize<'de> for MultiErrors<E> {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Vec::<E>::deserialize(deserializer).map(|errors| Self { errors })
    }
}

impl<E: fmt::Display> fmt::Display for MultiErrors<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.errors.len() {
            0 => write!(f, "no errors"),
            1 => write!(f, "{}", self.errors[0]),
            _ => {
                for (i, error) in self.errors.iter().enumerate() {
                    if i > 0 {
                        writeln!(f)?;
                    }
                    write!(f, "  {}. {error}", i + 1)?;
                }
                Ok(())
            }
        }
    }
}

impl<E: std::error::Error + 'static> std::error::Error for MultiErrors<E> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.errors.first().map(|e| e as &(dyn std::error::Error + 'static))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formatter_uses_display() {
        let formatter = DisplayErrorFormatter;
        let error = Error::msg("test error");
        assert_eq!(formatter.format_error(&error), "test error");
    }

    #[test]
    fn noop_reporter_drops_errors() {
        let reporter = NoopErrorReporter;
        let error = Error::msg("test");
        assert!(reporter.capture(&error).is_ok());
        assert!(reporter.capture_message("message").is_ok());
    }

    #[test]
    fn multi_errors_new_is_empty() {
        let errors: MultiErrors<String> = MultiErrors::new();
        assert!(errors.is_empty());
        assert_eq!(errors.len(), 0);
    }

    #[test]
    fn multi_errors_push_and_len() {
        let mut errors = MultiErrors::new();
        errors.push("error 1".to_string());
        errors.push("error 2".to_string());
        assert!(!errors.is_empty());
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn multi_errors_collect_result_ok() {
        let mut errors: MultiErrors<String> = MultiErrors::new();
        let value: i32 = errors.collect_result(Ok::<_, String>(42)).unwrap_or(0);
        assert_eq!(value, 42);
        assert!(errors.is_empty());
    }

    #[test]
    fn multi_errors_collect_result_err() {
        let mut errors: MultiErrors<String> = MultiErrors::new();
        let value: i32 = errors.collect_result(Err::<i32, String>("bad".to_string())).unwrap_or(0);
        assert_eq!(value, 0);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn multi_errors_ok_succeeds_when_empty() {
        let errors: MultiErrors<String> = MultiErrors::new();
        assert!(errors.ok().is_ok());
    }

    #[test]
    fn multi_errors_ok_fails_when_not_empty() {
        let mut errors = MultiErrors::new();
        errors.push("error".to_string());
        assert!(errors.ok().is_err());
    }

    #[test]
    fn multi_errors_display_empty() {
        let errors: MultiErrors<String> = MultiErrors::new();
        assert_eq!(errors.to_string(), "no errors");
    }

    #[test]
    fn multi_errors_display_single() {
        let mut errors = MultiErrors::new();
        errors.push("something failed".to_string());
        assert_eq!(errors.to_string(), "something failed");
    }

    #[test]
    fn multi_errors_display_multiple() {
        let mut errors = MultiErrors::new();
        errors.push("first issue".to_string());
        errors.push("second issue".to_string());
        let display = errors.to_string();
        assert!(display.contains("1. first issue"));
        assert!(display.contains("2. second issue"));
    }

    #[test]
    fn multi_errors_extend() {
        let mut errors = MultiErrors::new();
        errors.extend(vec!["a".to_string(), "b".to_string()]);
        assert_eq!(errors.len(), 2);
    }

    #[test]
    fn multi_errors_into_inner() {
        let mut errors = MultiErrors::new();
        errors.push("test".to_string());
        let inner: Vec<String> = errors.into_inner();
        assert_eq!(inner.len(), 1);
    }

    #[test]
    fn multi_errors_from_vec() {
        let errors: MultiErrors<String> = MultiErrors::from(vec!["a".to_string()]);
        assert_eq!(errors.len(), 1);
    }

    #[test]
    fn multi_errors_into_iterator() {
        let mut errors = MultiErrors::new();
        errors.push("a".to_string());
        errors.push("b".to_string());
        let collected: Vec<String> = errors.into_iter().collect();
        assert_eq!(collected, vec!["a", "b"]);
    }

    #[test]
    fn multi_errors_slice_access() {
        let mut errors = MultiErrors::new();
        errors.push("err".to_string());
        assert_eq!(errors.as_slice(), &["err".to_string()]);
    }

    #[test]
    fn multi_errors_to_anyhow() {
        let mut errors = MultiErrors::new();
        errors.push("something broke".to_string());
        let err = errors.to_anyhow();
        assert!(err.to_string().contains("something broke"));
    }
}
