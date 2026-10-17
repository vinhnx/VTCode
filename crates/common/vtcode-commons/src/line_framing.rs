//! Bounded byte framing for newline-delimited subprocess streams.

use tokio::io::{AsyncBufRead, AsyncBufReadExt};

/// Whether LF is retained and charged against the byte limit. CR is always content.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEnding {
    IncludeLf,
    ExcludeLf,
}

/// Read and drain one physical line, retaining at most `max_bytes` in `line`.
///
/// Clears and reuses the caller's buffer. Returns `None` only at EOF without a
/// line, and `Some(truncated)` for a complete or unterminated final line. Even
/// with a zero-byte cap, a physical line is returned and drained. Oversized
/// content is discarded through LF (or EOF), preserving the next frame boundary.
/// This is byte framing: retained data need not end at a UTF-8 character boundary.
///
/// # Errors
/// Returns the underlying read error. After error or cancellation, callers must
/// not assume the stream is aligned at a new line; this operation is not
/// cancellation-safe and transports should terminate the reader on failure.
pub async fn read_bounded_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    line: &mut Vec<u8>,
    max_bytes: usize,
    ending: LineEnding,
) -> std::io::Result<Option<bool>> {
    line.clear();
    let mut truncated = false;
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if line.is_empty() && !truncated {
                None
            } else {
                Some(truncated)
            });
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let consumed = newline.map_or(available.len(), |position| position + 1);
        let content_len = consumed - usize::from(newline.is_some() && ending == LineEnding::ExcludeLf);
        let copy_len = content_len.min(max_bytes.saturating_sub(line.len()));
        line.extend(available.iter().copied().take(copy_len));
        truncated |= copy_len < content_len;
        reader.consume(consumed);
        if newline.is_some() {
            return Ok(Some(truncated));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::task::{Context, Poll};
    use tokio::io::{AsyncRead, BufReader, ReadBuf};

    #[tokio::test]
    async fn delimiter_policy_preserves_exact_limit_and_crlf() -> std::io::Result<()> {
        for (ending, expected, truncated) in [
            (LineEnding::ExcludeLf, b"abc".as_slice(), false),
            (LineEnding::IncludeLf, b"abc".as_slice(), true),
        ] {
            let mut reader = BufReader::with_capacity(1, b"abc\n\r\nnext".as_slice());
            let mut line = Vec::new();
            assert_eq!(read_bounded_line(&mut reader, &mut line, 3, ending).await?, Some(truncated));
            assert_eq!(line, expected);
            assert_eq!(read_bounded_line(&mut reader, &mut line, 3, ending).await?, Some(false));
            assert_eq!(
                line,
                if ending == LineEnding::ExcludeLf {
                    b"\r".as_slice()
                } else {
                    b"\r\n".as_slice()
                }
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn framing_matches_physical_lines_across_caps_and_chunk_boundaries() -> std::io::Result<()> {
        // Independent oracle splits complete input first; production frames incremental chunks.
        for input in [
            b"".as_slice(),
            b"\n",
            b"abc\nnext\n",
            b"\nabc\r\nz\nlast",
            "đ\n終".as_bytes(),
        ] {
            for ending in [LineEnding::IncludeLf, LineEnding::ExcludeLf] {
                for cap in 0..=8 {
                    for chunk in 1..=9 {
                        let mut reader = BufReader::with_capacity(chunk, input);
                        let mut line = vec![b'!'; 20];
                        for physical_line in input.split_inclusive(|byte| *byte == b'\n') {
                            let content = if ending == LineEnding::ExcludeLf {
                                physical_line.strip_suffix(b"\n").unwrap_or(physical_line)
                            } else {
                                physical_line
                            };
                            assert_eq!(
                                read_bounded_line(&mut reader, &mut line, cap, ending).await?,
                                Some(content.len() > cap)
                            );
                            assert_eq!(
                                line,
                                &content[..content.len().min(cap)],
                                "input={input:?}, ending={ending:?}, cap={cap}, chunk={chunk}"
                            );
                            assert!(line.len() <= cap);
                        }
                        assert_eq!(read_bounded_line(&mut reader, &mut line, cap, ending).await?, None);
                        assert!(line.is_empty());
                    }
                }
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn oversized_unterminated_line_is_drained_and_bounded() -> std::io::Result<()> {
        for ending in [LineEnding::IncludeLf, LineEnding::ExcludeLf] {
            let input = vec![b'x'; 4096];
            let mut reader = BufReader::with_capacity(7, input.as_slice());
            let mut line = Vec::new();
            assert_eq!(read_bounded_line(&mut reader, &mut line, 5, ending).await?, Some(true));
            assert_eq!(line, b"xxxxx");
            assert_eq!(read_bounded_line(&mut reader, &mut line, 5, ending).await?, None);
        }
        Ok(())
    }

    struct FailingReader {
        sent_prefix: bool,
    }

    impl AsyncRead for FailingReader {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _: &mut Context<'_>,
            buffer: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            if self.sent_prefix {
                Poll::Ready(Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, "test read failure")))
            } else {
                self.sent_prefix = true;
                buffer.put_slice(b"x");
                Poll::Ready(Ok(()))
            }
        }
    }

    #[tokio::test]
    async fn read_errors_propagate_after_a_partial_line() {
        for ending in [LineEnding::IncludeLf, LineEnding::ExcludeLf] {
            let mut reader = BufReader::new(FailingReader { sent_prefix: false });
            let mut line = Vec::new();
            let error = read_bounded_line(&mut reader, &mut line, 3, ending).await.unwrap_err();
            assert_eq!(error.kind(), std::io::ErrorKind::BrokenPipe);
            assert_eq!(error.to_string(), "test read failure");
            assert_eq!(line, b"x");
        }
    }
}
