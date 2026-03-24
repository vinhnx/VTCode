use serde_json::{Value, json};

pub const NEXT_CONTINUE_PROMPT: &str = "Reuse `next_continue_args`.";
pub const NEXT_READ_PROMPT: &str = "Reuse `next_read_args`.";
pub const DEFAULT_NEXT_READ_LIMIT: usize = 40;

const SESSION_ID_KEY: &str = "session_id";
const COMPACT_SESSION_ID_KEY: &str = "s";
const PATH_KEY: &str = "path";
const COMPACT_PATH_KEY: &str = "p";
const OFFSET_KEY: &str = "offset";
const COMPACT_OFFSET_KEY: &str = "o";
const LIMIT_KEY: &str = "limit";
const COMPACT_LIMIT_KEY: &str = "l";

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
            .get(SESSION_ID_KEY)
            .or_else(|| value.get(COMPACT_SESSION_ID_KEY))
            .and_then(Value::as_str)
            .map(Self::new)
    }

    pub fn to_value(&self) -> Value {
        json!({ SESSION_ID_KEY: self.session_id })
    }

    pub fn to_compact_value(&self) -> Value {
        json!({ COMPACT_SESSION_ID_KEY: self.session_id })
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
        let path = value
            .get(PATH_KEY)
            .or_else(|| value.get(COMPACT_PATH_KEY))
            .and_then(Value::as_str)?
            .to_string();
        let offset = value
            .get(OFFSET_KEY)
            .or_else(|| value.get(COMPACT_OFFSET_KEY))
            .and_then(value_to_usize)?
            .max(1);
        let limit = value
            .get(LIMIT_KEY)
            .or_else(|| value.get(COMPACT_LIMIT_KEY))
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
            PATH_KEY: self.path,
            OFFSET_KEY: self.offset,
            LIMIT_KEY: self.limit
        })
    }

    pub fn to_compact_value(&self) -> Value {
        json!({
            COMPACT_PATH_KEY: self.path,
            COMPACT_OFFSET_KEY: self.offset,
            COMPACT_LIMIT_KEY: self.limit
        })
    }
}

pub fn read_chunk_progress_from_result(result: &Value) -> Option<(usize, usize)> {
    result
        .get("next_read_args")
        .and_then(ReadChunkContinuationArgs::from_value)
        .map(|next_read_args| (next_read_args.offset, next_read_args.limit))
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
    fn pty_continuation_accepts_compact_form() {
        let parsed = PtyContinuationArgs::from_value(&json!({
            "s": "run-123"
        }))
        .unwrap();

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
    fn read_chunk_continuation_accepts_compact_form() {
        let parsed = ReadChunkContinuationArgs::from_value(&json!({
            "p": "out.txt",
            "o": 2,
            "l": 3
        }))
        .unwrap();

        assert_eq!(parsed.path, "out.txt");
        assert_eq!(parsed.offset, 2);
        assert_eq!(parsed.limit, 3);
    }

    #[test]
    fn read_chunk_progress_reads_canonical_args() {
        let result = json!({
            "next_read_args": {
                "path": "out.txt",
                "offset": 81,
                "limit": 40
            }
        });
        assert_eq!(read_chunk_progress_from_result(&result), Some((81, 40)));
    }

    #[test]
    fn read_chunk_progress_reads_compact_args() {
        let result = json!({
            "next_read_args": {
                "p": "out.txt",
                "o": 81,
                "l": 40
            }
        });
        assert_eq!(read_chunk_progress_from_result(&result), Some((81, 40)));
    }

    #[test]
    fn read_chunk_progress_requires_canonical_args() {
        let result = json!({
            "next_offset": "10",
            "chunk_limit": "0"
        });
        assert_eq!(read_chunk_progress_from_result(&result), None);
    }
}
