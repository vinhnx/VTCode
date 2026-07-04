https://huggingface.co/docs/hub/agents-overview#register-your-agent-harness

---

check and improve the first time lauch onboarding, improve the error message for guidance settings API to .env or just show a API key secured input box in the onboarding flow. whenever user hit error like this:

```

  ////////////////////////////////////////////////////// Error //////////////////////////////////////////////////////
    LLM request failed: Unauthorized
  ///////////////////////////////////////////////////////////////////////////////////////////////////////////////////
  ------------------------------------------------------ Info -------------------------------------------------------
    Hint: Verify your API key or credentials; Check that your account is active and has sufficient permissions;
    Ensure environment variables for API keys are set correctly
  -------------------------------------------------------------------------------------------------------------------
```

check existing /model selecting flow and improve it to be more user friendly and easier to setup API keys and model selection. Consider adding a guided setup wizard that walks users through the process of entering their API keys, selecting models, and configuring settings. Provide clear instructions and tooltips to help users understand each step.

---

Remaining Technical Debt / Next Steps:

Move tool-schema validation out of parse_structured.rs so the parser does not know which parameters are required for which tool.
Consider a ParseResult enum instead of Option so rejection reasons are first-class.
Optimize strip_textual_tool_call_regions further by having parsers report consumed spans directly, eliminating the per-region parse validation loop.

---

Notes for the team

- Test-harness gap recorded in memory: cargo test -p vtcode --lib runs 0 tests because src/lib.rs has no mod agent. Use cargo test -p vtcode --bin vtcode to run the ~1848 inline agent tests.
- 8 pre-existing baseline failures remain open (em-dash — vs -- in canonical guidance strings, [Session Memory Envelope] missing, reused_recent_result not appearing). These are unrelated to this fix and should be addressed separately.
- Findings recorded in .vtcode/memory/gotchas.md and issues.md for cross-session continuity.
