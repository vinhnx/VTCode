scan to improve panic and exception handling like force unwrap and expect.

---

implement self update logic for vtcode.

---

Perform a comprehensive review and optimization of the vtcode agent harness, prioritizing execution speed, computational efficiency, and token economy. Refactor the tool call architecture to minimize overhead and latency, while implementing robust error handling strategies to significantly reduce the agent's error rate and ensure reliable, effective performance.

---

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/docs/BLOATY_ANALYSIS.md

---

extract tui modules to separate files for better organization and maintainability. Each module can have its own file with clear naming conventions, making it easier to navigate and manage the codebase as the project grows. and also to reduce the size of the main tui file, improving readability and reducing cognitive load when working on specific features or components.

also consider open sourcing the tui modules as a standalone library for other projects to use, which would require further refactoring and documentation to ensure it is reusable and adaptable to different contexts.

---

---

Shell integration: respects $SHELL environment variable and supports the ! prefix for direct shell execution.

---

Keyboard-first navigation: Full UNIX keybinding support (Ctrl+A/E/W/U/K, Alt+arrows), suspend/resume with Ctrl+Z, and a quick help overlay with ?.

---

Ctrl+X, Ctrl+E, Ctrl+G: Open your preferred terminal editor for composing longer prompts.

---

Accessibility: Screen reader mode, configurable reasoning visibility, and responsive layout for narrow terminals.

---

https://huggingface.co/Qwen/Qwen3.5-397B-A17B?inference_api=true&inference_provider=together&language=sh&client=curl

---

add syntax highlight for bash command in tui, to improve readability and user experience when executing shell commands through the terminal interface. This could involve integrating a syntax highlighting library that supports bash syntax, allowing users to easily distinguish between different components of the command, such as keywords, variables, and strings, enhancing clarity and reducing the likelihood of errors when composing or reviewing commands in the terminal UI.

---

check edit file patch preview to show condense info with git changes lines + small offset only, don't show full file

reference:

```
• Edited vtcode-config/src/loader/config.rs (+2 -2)
    536  # Suppress notifications while terminal is focused
    537 -suppress_when_focused = false
    537 +suppress_when_focused = true
    538
        ⋮
    545  # Success notifications for tool call results
    546 -tool_success = true
    546 +tool_success = false
    547
```

reference currently PTY's truncated file preview logic

---

idea: add timer for task / turns

```
─ Worked for 1m 30s ─────────────────────────────────────────────────────────────────────────────────────
```

```
• Running targeted clippy checks (2m 47s • esc to interrupt) · 1 background terminal running · /ps to vie
```

---

add context calculation logic and display at bottom status bar, showing how many tokens are currently in context and how many are left before reaching the model's limit. This would help users manage their prompts and tool calls more effectively, ensuring they stay within the token limits of the LLM and avoid unexpected truncation or errors due to exceeding context size.

suggested display format:

```

> Continue, or @files, /commands, Shift+Tab: cycle modes, Control+C: cancel task, tab: queue messages

Ghostty main*                                                              gemini-3-flash-preview | (low) | 17% context left
```

---

# Unify ModelId/Provider on vtcode-config and add hard regression gates

## Summary

Unify vtcode-core to use vtcode-config’s ModelId, Provider, and ModelParseError as the single source
of truth, while preserving current vtcode-core runtime behavior. Remove duplicated model/provider
definitions from vtcode-core, port core-only helper APIs into vtcode-config, and add CI-failing tests
to prevent future drift/regressions.

## Implementation Plan

1. Baseline and lock current behavior in tests.

- Add/adjust tests that explicitly cover current vtcode-core defaults and helper behavior (especially
  provider defaults and reasoning/tool helper methods).
- Add explicit tests for known drifted models: MoonshotMinimaxM25, MoonshotQwen3CoderNext,
  OpenRouterMinimaxM25, and GPT53Codex.

2. Port core-only ModelId APIs into vtcode-config.

- Add to vtcode-config/src/models/model_id/\*:
    - default_model()
    - non_reasoning_variant()
    - is_gpt51_variant()
    - supports_apply_patch_tool()
    - supports_shell_tool()
- Keep method signatures compatible with current vtcode-core callers.
- Preserve existing OpenRouter metadata behavior and docsrs guards.

3. Port core-only Provider APIs into vtcode-config.

- Add to vtcode-config/src/models/provider.rs:
    - is_dynamic()
    - is_local()
    - local_install_instructions()
- Preserve current vtcode-core semantics for these methods.

4. Align vtcode-config behavior to current vtcode-core behavior where they differ.

- Update defaults/order/selection logic in vtcode-config to match current vtcode-core runtime
  behavior (per your decision).
- Keep bug-fix improvements from config superset (Moonshot/OpenRouter coverage), not old drifted
  omissions.

5. Rewire vtcode-core config model module to re-export shared types.

- In vtcode-core/src/config/models.rs, replace local model/provider/error module wiring with re-
  exports from vtcode_config::models.
- Keep public path stability (crate::config::models::{ModelId, Provider, ModelParseError} remains
  valid).

6. Remove now-redundant duplicated files from vtcode-core.

- Delete/retire duplicated enum/parser/provider/catalog/capability modules no longer needed after re-
  export.
- Keep only genuinely core-specific logic that cannot live in vtcode-config (if any remains after
  step 2/3).

7. Add hard CI gates for regression prevention.

- Add a vtcode-core test proving shared-type identity via behavior (parse/format/default/provider
  (as_str, from_str, provider, defaults, capability helpers).

8. Validate end-to-end.
    - cargo clippy

- vtcode-core::config::models::Provider becomes a re-export of vtcode_config::models::Provider.
- vtcode-core::config::models::ModelParseError becomes a re-export of
  vtcode_config::models::ModelParseError.
- vtcode-config gains additional helper methods previously only present in vtcode-core (ModelId and
  Provider methods listed above).

## Test Cases and Scenarios

- Parsing roundtrip:
    - GPT53Codex, Moonshot models, OpenRouter minimax/minimax-m2.5.
- Provider mapping:
    - Every newly covered model returns expected provider.
- Defaults invariants:
    - default_orchestrator_for_provider, default_subagent_for_provider, default_single_for_provider
      match preserved vtcode-core behavior.
- Capability invariants:
    - supports_reasoning_effort, supports_tool_calls, non_reasoning_variant,
      supports_apply_patch_tool, supports_shell_tool.
- Re-export stability:
    - Existing vtcode-core call sites compile without local enum/provider definitions.

## Assumptions and Chosen Defaults

- Chosen scope: full unification (single source of truth in vtcode-config).
- Guardrail strictness: hard CI gate (failing tests on regressions).
- Canonical behavior policy: preserve current vtcode-core runtime behavior, and align vtcode-config
  accordingly.
- Drift bug fix is included: missing Moonshot/OpenRouter variants in core behavior are treated as
  defects and corrected via unification.
