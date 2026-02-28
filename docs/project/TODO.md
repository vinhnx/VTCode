scan to improve panic and exception handling like force unwrap and expect.

---

implement self update logic for vtcode.

---

Perform a comprehensive review and optimization of the vtcode agent harness, prioritizing execution speed, computational efficiency, and token economy. Refactor the tool call architecture to minimize overhead and latency, while implementing robust error handling strategies to significantly reduce the agent's error rate and ensure reliable, effective performance.

---

extract and open source more components from vtcode-core

---

Shell integration: respects $SHELL environment variable and supports the ! prefix for direct shell execution.

---

Keyboard-first navigation: Full UNIX keybinding support (Ctrl+A/E/W/U/K, Alt+arrows), suspend/resume with Ctrl+Z, and a quick help overlay with ?.

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

---

implement realtime status line config, also add /statusline command to show it

```
Configure Status Line
  Select which items to display in the status lin

  Type to search
  >
› [x] current-dir           Current working dire…
  [x] git-branch            Current Git branch (…
  [ ] model-name            Current model name
  [ ] model-with-reasoning  Current model name w…
  [ ] project-root          Project root directo…
  [ ] context-remaining     Percentage of contex…
  [ ] context-used          Percentage of contex…
  [ ] five-hour-limit       Remaining usage on 5…

  ~/project/path · feat/awesome-feature
  Use ↑↓ to navigate, ←→ to move, space to select,
```

---

fix missing sysntax higlight and theme aware diff rendering
/Users/vinhnguyenxuan/Documents/vtcode-resources/diff.png

---

improve shell command pty output ansi styling
/Users/vinhnguyenxuan/Documents/vtcode-resources/command.png

