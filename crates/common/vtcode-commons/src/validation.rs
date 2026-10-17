//! Validation utilities for common operations
//!
//! This module follows the **"Parse, don't validate"** pattern:
//! boundary functions transform raw input into types that carry
//! their invariants (`NonEmptyString`, `NonEmptyVec`, `NonEmptySlice`),
//! so downstream code never re-checks.

use anyhow::{Result, bail};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as DeError};
use std::path::Path;

/// Validate that a string is non-empty
pub fn validate_non_empty(value: &str, field_name: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field_name} cannot be empty");
    }
    Ok(())
}

/// Validate and return non-empty string
pub fn validate_non_empty_string(value: String, field_name: &str) -> Result<String> {
    if value.trim().is_empty() {
        bail!("{field_name} cannot be empty");
    }
    Ok(value)
}

/// Validate optional non-empty string
pub fn validate_optional_non_empty(value: &Option<String>, field_name: &str) -> Result<()> {
    if let Some(v) = value {
        validate_non_empty(v, field_name)?;
    }
    Ok(())
}

/// Validate collection is not empty
pub fn validate_non_empty_collection<T>(collection: &[T], field_name: &str) -> Result<()> {
    if collection.is_empty() {
        bail!("{field_name} collection cannot be empty");
    }
    Ok(())
}

/// Validate that all strings in a slice are non-empty
pub fn validate_all_non_empty(values: &[String], field_name: &str) -> Result<()> {
    for (i, value) in values.iter().enumerate() {
        if value.trim().is_empty() {
            bail!("{field_name}[{i}] cannot be empty");
        }
    }
    Ok(())
}

/// Validate path exists
pub fn validate_path_exists(path: &Path, field_name: &str) -> Result<()> {
    if !path.exists() {
        bail!("{} path does not exist: {}", field_name, path.display());
    }
    Ok(())
}

/// Validate path is a file
pub fn validate_is_file(path: &Path, field_name: &str) -> Result<()> {
    validate_path_exists(path, field_name)?;
    if !path.is_file() {
        bail!("{} is not a file: {}", field_name, path.display());
    }
    Ok(())
}

/// Validate path is a directory
pub fn validate_is_directory(path: &Path, field_name: &str) -> Result<()> {
    validate_path_exists(path, field_name)?;
    if !path.is_dir() {
        bail!("{} is not a directory: {}", field_name, path.display());
    }
    Ok(())
}

/// Basic URL format validation
pub fn validate_url_format(url: &str, field_name: &str) -> Result<()> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        bail!("{field_name} must be a valid URL starting with http:// or https://");
    }
    Ok(())
}

/// Whether `origin` is a bare `http(s)://host[:port]` web origin: no wildcard,
/// whitespace, credentials, path (including a trailing slash), query, or fragment.
#[must_use]
pub fn is_valid_origin(origin: &str) -> bool {
    let Ok(parsed) = url::Url::parse(origin) else {
        return false;
    };
    origin == origin.trim()
        && !origin.chars().any(char::is_whitespace)
        && !origin.contains('*')
        && matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some_and(|host| !host.is_empty())
        && parsed.username().is_empty()
        && parsed.password().is_none()
        && (parsed.path().is_empty() || (parsed.path() == "/" && !origin.ends_with('/')))
        && parsed.query().is_none()
        && parsed.fragment().is_none()
}

/// Validate alphanumeric identifier
pub fn validate_identifier(id: &str, field_name: &str) -> Result<()> {
    if id.is_empty() {
        bail!("{field_name} cannot be empty");
    }
    if !id.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '-') {
        bail!("{field_name} must be alphanumeric (can include _ or -)");
    }
    Ok(())
}

/// A validated string that is guaranteed to be non-empty after trimming.
///
/// Follows the **"Parse Don't Validate"** pattern (Ch 15): the constraint is
/// enforced at construction time via [`TryFrom`], so downstream code never
/// needs to re-check.
///
/// ```rust
/// use vtcode_commons::validation::NonEmptyString;
///
/// let name = NonEmptyString::try_from("hello").unwrap();
/// assert_eq!(name.as_str(), "hello");
///
/// assert!(NonEmptyString::try_from("").is_err());
/// assert!(NonEmptyString::try_from("   ").is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NonEmptyString(String);

