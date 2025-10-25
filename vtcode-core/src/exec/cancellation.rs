use tokio_util::sync::CancellationToken;

use std::future::Future;

use tokio::task_local;

task_local! {
    static ACTIVE_TOOL_TOKEN: CancellationToken;
}

/// Run the provided future with the supplied cancellation token made available to tools.
pub async fn with_tool_cancellation<F, T>(token: CancellationToken, fut: F) -> T
where
    F: Future<Output = T>,
{
    ACTIVE_TOOL_TOKEN.scope(token, fut).await
}

/// Retrieve the currently scoped cancellation token, if any.
pub fn current_tool_cancellation() -> Option<CancellationToken> {
    ACTIVE_TOOL_TOKEN.try_with(|token| token.clone()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn scoped_token_is_accessible() {
        assert!(current_tool_cancellation().is_none());
        let token = CancellationToken::new();
        with_tool_cancellation(token.clone(), async move {
            let current = current_tool_cancellation().expect("token should be set");
            assert!(!current.is_cancelled());
            token.cancel();
            assert!(current.is_cancelled());
        })
        .await;
        assert!(current_tool_cancellation().is_none());
    }
}
