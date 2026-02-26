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

I see the problem now. The codebase has two separate ModelId enums:

     1. `vtcode-config/src/models/model_id.rs` - Used by vtcode-config crate
     2. `vtcode-core/src/config/models/model_id/definition.rs` - Used by
        vtcode-core crate

    These are NOT unified - they're completely separate enums that need to be
    kept in sync manually. This is indeed a design issue, but it's the current
    architecture.

    I've already added GPT53Codex to both enums:
     - ✅ vtcode-config/src/models/model_id.rs (line 41)
     - ✅ vtcode-core/src/config/models/model_id/definition.rs (line 22)

    And all the corresponding implementations in both crates. The build
    succeeded, which means both enums are now in sync.

    Why this architecture exists:
     - vtcode-config handles configuration, constants, and model metadata
     - vtcode-core handles the core agent logic, LLM providers, and runtime
       behavior
     - They're separate crates that can be used independently

    The proper fix would be to have vtcode-core re-export ModelId from
    vtcode-config, but that would be a larger refactoring. For now, both enums
    have been updated with GPT53Codex.

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