impl NonEmptyString {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

impl std::ops::Deref for NonEmptyString {
    type Target = str;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl std::borrow::Borrow<str> for NonEmptyString {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl AsRef<str> for NonEmptyString {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NonEmptyString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl TryFrom<String> for NonEmptyString {
    type Error = String;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            Err("string must be non-empty".to_string())
        } else {
            Ok(Self(value))
        }
    }
}

impl TryFrom<&str> for NonEmptyString {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value.trim().is_empty() {
            Err("string must be non-empty".to_string())
        } else {
            Ok(Self(value.to_string()))
        }
    }
}

impl From<NonEmptyString> for String {
    fn from(value: NonEmptyString) -> Self {
        value.0
    }
}

impl Serialize for NonEmptyString {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for NonEmptyString {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = String::deserialize(deserializer)?;
        if raw.trim().is_empty() {
            return Err(D::Error::invalid_value(serde::de::Unexpected::Str(&raw), &"a non-empty string"));
        }
        Ok(Self(raw))
    }
}

/// Error returned when a collection is empty but at least one element is required.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EmptyCollectionError;

impl std::fmt::Display for EmptyCollectionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("collection must contain at least one element")
    }
}

impl std::error::Error for EmptyCollectionError {}

/// A vector guaranteed to contain at least one element.
///
/// Follows the **"Parse, don't validate"** pattern: the non-empty invariant
/// is established once via [`NonEmptyVec::from_vec`] or `TryFrom<Vec<T>>`,
/// and [`NonEmptyVec::first`] is then infallible (returns `&T`, not `Option<&T>`).
///
/// The shape mirrors the `nonempty` crate discussed in Eli Bendersky's
/// "Rusty thoughts on Parse, don't validate": `head` holds the first element,
/// `tail` holds the rest, so no indexing or `unwrap` is needed downstream.
///
/// ```rust
/// use vtcode_commons::validation::NonEmptyVec;
///
/// let parsed = NonEmptyVec::from_vec(vec!["a", "b"]).expect("non-empty");
/// assert_eq!(parsed.first(), &"a");
/// assert!(NonEmptyVec::<String>::from_vec(Vec::new()).is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NonEmptyVec<T> {
    /// First element; always present.
    pub head: T,
    /// Remaining elements; possibly empty.
    pub tail: Vec<T>,
}

impl<T> NonEmptyVec<T> {
    /// Create a singleton collection from one element.
    pub const fn new(head: T) -> Self {
        Self { head, tail: Vec::new() }
    }

    /// Alias for [`NonEmptyVec::new`], matching the `nonempty` crate naming.
    pub const fn singleton(head: T) -> Self {
        Self { head, tail: Vec::new() }
    }

    /// Parse a `Vec` into a non-empty collection, returning `None` when empty.
    ///
    /// This is the single boundary where the invariant is established.
    /// Order is preserved: `head` is the original `vec[0]`.
    pub fn from_vec(vec: Vec<T>) -> Option<Self> {
        let mut iter = vec.into_iter();
        let head = iter.next()?;
        Some(Self { head, tail: iter.collect() })
    }

    /// Infallible access to the first element; no `Option` to re-check.
    #[must_use]
    pub const fn first(&self) -> &T {
        &self.head
    }

    /// Access the last element.
    #[must_use]
    pub fn last(&self) -> &T {
        self.tail.last().unwrap_or(&self.head)
    }

    /// Number of elements; always `>= 1`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.tail.len().saturating_add(1)
    }

    /// Always returns `false`; provided for generic `len`/`is_empty` pairing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Iterate over all elements in order.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        std::iter::once(&self.head).chain(self.tail.iter())
    }

    /// Append an element to the end.
    pub fn push(&mut self, value: T) {
        self.tail.push(value);
    }

    /// Consume into a plain `Vec`, preserving order.
    #[must_use]
    pub fn into_vec(self) -> Vec<T> {
        let mut vec = Vec::with_capacity(self.tail.len().saturating_add(1));
        vec.push(self.head);
        vec.extend(self.tail);
        vec
    }
}

impl<T> TryFrom<Vec<T>> for NonEmptyVec<T> {
    type Error = EmptyCollectionError;

    fn try_from(value: Vec<T>) -> std::result::Result<Self, Self::Error> {
        Self::from_vec(value).ok_or(EmptyCollectionError)
    }
}

impl<T> From<NonEmptyVec<T>> for Vec<T> {
    fn from(value: NonEmptyVec<T>) -> Self {
        value.into_vec()
    }
}

