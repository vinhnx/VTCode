Perform a detailed audit of the vtcode modes and subagents, focusing specifically on the 'plan', 'build', and 'auto' modes. Identify any existing blockers, technical issues, or workflow inefficiencies. Propose an optimized system architecture and outline a validation process to ensure that each mode functions correctly and achieves its intended objective.

===

Investigate and resolve a regression where the program fails to exit upon receiving repeated Ctrl+C (SIGINT) signals. Implement a robust signal handler that intercepts these interrupts to facilitate a graceful shutdown. The solution should include setting a termination flag to halt ongoing processes and executing all necessary cleanup routines before the program exits, ensuring the agent remains responsive to user interrupt requests and does not become stuck in an indefinite running state.

===

Develop a technical architecture and implementation plan to transform the current provider-specific `/compact` command into a unified, provider-agnostic feature. Currently, compaction is only available for the native OpenAI provider on api.openai.com. The goal is to abstract the compaction logic so that the command functions consistently across all LLM models and backends, including all providers. Your response should include an architectural design for the abstraction layer, strategies for integrating various provider APIs, and a method for ensuring manual user triggers work universally across all environments.

===

Root cause of the critical bug

Both broken call sites used the guarded activate_recovery_with_mode (only acts when recovery_phase == Inactive) at a point where a recovery pass was already in flight (phase == InPass). The sibling paths in apply_recovery_decision correctly used the unconditional set_recovery_mode. The fix makes all mid-pass mode switches use set_recovery_mode, with explanatory comments at both sites.

Recommendation

If crash-resumability during a request_more_resources pause becomes a concern, extract Completed's checkpoint-snapshot logic into a shared helper and call it from both Completed and AwaitingUser — rather than duplicating the block.
