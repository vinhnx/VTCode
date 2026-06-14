can we wrap the TUI container text for both edges of terminals? It looks weird when the text just abruptly ends at the edge without wrapping. Wrapping would make it look more polished and easier to read, especially for longer lines of text.

some components like quote, table are not wrapped properly and just get cut off at the edge of the terminal. Wrapping would ensure that all content is visible and the UI looks more polished. It would also improve readability, especially for users with smaller terminal windows.

---

audit vtcode-\* crates and check if can merged combines any similiar to reduce reduntdant. don;t need check tests

===

vtcode-llm Extraction -- DONE (partial)

The LLM module was partially extracted into vtcode-llm (~100 files). Integration-point files
(cgp.rs, factory.rs, provider_config.rs, provider_builder.rs, lightweight_routing.rs,
copilot.rs, openresponses/provider.rs) remain in vtcode-core. vtcode-core depends on
vtcode-llm but does not yet consume it; full wiring will happen when CGP integration is decoupled.

--

2. copilot circular dependency -- llm/providers/copilot.rs imports 10+ types from crate::copilot::\*, and copilot imports back from llm. Fix: keep Copilot provider in vtcode-core, extract everything else.
3. open_responses circular dependency -- same pattern as copilot. Fix: keep OpenResponses provider in vtcode-core.

---

#: H1
Severity: HIGH
Issue: vtcode-llm is a dead dependency from vtcode-core
Rationale: Intentional partial extraction — vtcode-core will consume
vtcode-llm in a follow-up PR when CGP integration is decoupled
────────────────────────────────────────
#: M1
Severity: MEDIUM
Issue: ProviderConfig struct duplicated across crates
Rationale: Acknowledged in code comment; will merge when vtcode-llm is fully
decoupled
────────────────────────────────────────
#: M2
Severity: MEDIUM
Issue: RetryPolicy duplicated in gemini wire client
Rationale: Local copy is intentionally different (simpler); acceptable
maintenance cost
────────────────────────────────────────
#: M3
Severity: MEDIUM
Issue: ProviderConfig naming confusion (struct vs trait)
Rationale: Different semantic contexts; ProviderConfigData alias available
