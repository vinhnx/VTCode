fix techdebt

- Merging duplicated LLM provider stacks between vtcode-core and vtcode-llm.
- Splitting very large modules (structural_search.rs, subagents/mod.rs).
- Converting stringly-typed IDs (PluginId, MarketplaceId, etc.) to newtypes.
- Systematically removing the remaining ~8,128 unwrap/expect/panic sites.
- Enabling #![warn(missing_docs)] project-wide.

--

- H3 structural_search.rs:25 — 1600-char FAQ baked into every error message. Single call site (format_ast_grep_failure), so the lazy-map approach would not reduce token cost.
- H4 labels.rs — 28-arm match with repeated Cow::Borrowed(...) literals. Low impact, requires adding phf dependency.
- M2/M3 — switch_to_tool_free_recovery state machine and the 3 near-identical post-tool error-recovery arms in turn_loop.rs. Refactoring is high-risk relative to impact.

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/plans/future-out-of-scope-fixes.md
