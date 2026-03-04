use serde_json::{Value, json};

pub const NEXT_CONTINUE_PROMPT: &str = "Use `next_continue_args`.";
pub const NEXT_READ_PROMPT: &str = "Use `next_read_args`.";
pub const DEFAULT_NEXT_READ_LIMIT: usize = 40;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PtyContinuationArgs {
    pub session_id: String,
}

impl PtyContinuationArgs {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
        }
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        value
            .get("session_id")
            .and_then(Value::as_str)
            .map(Self::new)
    }

    pub fn to_value(&self) -> Value {
        json!({ "session_id": self.session_id })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadChunkContinuationArgs {
    pub path: String,
    pub offset: usize,
    pub limit: usize,
}

impl ReadChunkContinuationArgs {
    pub fn new(path: impl Into<String>, offset: usize, limit: usize) -> Self {
        Self {
            path: path.into(),
            offset: offset.max(1),
            limit: limit.max(1),
        }
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        let path = value.get("path").and_then(Value::as_str)?.to_string();
        let offset = value.get("offset").and_then(value_to_usize)?.max(1);
        let limit = value
            .get("limit")
            .and_then(value_to_usize)
            .unwrap_or(DEFAULT_NEXT_READ_LIMIT)
            .max(1);
        Some(Self {
            path,
            offset,
            limit,
        })
    }

    pub fn to_value(&self) -> Value {
        json!({
            "path": self.path,
            "offset": self.offset,
            "limit": self.limit
        })
    }
}

pub fn read_chunk_progress_from_result(result: &Value) -> Option<(usize, usize)> {
    if let Some(next_read_args) = result
        .get("next_read_args")
        .and_then(ReadChunkContinuationArgs::from_value)
    {
        return Some((next_read_args.offset, next_read_args.limit));
    }

    let next_offset = result.get("next_offset").and_then(value_to_usize)?.max(1);
    let chunk_limit = result
        .get("chunk_limit")
        .and_then(value_to_usize)
        .unwrap_or(DEFAULT_NEXT_READ_LIMIT)
        .max(1);
    Some((next_offset, chunk_limit))
}

fn value_to_usize(value: &Value) -> Option<usize> {
    value
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .or_else(|| value.as_str().and_then(|s| s.parse::<usize>().ok()))
}

#[cfg(test)]
mod tests {
    use super::{PtyContinuationArgs, ReadChunkContinuationArgs, read_chunk_progress_from_result};
    use serde_json::json;

    #[test]
    fn pty_continuation_round_trips() {
        let args = PtyContinuationArgs::new("run-123");
        let payload = args.to_value();
        let parsed = PtyContinuationArgs::from_value(&payload).unwrap();

        assert_eq!(parsed.session_id, "run-123");
    }

    #[test]
    fn read_chunk_continuation_round_trips() {
        let args = ReadChunkContinuationArgs::new("out.txt", 41, 40);
        let payload = args.to_value();
        let parsed = ReadChunkContinuationArgs::from_value(&payload).unwrap();

        assert_eq!(parsed.path, "out.txt");
        assert_eq!(parsed.offset, 41);
        assert_eq!(parsed.limit, 40);
    }

    #[test]
    fn read_chunk_continuation_accepts_string_numbers() {
        let parsed = ReadChunkContinuationArgs::from_value(&json!({
            "path": "out.txt",
            "offset": "2",
            "limit": "3"
        }))
        .unwrap();

        assert_eq!(parsed.offset, 2);
        assert_eq!(parsed.limit, 3);
    }

    #[test]
    fn read_chunk_progress_prefers_canonical_args() {
        let result = json!({
            "next_read_args": {
                "path": "out.txt",
                "offset": 81,
                "limit": 40
            },
            "next_offset": 999,
            "chunk_limit": 999
        });
        assert_eq!(read_chunk_progress_from_result(&result), Some((81, 40)));
    }

    #[test]
    fn read_chunk_progress_supports_legacy_fields() {
        let result = json!({
            "next_offset": "10",
            "chunk_limit": "0"
        });
        assert_eq!(read_chunk_progress_from_result(&result), Some((10, 1)));
    }
}
