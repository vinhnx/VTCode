//! Serde helper utilities

use serde::{Deserialize, Deserializer};
use std::fmt::Display;
use std::str::FromStr;

/// Render a `serde_json::Value` as pretty-printed JSON, falling back to its
/// `Display` impl when the value cannot be serialized.
///
/// This is the standard fallback used across the workspace for log lines and
/// spooled output where we want a readable form but still want a non-empty
/// string even for exotic value shapes.
pub fn json_to_string_pretty(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string())
}

/// Deserializes a value that can be represented as either its native type or a quoted string.
/// This is particularly useful for LLM tool calls which sometimes quote numeric arguments.
pub fn deserialize_maybe_quoted<'de, T, D>(deserializer: D) -> Result<T, D::Error>
where
    T: FromStr + Deserialize<'de>,
    T::Err: Display,
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MaybeQuoted<T> {
        Native(T),
        Quoted(String),
    }

    match MaybeQuoted::<T>::deserialize(deserializer)? {
        MaybeQuoted::Native(val) => Ok(val),
        MaybeQuoted::Quoted(s) => T::from_str(&s).map_err(serde::de::Error::custom),
    }
}

/// Deserializes an optional value that can be represented as either its native type or a quoted string.
pub fn deserialize_opt_maybe_quoted<'de, T, D>(deserializer: D) -> Result<Option<T>, D::Error>
where
    T: FromStr + Deserialize<'de>,
    T::Err: Display,
    D: Deserializer<'de>,
{
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum MaybeQuoted<T> {
        Native(T),
        Quoted(String),
        Null,
    }

    match MaybeQuoted::<T>::deserialize(deserializer)? {
        MaybeQuoted::Native(val) => Ok(Some(val)),
        MaybeQuoted::Quoted(s) => T::from_str(&s).map(Some).map_err(serde::de::Error::custom),
        MaybeQuoted::Null => Ok(None),
    }
}
