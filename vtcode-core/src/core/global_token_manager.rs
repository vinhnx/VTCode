//! Global token budget manager for cross-component access

use std::sync::Arc;
use tokio::sync::OnceCell;
use crate::core::token_budget::TokenBudgetManager;

static GLOBAL_TOKEN_BUDGET: OnceCell<Arc<TokenBudgetManager>> = OnceCell::const_new();

/// Initialize the global token budget manager
pub async fn init_global_token_budget(config: crate::core::token_budget::TokenBudgetConfig) -> Arc<TokenBudgetManager> {
    GLOBAL_TOKEN_BUDGET
        .get_or_init(|| async {
            Arc::new(TokenBudgetManager::new(config))
        })
        .await
        .clone()
}

/// Get a reference to the global token budget manager
pub fn get_global_token_budget() -> Option<Arc<TokenBudgetManager>> {
    GLOBAL_TOKEN_BUDGET.get().cloned()
}

/// Set a specific token budget manager as the global one (for initialization)
pub async fn set_global_token_budget(token_budget: Arc<TokenBudgetManager>) -> Arc<TokenBudgetManager> {
    GLOBAL_TOKEN_BUDGET
        .set(token_budget)
        .map(|tb| tb.clone())
        .unwrap_or_else(|existing| existing.clone())
}