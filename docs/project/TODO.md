https://github.com/vinhnx/VTCode/issues/705

--

reference and explore research /Users/vinhnguyenxuan/Developer/learn-by-doing/claude-code-main and apply learning to improve vtcode codebase.

===

reference and support accessibility research and best practices to apply to vtcode codebase.

https://code.claude.com/docs/en/accessibility

===

implement thinking ui/ux, remove the italic style text implement 2 state thinking mode, and add a thinking icon to the thinking state.

/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/though.png
/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/thinking.png

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
