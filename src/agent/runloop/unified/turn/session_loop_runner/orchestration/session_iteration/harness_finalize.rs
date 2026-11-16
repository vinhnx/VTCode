//! Shared harness-error finalization for one session iteration.
//!
//! [`finalize_harness_on_err`] replaces five identical function-local
//! `harness_try!` macros (plus one inlined copy in `outcome.rs`). A plain
//! async helper keeps the early-exit contract explicit: no hidden control
//! flow and no ambient `harness_emitter` capture.

use crate::agent::runloop::unified::inline_events::harness::HarnessEventEmitter;

/// Pass through `result`, finalizing canonical harness persistence on error.
///
/// On `Err`, emits the unexpected-exit terminal events through
/// [`HarnessEventEmitter::finish_after_unexpected_exit`] (when an emitter is
/// present) and returns the converted [`anyhow::Error`]. Call sites append
/// `?` so the error still returns early, exactly like the macro this
/// replaces:
///
/// ```rust,ignore
/// finalize_harness_on_err(harness_emitter.as_ref(), renderer.line(style, text)).await?;
/// ```
pub(crate) async fn finalize_harness_on_err<T, E>(
    harness_emitter: Option<&HarnessEventEmitter>,
    result: Result<T, E>,
) -> Result<T, anyhow::Error>
where
    E: Into<anyhow::Error>,
{
    match result {
        Ok(value) => Ok(value),
        Err(error) => {
            let error: anyhow::Error = error.into();
            if let Some(emitter) = harness_emitter {
                emitter.finish_after_unexpected_exit().await;
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Error as IoError;

    #[tokio::test]
    async fn ok_passes_value_through_without_emitter() {
        let value = finalize_harness_on_err(None, Ok::<u32, anyhow::Error>(7)).await.unwrap();
        assert_eq!(value, 7);
    }

    #[tokio::test]
    async fn io_err_without_emitter_preserves_message() {
        let err = finalize_harness_on_err(None, Err::<u32, _>(IoError::other("disk gone")))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("disk gone"), "{err:?}");
    }

    #[tokio::test]
    async fn ok_passes_value_through_with_emitter() {
        let dir = tempfile::tempdir().unwrap();
        let emitter = HarnessEventEmitter::new(dir.path().join("events.jsonl")).unwrap();
        let value = finalize_harness_on_err(Some(&emitter), Ok::<u32, anyhow::Error>(7))
            .await
            .unwrap();
        assert_eq!(value, 7);
    }

    #[tokio::test]
    async fn anyhow_err_with_emitter_preserves_message() {
        let dir = tempfile::tempdir().unwrap();
        let emitter = HarnessEventEmitter::new(dir.path().join("events.jsonl")).unwrap();
        let err = finalize_harness_on_err(Some(&emitter), Err::<u32, _>(anyhow::anyhow!("renderer offline")))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("renderer offline"), "{err:?}");
    }
}
