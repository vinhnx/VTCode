//! Async utility functions

use anyhow::{Context, Result};
use std::future::Future;
use std::time::Duration;
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
        Err(_) => anyhow::bail!("Operation timed out after {:?}: {}", duration, context),
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
    Err(err).with_context(|| {
        format!(
            "Operation failed after {} retries: {}",
            max_retries, context
        )
    })
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