impl<T: Serialize> Serialize for NonEmptyVec<T> {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.collect_seq(self.iter())
    }
}

impl<'de, T> Deserialize<'de> for NonEmptyVec<T>
where
    T: Deserialize<'de>,
{
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = Vec::<T>::deserialize(deserializer)?;
        Self::from_vec(raw).ok_or_else(|| D::Error::invalid_length(0, &"a non-empty array"))
    }
}

/// A borrowed slice guaranteed to contain at least one element.
///
/// The borrowed counterpart to [`NonEmptyVec`]: parse once via
/// [`NonEmptySlice::from_slice`], then [`NonEmptySlice::first`] is
/// infallible. Avoids cloning large payloads (e.g. LLM `choices` arrays)
/// while still removing validate-then-index (`is_empty` + `[0]`) sites.
///
/// ```rust
/// use vtcode_commons::validation::NonEmptySlice;
///
/// let values = vec![10, 20];
/// let parsed = NonEmptySlice::from_slice(&values).expect("non-empty");
/// assert_eq!(parsed.first(), &10);
/// assert!(NonEmptySlice::<i32>::from_slice(&[]).is_none());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NonEmptySlice<'a, T> {
    /// First element; always present.
    first: &'a T,
    /// Remaining elements; possibly empty.
    rest: &'a [T],
}

impl<'a, T> NonEmptySlice<'a, T> {
    /// Parse a slice into a non-empty view, returning `None` when empty.
    ///
    /// This is the single boundary where the invariant is established.
    pub fn from_slice(slice: &'a [T]) -> Option<Self> {
        let (first, rest) = slice.split_first()?;
        Some(Self { first, rest })
    }

