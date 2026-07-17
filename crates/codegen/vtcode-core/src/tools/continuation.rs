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
const OFFSET_BYTES_KEY: &str = "offset_bytes";
const PAGE_SIZE_BYTES_KEY: &str = "page_size_bytes";

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
    pub offset_bytes: Option<u64>,
    pub page_size_bytes: Option<usize>,
}

impl ReadChunkContinuationArgs {
    pub fn new(path: impl Into<String>, offset: usize, limit: usize) -> Self {
        Self {
            path: path.into(),
            offset: offset.max(1),
            limit: limit.max(1),
            offset_bytes: None,
            page_size_bytes: None,
        }
    }

    /// Create byte-based continuation args.
    pub fn new_byte_range(
        path: impl Into<String>,
        offset_bytes: u64,
        page_size_bytes: usize,
    ) -> Self {
        Self {
            path: path.into(),
            offset: 1,
            limit: DEFAULT_NEXT_READ_LIMIT,
            offset_bytes: Some(offset_bytes),
            page_size_bytes: Some(page_size_bytes),
        }
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        let path = value
            .get(PATH_KEY)
            .or_else(|| value.get(COMPACT_PATH_KEY))
            .and_then(Value::as_str)?
            .to_string();

        // Byte-based continuation
        let offset_bytes = value.get(OFFSET_BYTES_KEY).and_then(Value::as_u64);
        let page_size_bytes = value.get(PAGE_SIZE_BYTES_KEY).and_then(value_to_usize);

        if offset_bytes.is_some() || page_size_bytes.is_some() {
            return Some(Self {
                path,
                offset: 1,
                limit: DEFAULT_NEXT_READ_LIMIT,
                offset_bytes,
                page_size_bytes,
            });
        }

        // Line-based continuation
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
            offset_bytes: None,
            page_size_bytes: None,
        })
    }

    pub fn to_value(&self) -> Value {
        self.serialize_inner(PATH_KEY, OFFSET_KEY, LIMIT_KEY)
    }

    pub fn to_compact_value(&self) -> Value {
        self.serialize_inner(COMPACT_PATH_KEY, COMPACT_OFFSET_KEY, COMPACT_LIMIT_KEY)
    }

    fn serialize_inner(&self, path_key: &str, offset_key: &str, limit_key: &str) -> Value {
        if self.offset_bytes.is_some() || self.page_size_bytes.is_some() {
            let mut map = json!({ path_key: self.path });
            if let Some(ob) = self.offset_bytes {
                map[OFFSET_BYTES_KEY] = json!(ob);
            }
            if let Some(ps) = self.page_size_bytes {
                map[PAGE_SIZE_BYTES_KEY] = json!(ps);
            }
            return map;
        }
        json!({
            path_key: self.path,
            offset_key: self.offset,
            limit_key: self.limit
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

    #[test]
    fn byte_range_continuation_round_trips() {
        let args = ReadChunkContinuationArgs::new_byte_range("big.bin", 8192, 4096);
        let payload = args.to_value();
        let parsed = ReadChunkContinuationArgs::from_value(&payload).unwrap();

        assert_eq!(parsed.path, "big.bin");
        assert_eq!(parsed.offset_bytes, Some(8192));
        assert_eq!(parsed.page_size_bytes, Some(4096));
    }

    #[test]
    fn byte_range_continuation_from_value() {
        let parsed = ReadChunkContinuationArgs::from_value(&json!({
            "path": "data.log",
            "offset_bytes": 1024,
            "page_size_bytes": 2048
        }))
        .unwrap();

        assert_eq!(parsed.path, "data.log");
        assert_eq!(parsed.offset_bytes, Some(1024));
        assert_eq!(parsed.page_size_bytes, Some(2048));
    }

    #[test]
    fn byte_range_continuation_to_value_includes_byte_fields() {
        let args = ReadChunkContinuationArgs::new_byte_range("out.bin", 0, 8192);
        let value = args.to_value();

        assert_eq!(value["path"], "out.bin");
        assert_eq!(value["offset_bytes"], 0);
        assert_eq!(value["page_size_bytes"], 8192);
        // Line-based fields should not be present
        assert!(value.get("offset").is_none());
        assert!(value.get("limit").is_none());
    }

    #[test]
    fn line_based_continuation_excludes_byte_fields() {
        let args = ReadChunkContinuationArgs::new("out.txt", 41, 40);
        let value = args.to_value();

        assert_eq!(value["path"], "out.txt");
        assert_eq!(value["offset"], 41);
        assert_eq!(value["limit"], 40);
        assert!(value.get("offset_bytes").is_none());
        assert!(value.get("page_size_bytes").is_none());
    }
}
