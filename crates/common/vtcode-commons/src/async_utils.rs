//! Async utility functions

use anyhow::{Context, Result};
use std::future::Future;
use std::time::Duration;
use tokio::io::AsyncReadExt;
use tokio::time::timeout;

pub const DEFAULT_ASYNC_TIMEOUT: Duration = Duration::from_secs(30);
pub const SHORT_ASYNC_TIMEOUT: Duration = Duration::from_secs(5);
pub const LONG_ASYNC_TIMEOUT: Duration = Duration::from_secs(300);

/// Execute a future with a timeout and context
pub async fn with_timeout<F, T>(fut: F, duration: Duration, context: &str) -> Result<T>
where
    F: Future<Output = T>,
{
    match timeout(duration, fut).await {
        Ok(result) => Ok(result),
        Err(_) => anyhow::bail!("Operation timed out after {duration:?}: {context}"),
    }
}

/// Execute a future with the default timeout
pub async fn with_default_timeout<F, T>(fut: F, context: &str) -> Result<T>
where
    F: Future<Output = T>,
{
    with_timeout(fut, DEFAULT_ASYNC_TIMEOUT, context).await
}

/// Execute a future with a short timeout
pub async fn with_short_timeout<F, T>(fut: F, context: &str) -> Result<T>
where
    F: Future<Output = T>,
{
    with_timeout(fut, SHORT_ASYNC_TIMEOUT, context).await
}

/// Execute a future with a long timeout
pub async fn with_long_timeout<F, T>(fut: F, context: &str) -> Result<T>
where
    F: Future<Output = T>,
{
    with_timeout(fut, LONG_ASYNC_TIMEOUT, context).await
}

/// Retry an operation with exponential backoff
pub async fn retry_with_backoff<F, Fut, T>(
    mut op: F,
    max_retries: usize,
    initial_delay: Duration,
    context: &str,
) -> Result<T>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T>>,
{
    let mut delay = initial_delay;
    let mut last_error = None;

    for i in 0..=max_retries {
        match op().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                last_error = Some(e);
                if i < max_retries {
                    tokio::time::sleep(delay).await;
                    delay *= 2;
                }
            }
        }
    }

    let err = last_error.unwrap_or_else(|| anyhow::anyhow!("Retry failed without error"));
    Err(err).with_context(|| format!("Operation failed after {max_retries} retries: {context}"))
}

/// Sleep with context
pub async fn sleep_with_context(duration: Duration, _context: &str) {
    tokio::time::sleep(duration).await;
}

/// Run multiple futures and wait for all with a timeout
pub async fn join_all_with_timeout<F, T>(
    futs: Vec<F>,
    duration: Duration,
    context: &str,
) -> Result<Vec<T>>
where
    F: Future<Output = T>,
{
    with_timeout(futures::future::join_all(futs), duration, context).await
}

/// Read exactly `len` bytes from an async reader without zero-initializing
/// the buffer first.
///
/// This avoids the double-write overhead of `vec![0u8; len]` followed by
/// `read_exact` — `read_buf` appends directly into the `Vec`'s spare capacity,
/// so the zeroing pass is skipped. For large payloads this can yield
/// measurable performance gains.
///
/// The returned `Vec` has exactly `len` initialized bytes.
///
/// This used to be an `unsafe` function that handed `read_exact` a
/// `&mut [u8]` over uninitialized memory via `from_raw_parts_mut`. That was
/// unsound: `tokio::io::ReadBuf::new(&mut [u8])` asserts the whole buffer is
/// initialized, so a reader conforming to the `ReadBuf` contract would be
/// entitled to read the (uninitialized) `[filled, initialized)` region. The
/// `read_buf`-based implementation below is fully safe and keeps the same
/// zero-overhead property, miri-clean by construction.
///
/// # Errors
///
/// Returns `io::ErrorKind::UnexpectedEof` if the reader reaches EOF before
/// `len` bytes have been read.
pub async fn read_exact_uninit<R>(reader: &mut R, len: usize) -> std::io::Result<Vec<u8>>
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut buf = Vec::with_capacity(len);
    // `read_buf` fills the Vec's spare capacity without zero-initializing it,
    // and is sound with respect to uninitialized memory by construction.
    while buf.len() < len {
        let n = reader.read_buf(&mut buf).await?;
        if n == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                format!(
                    "unexpected EOF before reading {len} bytes (got {})",
                    buf.len()
                ),
            ));
        }
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_exact_uninit_round_trips_known_payload() {
        let payload: Vec<u8> = (0..64u8).collect();
        let mut reader = std::io::Cursor::new(payload.clone());
        let got = read_exact_uninit(&mut reader, payload.len())
            .await
            .expect("read full payload");
        assert_eq!(got, payload);
    }

    #[tokio::test]
    async fn read_exact_uninit_reads_across_multiple_poll_reads() {
        // A payload larger than a single `read_buf` is likely to require
        // several poll_read calls; the loop must still accumulate correctly.
        let payload: Vec<u8> = (0..2000u32).map(|i| (i % 256) as u8).collect();
        let mut reader = std::io::Cursor::new(payload.clone());
        let got = read_exact_uninit(&mut reader, payload.len())
            .await
            .expect("read full payload");
        assert_eq!(got, payload);
    }

    #[tokio::test]
    async fn read_exact_uninit_returns_unexpected_eof_on_short_read() {
        let payload = b"only ten!".to_vec();
        let mut reader = std::io::Cursor::new(payload);
        let err = read_exact_uninit(&mut reader, 64)
            .await
            .expect_err("short read must error");
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn read_exact_uninit_returns_unexpected_eof_on_empty_reader() {
        let mut reader = std::io::Cursor::new(Vec::<u8>::new());
        let err = read_exact_uninit(&mut reader, 1)
            .await
            .expect_err("empty reader must error");
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn read_exact_uninit_zero_len_returns_empty_vec() {
        let mut reader = std::io::Cursor::new(Vec::<u8>::new());
        let got = read_exact_uninit(&mut reader, 0)
            .await
            .expect("zero-length read must succeed");
        assert!(got.is_empty());
    }
}
