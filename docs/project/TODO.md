scan to improve panic and exception handling like force unwrap and expect.

---

implement self update logic for vtcode.

---

Perform a comprehensive review and optimization of the vtcode agent harness, prioritizing execution speed, computational efficiency, and token economy. Refactor the tool call architecture to minimize overhead and latency, while implementing robust error handling strategies to significantly reduce the agent's error rate and ensure reliable, effective performance.

---

extract tui modules to separate files for better organization and maintainability. Each module can have its own file with clear naming conventions, making it easier to navigate and manage the codebase as the project grows. and also to reduce the size of the main tui file, improving readability and reducing cognitive load when working on specific features or components.

also consider open sourcing the tui modules as a standalone library for other projects to use, which would require further refactoring and documentation to ensure it is reusable and adaptable to different contexts.

Status: completed (module extraction + standalone `vtcode-tui` options/host adapter API + docs/examples + app/test callsite migration to `SessionOptions` launch path).

check docs/guides/tui-library.md

---

splash command should be in core and not related to tui component

---

tui module should just be plug and play tui rust component for
other agents to use. no concrete business logic

---

› splash command should be generic confirgurable and doesn't hold
concrete logic

---

extract and open source more components from vtcode-core

---

Shell integration: respects $SHELL environment variable and supports the ! prefix for direct shell execution.

---

Keyboard-first navigation: Full UNIX keybinding support (Ctrl+A/E/W/U/K, Alt+arrows), suspend/resume with Ctrl+Z, and a quick help overlay with ?.

---

https://github.com/openai/codex/releases/tag/rust-v0.105.0

---

review potential gap in unified_exec, and vtcode's tools and identify which wastes tokens broadly, and which are more efficient. optimize the inefficient ones, and consider adding new tools that can help reduce token usage in common scenarios.
for example, if there are tools that require multiple calls to achieve a common task, consider consolidating them into a single tool that can perform the entire task in one call, thus reducing the number of interactions and tokens used. Additionally, review the prompts and responses for each tool to ensure they are concise and effective, minimizing unnecessary verbosity that can lead to increased token consumption.
or
• I found a concrete gap affecting non-diff unified_exec: command payloads still
often carry duplicated text (output + stdout), which wastes tokens broadly. I’m
patching normalization and fallback handling so all command-like tool calls
benefit, not only git diff. => should be fixed by the new unified_exec refactor, which normalizes all tool calls to have a consistent structure and eliminates redundant information in the payload, thus optimizing token usage across all tools that utilize the unified_exec interface.

• I identified three additional broad token waste points and I’m patching them
now.

1. Remove duplicated spool guidance reaching the model twice (spool_hint field
   plus extra system nudge).
2. Trim repeated/unused metadata in model-facing tool payloads (spool_hint,
   spooled_bytes, duplicate id==session_id, null working_directory).
3. Shorten high-frequency follow-up prompts for PTY and spool-chunk reads, and
   provide compact structured continuation args for chunked spool reads.

---

check external edtior (control+e) can not use normal mode (escape). This is because the escape key is currently hardcoded to trigger the rewind functionality, which conflicts with its expected behavior in normal mode. To resolve this, we need to implement a more context-aware handling of the escape key, allowing it to function as intended in normal mode while still providing the rewind functionality when appropriate. This may involve introducing a new keybinding for the rewind action or implementing a double-escape mechanism to differentiate between the two use cases. check crossterm event handling for escape key and implement the necessary logic to support both functionalities without conflict. vtcode support external editor config.

expand external editor support to allow users to configure their preferred editor for the control+e shortcut. This would involve adding a configuration option in the settings where users can specify the command for their external editor (e.g., vim, nano, code). The system would then use this command to launch the specified editor when the control+e shortcut is pressed, providing a seamless and customizable editing experience for users who prefer to use an external editor over the built-in one. Also check to support VS Code, Zed, Text Edit, Sublime Text, TextMate, Emacs, and other popular editors, nvim...