1. dimmed line numbers
2. better handling of line wrapping (currently it breaks the ANSI styling)
3. theme-aware colors (currently it uses hardcoded colors that may not look good in all
4. aligned the command output, currently the first line has padding to the left but the rest of the lines don't, it would look better if all lines are aligned
5. if possible add simple syntax highlighting for code snippets in the command output, for example if the output contains Rust code, it would be great to have basic syntax highlighting for it
6. add an option to toggle the ANSI styling on and off, some users may prefer to see the raw output without any styling
7. unify diff preview edit with command output preview, currently they look quite different but they can share the same styling and rendering logic to make it more consistent. DRY

---

improve readfile code rendering, it should preserve newlines and indentation, and use a monospace font in the UI and code syntax highlighting if possible. This will make it easier to read and understand the code snippets, especially for languages like Rust where indentation is important.

```

• Read file /Users/vinhnguyenxuan/Develope…core/src/tools/tool_intent.rs
└ Offset lines: 200
└ Limit: 30
│
│       None
│       }
│       })
│       }
```

---

Compaction is an important technique for efficiently running agents. As conversations grow lengthy, the context window fills up. Codex uses a compaction strategy: once the conversation exceeds a certain token count, it calls a special Responses API endpoint, which generates a smaller representation of the conversation history. This smaller version replaces the old input and avoids quadratic inference costs.

---

https://newsletter.pragmaticengineer.com/p/how-codex-is-built?publication_id=458709&post_id=188285137&isFreemail=true&r=1nha8&triedRedirect=true

---

How Codex works

The core loop is a state machine, and the agent loop is the core logic in the Codex CLI. This loop orchestrates the interaction between the user, the model, and the tools the model uses. This “agent loop” is something every AI agent uses, not just Codex, and below is how Codex implements it, at a high level:
How Codex works, at a high level

    Prompt assembly: the agent takes user input and prepares the prompt to pass to the model. On top of user input, the prompt includes system instructions (coding standards, rules), a list of available tools (including MCP servers), and the actual input: text, images, files, AGENTS.md contents, and local environment info.

    Inference: the prompt is converted to tokens and fed to the model, which streams back output events: reasoning steps, tool calls, or a response.

    Response:

        Stream the response to the user by showing it on the terminal.

        If the model decides to use a tool, make this tool call: e.g. read a file, run a bash command, write code. If a command fails, the error message goes back to the model, the model attempts to diagnose the issue, and may decide to retry.

    Tool response (optional): if a tool was invoked, return the response to the model. Repeat steps 3 and 4 for as long as more tool calls are needed.

    Assistant message: the “final message” intended for the user which closes one step in the loop. The loop then starts again with a new user message.

---

IMPLEMENT POSTAMBLE CROSSTERM EXIT SUMMARIAZATION

REF:

```
 Total usage est:        5 Premium
 requests
 API time spent:         14m 55s
 Total session time:     1h 13m 43s
 Total code changes:     +137 -13
 Breakdown by AI model:
  gpt-5.3-codex           10.0m in,
  40.1k out, 8.9m cached (Est. 5
  Premium requests)

 Resume this session with copilot
 --resume=ce712ef0-6605-4cd4-82bf-fad39
 bd55bca
```

---

review any duplicated code in the codebase and refactor to remove duplication. For example, the logic for rendering the diff preview and the command output preview can be unified to use the same rendering logic and styling. This will make the codebase cleaner and easier to maintain. Additionally, any common utility functions that are duplicated across different modules can be extracted into a shared utility module. search across modules for similar code patterns and identify opportunities for refactoring to reduce duplication and improve code reuse.

DRY and KISS

---

improve agent's postamlbe summarization. reference and also suggest next actions based on the summary. This will help users quickly understand the outcome of the agent's actions and what they can do next. And for the agent itself.

```
• Implemented. I tightened theme color distribution and readability logic across
  both vtcode-tui and vtcode-core.

  - Added luminance balancing for rendered text/accent styles (foreground,
    primary, secondary, user, response, reasoning, alert) relative to background:
      - vtcode-tui/src/ui/theme.rs:118:118
      - vtcode-core/src/ui/theme.rs:118:118
  - Added explicit luminance adjustment helpers:
      - vtcode-tui/src/ui/theme.rs:1414:1414
      - vtcode-core/src/ui/theme.rs:1373:1373
  - Strengthened ensure_contrast with final black/white high-contrast fallback so
    accessibility floor is always met:
      - vtcode-tui/src/ui/theme.rs:1450:1450
      - vtcode-core/src/ui/theme.rs:1409:1409
  - Added full-theme regression tests to enforce contrast + readability bounds
    across all registered themes:
      - vtcode-tui/src/ui/theme.rs:1781:1781
      - vtcode-core/src/ui/theme.rs:1726:1726

  Validation run:
  - cargo check -p vtcode-core passed
  - cargo test -p vtcode-tui
    ui::theme::tests::test_all_themes_have_readable_foreground_and_accents --
    --nocapture passed

  Note:

  - I also noticed an unrelated local config change in vtcode.toml (theme changed
    to ciapre). I left it untouched.
    ``
```

the goal is to provide users with a clear and concise summary of the agent's actions, including any relevant metrics such as API usage, time spent, and code changes. Additionally, the summary should suggest next steps for the user to take based on the outcome of the agent's actions. This could include options to resume the session, view detailed logs, or take specific actions based on the results of the agent's work. The postamble summarization should be designed to help users quickly understand the results of their interactions with the agent and guide them towards meaningful next steps. The tone should be informative and actionable, providing users with the information they need to make informed decisions about how to proceed after the agent has completed its tasks. Not too verbose, but comprehensive enough to cover the key outcomes and next steps. Not too blunt, but clear and concise.

---

---

improve file brower selection -> user input, it should show @file alias but full path context for agent. the agent should not confusedd by the @file alias and should be able to understand the full path context. This will help the agent to accurately identify the file and perform the necessary actions on it. Additionally, the UI can display the @file alias for better readability and user experience, while still providing the full path context in the background for the agent's processing.

---

show fatal and errors in tracing session log for
debug purpose. but don't show them in tui transcript.

for example: notification can catch error, fatal, but in the final sessions log, it doesn't record (/Users/vinhnguyenxuan/.vtcode/sessions/session-vtcode-20260228T141549Z_228471-31761.json) any error or fatal, which makes it hard to debug when something goes wrong. We should make sure that any errors or fatals that occur during the session are properly logged in the session log for debugging purposes, even if they are not shown in the TUI transcript. This way, we can have a complete record of what happened during the session and can investigate any issues that arise more effectively.

---

add syntect syntax CODE highlighting to read file preview tool
'/Users/vinhnguyenxuan/Desktop/Screenshot 2026-02-28 at 9.49.53 PM.png'

---

Conduct a comprehensive review and enhancement of error handling and recovery mechanisms within the agent loop, with particular emphasis on tool call operations. Implement a multi-layered error handling strategy that includes retry logic with exponential backoff for transient failures such as network timeouts, rate limiting, and temporary service unavailability while implementing fail-fast behavior for non-recoverable errors including authentication failures, invalid parameters, and permission denied scenarios. Develop and integrate a robust state management system that ensures the agent can maintain consistent internal state during and after error occurrences, including proper rollback mechanisms for partial operations and transaction-like semantics where appropriate. Create a comprehensive error categorization system that distinguishes between retryable and non-retryable errors and implements appropriate handling strategies for each category. Enhance user-facing error messages to be clear, actionable, and informative while avoiding technical jargon that may confuse end users. Implement proper logging at multiple levels including debug, info, warning, and error levels to facilitate troubleshooting and monitoring. Conduct a thorough audit of existing error handling implementations to identify gaps, inconsistencies, and potential failure points. Refactor the error handling code to improve modularity, testability, and maintainability while ensuring comprehensive test coverage for error scenarios including edge cases and unexpected inputs. Add appropriate circuit breaker patterns for external service calls to prevent cascading failures and enable graceful degradation when dependent services are unavailable. Implement proper resource cleanup and resource leak prevention throughout the agent loop.

---

review and improve error handling and recovery in the agent loop, especially around tool calls. The goal is to make the agent more robust and resilient to errors, so that it can recover gracefully from failures and continue to operate effectively. This may involve implementing retry logic for certain types of errors, providing more informative error messages to the user, and ensuring that the agent can maintain a consistent state even when errors occur. Additionally, we should review the existing error handling code to identify any gaps or weaknesses, and refactor it as needed to improve its effectiveness.
Findings

- High: A successful direct command (run ... / !cmd)
  still triggered a fresh LLM turn, so network/
  provider flakiness could fake a task failure
  afterward. Fixed in tool_dispatch.rs:117 and gate
  in session_loop_runner.rs:320.
- High: Post-tool LLM failures still aborted turns
  in multiple stages. Now handled centrally with one
  recovery helper in turn_loop.rs:195, applied at
  request/parse/result-handler points.
- Medium: Earlier recovery logic was too coupled
  (extra state checks + history mutation).
  Simplified to one condition: “tool response exists
  I found the core inconsistency causing the unstable
  state: shared token-count APIs were partially
  removed but still referenced across providers and
  runloop. I’m restoring those APIs in the canonical
  trait/common modules first, then re-checking end-to-
  end.

• I see two places to simplify further without losing
behavior: remove unnecessary history mutation in the
recovery helper, and inline the empty-input turn
gate (drop the extra helper/tests). I’m applying
those now.
