https://huggingface.co/docs/hub/agents-overview#register-your-agent-harness

===

implement automatically download any new updates when vtcode starts up.

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
