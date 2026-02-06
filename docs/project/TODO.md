[DON'T DELETE UNTIL FEEL COMPLETE] review duplicated and redundent logic from whole code base and remove and cleanup AND DRY.

continue with your recommendation, proceed with outcome. don't stop. review overall progress and changes again carefully, can you do better? go on don't ask me

---

- Large Paste Handling: Improved the readability of chat history by replacing massive text pastes with a clean, toggleable [Pasted Text: X lines] placeholder.

--

implement chat input textview keyboard navigation between block of text, for example, when the user press up/down arrow key, it will move the cursor to the previous/next block of text, this is useful when the user want to edit a specific part of the message and the message is too long that it's hard to navigate with mouse or trackpad. we can define a block of text as a paragraph or a line break, or we can use some heuristic to detect logical blocks of text. this will improve the user experience when editing long messages in the chat input.

--

implement queue messages UI. the part "↳ if user in plan mode, it should automatically switch to agent mode to use full tools in order to
disovery and and then switch to plan make the plan ask user question if needed
⌥ + ↑ edit"

example:

```

• Explored
  └ Read session_setup.rs

• I'm checking session_loop and related code to find where plan_mode_requested is applied since it's not
  set in initialize_session.

• Explored
  └ Search plan_mode in session_loop.rs
    Read session_loop.rs

• Noticing missing active agent update on plan mode init (3m 08s • esc to interrupt)

  ↳ if user in plan mode, it should automatically switch to agent mode to use full tools in order to
    disovery and and then switch to plan make the plan ask user question if needed
    ⌥ + ↑ edit

› Explain this codebase

  ? for shortcuts                                                                      27% context left
```

--

the agent can't use and call `exit_plan_mode` to exit plan mode properly. it's got stuck

"Plan Mode still active. Call `exit_plan_mode` to review the plan or refine the plan before retrying"

---

the final propose plan could reference this

```Proposed Plan


  Title: Inline TUI Large Paste Placeholder (No Toggle)

  Summary

  - Detect large pasted user messages (≥20 lines) and render a
    single placeholder line in the inline TUI transcript: [Pasted
    Text: X lines].
  - Keep the full pasted content for actual submission and
    transcript logs; only the inline TUI rendering is collapsed.
  - No user toggle is provided (per request).

  Key Decisions

  - Threshold: >= 20 lines.
  - Scope: inline TUI only (stdout and transcript logs keep full
    text).
  - Placeholder text: exact format [Pasted Text: X lines].
  - Applies only to user messages (not agent, tool, or PTY output).

  Implementation Details

  1. Add constants
      - Add INLINE_PASTE_PLACEHOLDER_MIN_LINES: usize = 20 and
        INLINE_PASTE_PLACEHOLDER_LABEL: &str = "Pasted Text" in
      - In vtcode-core/src/utils/ansi.rs, inside
        InlineSink::write_multiline, intercept
        InlineMessageKind::User.
      - Compute line count from the original text using
        text.split('\n').count() (counts trailing empty line if
        present).
      - If line count ≥ threshold:
          - Build placeholder segments using the fallback style
            (single line: [Pasted Text: X lines]).
          - Call self.handle.append_line(kind,
            placeholder_segments) to render in inline UI.
          - Append the original combined plain text to transcript
            via crate::utils::transcript::append(&combined_plain)
            (do not replace it with placeholder).
      - If below threshold, keep current behavior.
  3. No toggle
      - Do not add slash commands, keybindings, or config toggles.

  Public API / Interface Changes

  - New UI constants in vtcode-config/src/constants/ui.rs.
  - No new commands, no config schema changes, no new user-facing
    toggles.

  Test Cases

  - Add unit tests in vtcode-core/src/utils/ansi.rs:
      1. user_large_paste_collapses_in_inline_ui
          - Send a 20+ line user message to
            InlineSink::write_multiline.
          - Assert InlineCommand::AppendLine contains placeholder
            text, not original content.
      2. user_small_paste_renders_normally
          - Send a short user message (<20 lines).
          - Assert rendered segments contain original content.

  Assumptions & Defaults

  - “Large paste” means a user-submitted message with ≥20 lines.
  - Placeholder format is exactly [Pasted Text: X lines].
  - Only inline TUI transcript is collapsed; other outputs remain
    intact.
  - No toggle is required or desired.



  Implement this plan?

› 1. Yes, implement this plan  Switch to Default and start coding.
  2. No, stay in Plan mode     Continue planning with the model.

  Press enter to confirm or esc to go back
```

as a temporary markdown file or in .vtcode/plans. display and rendered in the tui after ask user
questions interview.

---

1.  TUI and UI suggest for plan mode: ask user question (interview)

    ```
    Question 1/3 (3 unanswered)
      How should the placeholder toggle work?

      › 1. Global toggle (Recommended)  Single command/key toggles all
                                        large pastes between
                                        placeholder and full text.
        2. Per-paste toggle             Each placeholder can be
                                        expanded/collapsed
    individually
                                        (more UI/state work).
        3. Always expanded              No placeholder; keep current
                                        behavior (not recommended for
                                        this task).
        4. None of the above            Optionally, add details in
                                        notes (tab).


      tab to add notes | enter to submit answer
      ctrl + n next question | esc to interrupt
    ```

    2. summarize form

    ```
    • Questions 3/3 answered
      • How should the placeholder toggle work?
        answer: Global toggle (Recommended)
      • When should a paste be replaced by the placeholder?
        answer: >= 20 lines (Recommended)
      • Where should the placeholder apply?
        answer: Inline TUI only (Recommended)
    ```

    3. if there is any unsure part the agent want to need to ask user
       for more clarification, just ask again if need

    ```
    Question 1/1 (1 unanswered)
      How should users toggle large-paste placeholders?

      › 1. Slash command (Recommended)  Add `/paste` or `/pastes` to
                                        toggle, with optional on/off
                                        args.
        2. Key binding                  Add a key like `Ctrl+P` to
                                        toggle during the session.
        3. Config-only                  Only via `vtcode.toml` (no
                                        runtime toggle).
        4. None of the above            Optionally, add details in
                                        notes (tab).

      tab to add notes | enter to submit answer | esc to interrupt
    ```

    4. free text input example

    ```

      Question 1/1 (1 unanswered)
      Pick the key binding for toggling large-paste placeholders.

        1. Ctrl+Shift+P (Recommended)  Avoids conflicts with existing
                                       Ctrl+P usage in palettes/
    history
                                       picker.
        2. Alt+P                       Simple chord, but may conflict
                                       with terminal Alt bindings.
        3. Ctrl+Alt+P                  Low conflict, but harder to
    type
                                       on some terminals.
      › 4. None of the above           Optionally, add details in
    notes
                                       (tab).

      › maybe skipp keybinding

      tab or esc to clear notes | enter to submit answer
    ```

    4. the final propose plan could reference this

    ```
     Proposed Plan


      Title: Inline TUI Large Paste Placeholder (No Toggle)

      Summary

      - Detect large pasted user messages (≥20 lines) and render a
        single placeholder line in the inline TUI transcript: [Pasted
        Text: X lines].
      - Keep the full pasted content for actual submission and
        transcript logs; only the inline TUI rendering is collapsed.
      - No user toggle is provided (per request).

      Key Decisions

      - Threshold: >= 20 lines.
      - Scope: inline TUI only (stdout and transcript logs keep full
        text).
      - Placeholder text: exact format [Pasted Text: X lines].
      - Applies only to user messages (not agent, tool, or PTY
    output).

      Implementation Details

      1. Add constants
          - Add INLINE_PASTE_PLACEHOLDER_MIN_LINES: usize = 20 and
            INLINE_PASTE_PLACEHOLDER_LABEL: &str = "Pasted Text" in
          - In vtcode-core/src/utils/ansi.rs, inside
            InlineSink::write_multiline, intercept
            InlineMessageKind::User.
          - Compute line count from the original text using
            text.split('\n').count() (counts trailing empty line if
            present).
          - If line count ≥ threshold:
              - Build placeholder segments using the fallback style
                (single line: [Pasted Text: X lines]).
              - Call self.handle.append_line(kind,
                placeholder_segments) to render in inline UI.
              - Append the original combined plain text to transcript
                via crate::utils::transcript::append(&combined_plain)
      3. No toggle
          - Do not add slash commands, keybindings, or config toggles.

      Public API / Interface Changes

      - New UI constants in vtcode-config/src/constants/ui.rs.
      - No new commands, no config schema changes, no new user-facing
        toggles.

      Test Cases

      - Add unit tests in vtcode-core/src/utils/ansi.rs:
          1. user_large_paste_collapses_in_inline_ui
              - Send a 20+ line user message to
                InlineSink::write_multiline.
              - Assert InlineCommand::AppendLine contains placeholder
                text, not original content.
          2. user_small_paste_renders_normally
              - Send a short user message (<20 lines).
              - Assert rendered segments contain original content.

      Assumptions & Defaults

      - “Large paste” means a user-submitted message with ≥20 lines.
      - Placeholder format is exactly [Pasted Text: X lines].
      - Only inline TUI transcript is collapsed; other outputs remain
        intact.
      - No toggle is required or desired.



      Implement this plan?

    › 1. Yes, implement this plan  Switch to Default and start coding.
      2. No, stay in Plan mode     Continue planning with the model.

      Press enter to confirm or esc to go back
    ```

    as a temporary markdown file or in .vtcode/plans. display and
    rendered in the tui after ask user questions interview.

    when user confirm

--

```
# Plan: Ask Questions Interview + Plan Confirmation Flow

  ## Summary

  Implement an interview-style Ask Questions wizard
  (request_user_input) with sequential steps, explicit Question X/Y
  headers, optional notes input, and a transcript summary. After
  the interview, render the plan markdown (from .vtcode/plans/) in
  the TUI and show a confirmation prompt with exactly two options:
  “Yes, implement this plan” and “No, stay in Plan mode.” Keep
  ask_user_question unchanged. Disable search in the wizard to
  repurpose Tab for notes input.

  ## Public APIs / Interfaces

  - No new external APIs.
  - Update TUI wizard rendering and request_user_input flow.
  - Plan confirmation UI copy updated to match provided text.

  ## Implementation Steps

  1. Switch Ask Questions to MultiStep wizard
      - In src/agent/runloop/unified/request_user_input.rs, change
        wizard mode from TabbedList to MultiStep.
      - Ensure steps advance on Enter, and final step submits all
        answers.
  2. Add “Question X/Y (N unanswered)” header
      - Extend wizard state to compute and expose:
          - current step index
          - total steps
          - unanswered count
      - Render this header in wizard modal above the question text.
      - Likely files:
          - vtcode-core/src/ui/tui/session/modal/state.rs
          - vtcode-core/src/ui/tui/session/modal/render.rs
  3. Add notes input for “Other / None of the above”
      - Disable wizard search for request_user_input modals so Tab
        can toggle notes input.
      - Add a text-input field shown when “Other/None of the above”
        is selected.
      - Capture notes and include them as other in
        RequestUserInputAnswer.
      - Update wizard event handling to accept text entry and clear
        via Tab or Esc as in the example.
      - Likely files:
          - vtcode-core/src/ui/tui/session/modal/state.rs
          - vtcode-core/src/ui/tui/session/modal/render.rs
          - src/agent/runloop/unified/request_user_input.rs
  4. Print summary to transcript
      - After wizard submission, format the summary exactly as in
        your example:
        InlineHandle::append_line.
      - Implement in src/agent/runloop/unified/
        request_user_input.rs.
  5. Plan markdown render + confirm
      - After the interview completes, render plan markdown in the
        TUI and show the confirmation prompt:
          - “Implement this plan?”
          - Options:
              1. “Yes, implement this plan  Switch to Default and
                 start coding.”
              2. “No, stay in Plan mode     Continue planning with
                 the model.”
      - Hook this into the plan flow after the interview is done
        and the plan file exists in .vtcode/plans/.
      - Likely integrate with existing plan confirmation modal in:
          - src/agent/runloop/unified/plan_confirmation.rs
          - src/agent/runloop/unified/tool_pipeline/execution.rs
            (exit_plan_mode flow)
  6. Keep ask_user_question unchanged
      - No changes to its UI or behavior.

  ## Tests

  - Add/adjust unit tests for wizard behavior:
      - MultiStep flow advances with Enter and submits at the end.
      - Header text renders Question X/Y (N unanswered).
      - Notes input captured when “Other/None of the above”
        selected.
  - If tests exist for request_user_input, extend them in:
      - vtcode-core/src/ui/tui/session/modal/tests.rs
      - src/agent/runloop/unified/tool_pipeline/tests.rs (if
        needed)

  ## Assumptions

  - “Ask Questions” refers to request_user_input (ask_questions
    alias).
  - Search is disabled for ask_questions wizard to allow Tab to
    control notes input.
  - Summary is printed to the transcript (not a modal).
  - Plan markdown is rendered in TUI and confirmed immediately
    after the interview.
```

---

Planning Mode [X] │
│ Enable planning mode and onboarding hints

1. sync planning mode on /config with persistant vtcode.toml config for plan/agent mode
2. change to switcher (agent/plan) instead of separate commands
