https://huggingface.co/docs/hub/agents-overview#register-your-agent-harness

===

implement automatically download any new updates when vtcode starts up.

---

CRITICAL: check session logs:

error:

1. The apply_patch tool is also being routed to unified_search
2. This appears to be a test/sandbox environment where the tools simulate behavior but don't actually write to disk.
3. why defuddle_search while this is file search only?
4. check the logs and find out why the apply_patch tool is being routed to unified_search instead of file search. This may be a misconfiguration or a bug in the routing logic.
5. it's unusable.

/Users/vinhnguyenxuan/Documents/podcast/.vtcode/checkpoints/turn_1.json

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
