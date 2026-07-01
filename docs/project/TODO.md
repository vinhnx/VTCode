migrate agent-client-protocol from 0.10.4 to 1.0.1 (breaking change) https://agentclientprotocol.com/llms.txt

--

## fix techdebt

- Converting stringly-typed IDs (PluginId, MarketplaceId, etc.) to newtypes.

---

- Systematically removing the remaining ~8,128 unwrap/expect/panic sites.

---

- Enabling #![warn(missing_docs)] project-wide.

--

- H3 structural_search/constants.rs:25 — 1600-char FAQ baked into every error message. Single call site (format_ast_grep_failure in fragment_hints.rs), so the lazy-map approach would not reduce token cost.
- H4 labels.rs — 28-arm match with repeated Cow::Borrowed(...) literals. Low impact, requires adding phf dependency.
- M2/M3 — switch_to_tool_free_recovery state machine and the 3 near-identical post-tool error-recovery arms in turn_loop.rs. Refactoring is high-risk relative to impact.

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/plans/future-out-of-scope-fixes.md

---

implement automatically download any new updates when vtcode starts up.
