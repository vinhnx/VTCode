#[path = "session_loop_impl.rs"]
mod session_loop_impl;

pub(crate) use session_loop_impl::run_single_agent_loop_unified;
pub(crate) use session_loop_impl::{
    BACKGROUND_COMPLETION_CONTINUATION_PROMPT_PREFIX, VERIFICATION_AUTO_RECOVERY_PREFIX,
    run_single_agent_loop_unified_with_host,
};
