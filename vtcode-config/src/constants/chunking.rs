/// Maximum lines before triggering chunking for read_file
pub const MAX_LINES_THRESHOLD: usize = 2_000;

/// Number of lines to read from start of file when chunking
pub const CHUNK_START_LINES: usize = 800;

/// Number of lines to read from end of file when chunking
pub const CHUNK_END_LINES: usize = 800;

/// Maximum content size for write_file before chunking (in bytes)
pub const MAX_WRITE_CONTENT_SIZE: usize = 500_000; // 500KB

/// Chunk size for write operations (in bytes)
pub const WRITE_CHUNK_SIZE: usize = 50_000; // 50KB chunks
