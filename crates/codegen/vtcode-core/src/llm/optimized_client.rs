//! Optimized LLM client with connection pooling and request batching.
//!
//! Re-exported from `vtcode_llm` to eliminate duplication.

pub use vtcode_llm::optimized_client::{
    BatchedRequest, ConnectionPool, OptimizedLLMClient, OptimizedRequest, OptimizedResponse,
    RateLimiter, RequestBatcher,
};