    /// Infallible access to the first element; no `Option` to re-check.
    #[must_use]
    pub const fn first(&self) -> &'a T {
        self.first
    }

    /// Access the last element.
    #[must_use]
    pub fn last(&self) -> &'a T {
        self.rest.last().unwrap_or(self.first)
    }

    /// Number of elements; always `>= 1`.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rest.len().saturating_add(1)
    }

    /// Always returns `false`; provided for generic `len`/`is_empty` pairing.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        false
    }

    /// Borrow the tail after the first element; possibly empty.
    ///
    /// Useful for argv-style parsing where `first` is the program and
    /// `rest` are the arguments.
    #[must_use]
    pub const fn rest(&self) -> &'a [T] {
        self.rest
    }

    /// Iterate over all elements in order.
    pub fn iter(&self) -> impl Iterator<Item = &'a T> {
        std::iter::once(self.first).chain(self.rest.iter())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_valid_origin_accepts_only_bare_web_origins() {
        for ok in ["http://localhost", "https://example.com", "http://127.0.0.1:3000"] {
            assert!(is_valid_origin(ok), "{ok}");
        }
        for bad in [
            "",
            "https://*.example.com",
            "https://example.com/",
            "https://example.com/path",
            "https://example.com?q=1",
            "https://example.com#frag",
            "https://user:pw@example.com",
            "ftp://example.com",
            " https://example.com",
            "https://exa mple.com",
        ] {
            assert!(!is_valid_origin(bad), "{bad}");
        }
    }

    #[test]
    fn test_validate_non_empty() {
        assert!(validate_non_empty("test", "field").is_ok());
        assert!(validate_non_empty("", "field").is_err());
        assert!(validate_non_empty("   ", "field").is_err());
    }

    #[test]
    fn test_validate_all_non_empty() {
        assert!(validate_all_non_empty(&["a".to_string(), "b".to_string()], "field").is_ok());
        assert!(validate_all_non_empty(&["a".to_string(), "".to_string()], "field").is_err());
        assert!(validate_all_non_empty(&[], "field").is_ok());
    }

    #[test]
    fn non_empty_string_accepts_valid() {
        let s = NonEmptyString::try_from("hello").unwrap();
        assert_eq!(s.as_str(), "hello");
        assert_eq!(s.len(), 5);
    }

    #[test]
    fn non_empty_string_rejects_empty() {
        assert!(NonEmptyString::try_from("").is_err());
        assert!(NonEmptyString::try_from("   ").is_err());
        assert!(NonEmptyString::try_from("\t\n").is_err());
    }

    #[test]
    fn non_empty_string_from_owned() {
        let s = NonEmptyString::try_from("test".to_string()).unwrap();
        assert_eq!(s.into_inner(), "test");
    }

    #[test]
    fn non_empty_string_deref() {
        let s = NonEmptyString::try_from("hello").unwrap();
        assert!(s.starts_with("hel"));
        assert_eq!(&*s, "hello");
    }

    #[test]
    fn non_empty_string_serde_roundtrip_and_rejection() {
        let parsed = NonEmptyString::try_from("hello").unwrap();
        let json = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, "\"hello\"");
        let back: NonEmptyString = serde_json::from_str(&json).unwrap();
        assert_eq!(back, parsed);

        assert!(serde_json::from_str::<NonEmptyString>("\"\"").is_err());
        assert!(serde_json::from_str::<NonEmptyString>("\"   \"").is_err());
        let spaced: NonEmptyString = serde_json::from_str("\"  hello  \"").unwrap();
        assert_eq!(spaced.as_str(), "  hello  ");
    }

    #[test]
    fn non_empty_vec_preserves_order_asymmetric() {
        let forward = NonEmptyVec::from_vec(vec!["a", "b"]).unwrap();
        let backward = NonEmptyVec::from_vec(vec!["b", "a"]).unwrap();
        assert_eq!(forward.first(), &"a");
        assert_eq!(backward.first(), &"b");
        assert_ne!(forward, backward);
        assert_eq!(forward.into_vec(), vec!["a", "b"]);
        assert_eq!(backward.into_vec(), vec!["b", "a"]);
    }

    #[test]
    fn non_empty_vec_singleton_and_empty_boundary() {
        assert!(NonEmptyVec::<String>::from_vec(Vec::new()).is_none());
        let singleton = NonEmptyVec::singleton("only".to_string());
        assert_eq!(singleton.first(), "only");
        assert_eq!(singleton.last(), "only");
        assert_eq!(singleton.len(), 1);
        assert!(!singleton.is_empty());
        assert_eq!(singleton.into_vec(), vec!["only".to_string()]);
    }

    #[test]
    fn non_empty_vec_try_from_reports_typed_error() {
        let parsed = NonEmptyVec::try_from(vec![1, 2, 3]).unwrap();
        assert_eq!(parsed.first(), &1);
        assert_eq!(parsed.last(), &3);
        assert_eq!(parsed.len(), 3);
        let err = NonEmptyVec::<i32>::try_from(Vec::new()).unwrap_err();
        assert_eq!(err, EmptyCollectionError);
        assert_eq!(err.to_string(), "collection must contain at least one element");
    }

    #[test]
    fn non_empty_vec_iter_push_and_serde() {
        let mut parsed = NonEmptyVec::new("x".to_string());
        parsed.push("y".to_string());
        let collected: Vec<&String> = parsed.iter().collect();
        assert_eq!(collected, vec!["x", "y"]);
        assert_eq!(parsed.len(), 2);

        let json = serde_json::to_string(&parsed).unwrap();
        assert_eq!(json, "[\"x\",\"y\"]");
        let back: NonEmptyVec<String> = serde_json::from_str(&json).unwrap();
        assert_eq!(back, parsed);
        assert!(serde_json::from_str::<NonEmptyVec<String>>("[]").is_err());
    }

    #[test]
    fn non_empty_slice_parses_once_then_first_is_infallible() {
        let forward = vec!["a", "b"];
        let backward = vec!["b", "a"];
        let parsed_forward = NonEmptySlice::from_slice(&forward).unwrap();
        let parsed_backward = NonEmptySlice::from_slice(&backward).unwrap();
        assert_eq!(parsed_forward.first(), &"a");
        assert_eq!(parsed_backward.first(), &"b");
        assert_ne!(parsed_forward, parsed_backward);
        assert_eq!(parsed_forward.len(), 2);
        assert!(!parsed_forward.is_empty());

        let empty: Vec<String> = Vec::new();
        assert!(NonEmptySlice::from_slice(&empty).is_none());

        let singleton = vec!["only"];
        let parsed_single = NonEmptySlice::from_slice(&singleton).unwrap();
        assert_eq!(parsed_single.first(), &"only");
        assert_eq!(parsed_single.last(), &"only");
        assert_eq!(parsed_single.rest(), &[] as &[&str]);
        let collected: Vec<&&str> = parsed_forward.iter().collect();
        assert_eq!(collected, vec![&"a", &"b"]);
        assert_eq!(parsed_forward.rest(), &["b"] as &[&str]);
    }
}
