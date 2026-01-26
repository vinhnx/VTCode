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
pub async fn read_line_with_limit<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    max_len: usize,
) -> io::Result<ReadLineResult> {
    buf.clear();
    let mut total_read = 0;

    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(ReadLineResult::Eof);
        }

        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            // Found a newline within the available buffer
            let to_read = pos + 1;
            let would_be_total = total_read + to_read;

            if would_be_total <= max_len {
                // Line fits within the limit
                buf.extend_from_slice(&available[..to_read]);
                reader.consume(to_read);
                return Ok(ReadLineResult::Line(buf.clone()));
            } else {
                // Line would exceed the limit, so we truncate
                let remaining_space = max_len.saturating_sub(total_read);
                if remaining_space > 0 {
                    buf.extend_from_slice(&available[..remaining_space]);
                }
                reader.consume(to_read); // Still consume the whole line from the reader
                return Ok(ReadLineResult::Truncated(buf.clone()));
            }
        }

        // No newline found in current buffer, add what we can
        let len = available.len();
        let would_be_total = total_read + len;

        if would_be_total <= max_len {
            // Buffer content fits within the limit
            buf.extend_from_slice(available);
            total_read = would_be_total;
            reader.consume(len);
        } else {
            // Would exceed the limit, add only what fits
            let remaining_space = max_len.saturating_sub(total_read);
            if remaining_space > 0 {
                buf.extend_from_slice(&available[..remaining_space]);
            }
            reader.consume(len);
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
}
