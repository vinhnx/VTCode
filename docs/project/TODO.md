https://github.com/vinhnx/VTCode/issues/705

--

reference and explore research /Users/vinhnguyenxuan/Developer/learn-by-doing/claude-code-main and apply learning to improve vtcode codebase.

==

add grok model https://docs.x.ai/overview

===

https://github.com/astral-sh/hawk

===

reference and support accessibility research and best practices to apply to vtcode codebase.

https://code.claude.com/docs/en/accessibility

===

implement thinking ui/ux, remove the italic style text implement 2 state thinking mode, and add a thinking icon to the thinking state.

/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/though.png
/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/thinking.png

===

improve agent's message with better formatting, colorization, and styling markdown, follow vtcode system prompt style and formatting, and apply to all agent's message.

the summarization message is showing just plain text without proper heading, bold, italic text, causing the message to be hard to read and understand. improve the summarization message with better formatting, colorization, and styling markdown, follow vtcode system prompt style and formatting, and apply to all agent's message.

```
 Completed the first startup optimization with verified build success.

### What changed

     • `src/startup/first_run.rs`: added an early-return guard in
       `maybe_run_first_run_setup` so the first-run setup wizard is skipped when
       the effective config already has `agent.provider` or `agent.default_model` set,
       even if the workspace lacks `vtcode.toml`.

### Why this helps

     • For returning users with global config, this avoids unnecessary
       interactive/non-interactive first-run work on startup.

### Verification

     • `cargo check --locked -p vtcode` passed.
     Current measurable instrumentation
     • `src/main_helpers/bootstrap.rs` already logs overall `startup_ms` to
       `vtcode.startup` for quantifying launch time.

### Recommended follow-up

     • Profile startup with `cargo run --release` to see whether first-run
       bypass materially improves real-world launch time.
```

===

VT Code should also able to trigger plan mode on demand for demanding and complex tasks. When the agent is in non-plan mode (build, auto, duck) modes, it should provide an option to auto-switch to plan mode. and execute the plan mode to generate a plan for the task, and then execute the plan to complete the task. The agent should also be able to provide a summary of the plan and its execution results. then, the agent should be able to switch back to non-plan mode and continue with the task execution. The plan mode should also support user input and feedback to refine the plan and improve its execution.

===

check and relax plan mode tools policy permission so that VT Code can use read tools to read and explore file, currently it is being blocked

ALSO CRITICAL: extend plan mode tools call limit
" Safety validation failed: Per-turn tool limit reached (max: 48). Wait or adjust config."
" Shell execution is blocked"
log: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_804.json

"• I can’t open files directly here because exec_command is blocked by tool policy"
" I cannot currently open files directly because shell execution is denied by
policy and the code search index appears unavailable in this session."

log:

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_795.json

/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/.vtcode/checkpoints/turn_801.json
