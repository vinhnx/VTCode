use std::io;
use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// Result of a bounded line read.
#[derive(Debug)]
pub enum ReadLineResult {
    Line(Vec<u8>),
    Truncated(Vec<u8>),
    Eof,
}

/// Read a line with a size limit, preventing unbounded memory growth.
pub(crate) async fn read_line_with_limit<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max_len: usize,
) -> io::Result<ReadLineResult> {
    buf.clear();

    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            // EOF with a partial line left in the buffer: report it as a
            // (truncated, if it overran) line rather than silently dropping
            // the data we already read.
            return Ok(if buf.is_empty() {
                ReadLineResult::Eof
            } else if buf.len() <= max_len {
                ReadLineResult::Line(std::mem::take(buf))
            } else {
                ReadLineResult::Truncated(std::mem::take(buf))
            });
        }

        if let Some(pos) = memchr::memchr(b'\n', available) {
            // Found a newline within the available buffer
            let to_read = pos + 1;
            let would_be_total = buf.len() + to_read;

            if would_be_total <= max_len {
                // Line fits within the limit
                debug_assert!(to_read <= available.len());
                #[expect(
                    clippy::indexing_slicing,
                    reason = "to_read is a memchr hit plus one, so it never exceeds available.len()"
                )]
                let chunk = &available[..to_read];
                buf.extend_from_slice(chunk);
                reader.consume(to_read);
                return Ok(ReadLineResult::Line(std::mem::take(buf)));
            } else {
                // Line would exceed the limit: fill the remaining space, then
                // report truncation. Consume the whole line from the reader so
                // the next read starts at a fresh line.
                let remaining_space = max_len.saturating_sub(buf.len());
                if remaining_space > 0 {
                    debug_assert!(remaining_space <= available.len());
                    #[expect(
                        clippy::indexing_slicing,
                        reason = "would_be_total > max_len implies remaining_space < to_read <= available.len()"
                    )]
                    let chunk = &available[..remaining_space];
                    buf.extend_from_slice(chunk);
                }
                reader.consume(to_read);
                return Ok(ReadLineResult::Truncated(std::mem::take(buf)));
            }
        }

        // No newline found in current buffer, add what we can
        let len = available.len();
        let would_be_total = buf.len() + len;

        if would_be_total <= max_len {
            // Buffer content fits within the limit
            buf.extend_from_slice(available);
            reader.consume(len);
        } else {
            // Would exceed the limit, add only what fits and report truncation
            // immediately: continuing would either grow the buffer past
            // `max_len` or block forever on a stream that never sends `\n`.
            let remaining_space = max_len.saturating_sub(buf.len());
            if remaining_space > 0 {
                debug_assert!(remaining_space <= available.len());
                #[expect(
                    clippy::indexing_slicing,
                    reason = "would_be_total > max_len implies remaining_space < available.len()"
                )]
                let chunk = &available[..remaining_space];
                buf.extend_from_slice(chunk);
            }
            reader.consume(len);
            return Ok(ReadLineResult::Truncated(std::mem::take(buf)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{ReadLineResult, read_line_with_limit};
    use tokio::io::BufReader;

    #[tokio::test]
    async fn read_line_with_limit_truncates() -> std::io::Result<()> {
        let data = "hello world\n";
        let mut reader = BufReader::new(data.as_bytes());
        let mut buf = Vec::new();

        let result = read_line_with_limit(&mut reader, &mut buf, 5).await?;
        match result {
            ReadLineResult::Truncated(bytes) => {
                assert!(!bytes.is_empty());
            }
            other => panic!("expected truncation, got {other:?}"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn read_line_with_limit_no_newline_stream_returns_truncated() -> std::io::Result<()> {
        // A stream with no '\n' must terminate with Truncated once the limit is
        // reached instead of growing the buffer or blocking forever.
        let data = vec![b'a'; 10_000];
        let mut reader = BufReader::new(data.as_slice());
        let mut buf = Vec::new();

        let result = read_line_with_limit(&mut reader, &mut buf, 100).await?;
        match result {
            ReadLineResult::Truncated(bytes) => {
                assert_eq!(bytes.len(), 100, "buffer must not exceed the limit");
            }
            other => panic!("expected truncation, got {other:?}"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn read_line_with_limit_reports_partial_line_at_eof() -> std::io::Result<()> {
        // Data without a trailing newline must be surfaced as a line rather
        // than silently dropped.
        let data = "no newline at end";
        let mut reader = BufReader::new(data.as_bytes());
        let mut buf = Vec::new();

        let result = read_line_with_limit(&mut reader, &mut buf, 100).await?;
        match result {
            ReadLineResult::Line(bytes) => {
                assert_eq!(bytes, b"no newline at end");
            }
            other => panic!("expected line, got {other:?}"),
        }
        Ok(())
    }

    #[tokio::test]
    async fn read_line_with_limit_treats_exact_fit_as_complete_line() -> std::io::Result<()> {
        // `would_be_total == max_len` takes the fits branch; one byte less
        // must truncate instead. Pins both sides of the limit boundary.
        for (max_len, expect_truncated) in [(6, false), (5, true)] {
            let mut reader = BufReader::new(b"hello\n".as_slice());
            let mut buf = Vec::new();
            match read_line_with_limit(&mut reader, &mut buf, max_len).await? {
                ReadLineResult::Line(bytes) => {
                    assert!(!expect_truncated, "max_len={max_len} must fit");
                    assert_eq!(bytes, b"hello\n");
                }
                ReadLineResult::Truncated(bytes) => {
                    assert!(expect_truncated, "max_len={max_len} must truncate");
                    assert_eq!(bytes.len(), max_len);
                }
                ReadLineResult::Eof => panic!("max_len={max_len} unexpectedly hit EOF"),
            }
        }
        Ok(())
    }
}
