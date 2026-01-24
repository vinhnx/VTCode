use std::io;
use tokio::io::{AsyncBufReadExt, AsyncBufRead};

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
    let mut truncated = false;

    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(ReadLineResult::Eof);
        }

        if let Some(pos) = available.iter().position(|&b| b == b'\n') {
            let to_read = pos + 1;
            if total_read + to_read <= max_len {
                buf.extend_from_slice(&available[..to_read]);
            } else {
                truncated = true;
            }
            reader.consume(to_read);
            return Ok(if truncated {
                ReadLineResult::Truncated(buf.clone())
            } else {
                ReadLineResult::Line(buf.clone())
            });
        }

        let len = available.len();
        if total_read + len <= max_len {
            buf.extend_from_slice(available);
            total_read += len;
        } else {
            truncated = true;
        }
        reader.consume(len);
    }
}

#[cfg(test)]
mod tests {
    use super::{ReadLineResult, read_line_with_limit};
    use tokio::io::BufReader;

    #[tokio::test]
    async fn read_line_with_limit_truncates() {
        let data = "hello world\n";
        let mut reader = BufReader::new(data.as_bytes());
        let mut buf = Vec::new();

        let result = read_line_with_limit(&mut reader, &mut buf, 5).await.unwrap();
        match result {
            ReadLineResult::Truncated(bytes) => {
                assert!(!bytes.is_empty());
            }
            other => panic!("expected truncation, got {:?}", other),
        }
    }
}
