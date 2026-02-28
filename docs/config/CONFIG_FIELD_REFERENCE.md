# Config Field Reference

Generated from `vtcode-config` schema (`VTCodeConfig`) for complete field coverage.

Regenerate:

```bash
python3 scripts/generate_config_field_reference.py
```

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `acp.enabled` | `boolean` | no | `false` | Globally enable the ACP bridge |
| `acp.zed.auth.auth_url` | `null \| string` | no | `null` | URL where users can get their API key (optional, for UI display) |
| `acp.zed.auth.default_method` | `string` | no | `"agent"` | Default authentication method for ACP agents Options: "agent" (default - agent handles auth), "env_var", "terminal" |
| `acp.zed.auth.env_var_name` | `null \| string` | no | `null` | Environment variable name for auth (used when default_method is "env_var") Examples: "OPENAI_API_KEY", "ANTHROPIC_API_KEY" |
| `acp.zed.enabled` | `boolean` | no | `false` | Enable Zed integration |
| `acp.zed.tools.list_files` | `boolean` | no | `true` | Toggle the list_files function bridge |
| `acp.zed.tools.read_file` | `boolean` | no | `true` | Toggle the read_file function bridge |
| `acp.zed.transport` | `string` | no | `"stdio"` | Transport used to communicate with the Zed client |
| `acp.zed.workspace_trust` | `string` | no | `"full_auto"` | Desired workspace trust level when running under ACP |
| `agent.api_key_env` | `string` | no | `"ANTHROPIC_API_KEY"` | Environment variable that stores the API key for the active provider |
| `agent.autonomous_mode` | `boolean` | no | `false` | Enable autonomous mode - auto-approve safe tools with reduced HITL prompts When true, the agent operates with fewer confirmation prompts for safe tools. |
| `agent.checkpointing.enabled` | `boolean` | no | `true` | Enable automatic checkpoints after each successful turn |
| `agent.checkpointing.max_age_days` | `integer \| null` | no | `30` | Maximum age in days before checkpoints are removed automatically (None disables) |
| `agent.checkpointing.max_snapshots` | `integer` | no | `50` | Maximum number of checkpoints to retain on disk |
| `agent.checkpointing.storage_dir` | `null \| string` | no | `null` | Optional custom directory for storing checkpoints (relative to workspace or absolute) |
| `agent.circuit_breaker.enabled` | `boolean` | no | `true` | Enable circuit breaker functionality |
| `agent.circuit_breaker.failure_threshold` | `integer` | no | `5` | Number of consecutive failures before opening circuit |
| `agent.circuit_breaker.max_open_circuits` | `integer` | no | `3` | Number of open circuits before triggering pause |
| `agent.circuit_breaker.pause_on_open` | `boolean` | no | `true` | Pause and ask user when circuit opens (vs auto-backoff) |
| `agent.circuit_breaker.recovery_cooldown` | `integer` | no | `60` | Cooldown period between recovery prompts (seconds) |
| `agent.custom_api_keys` | `object` | no | `{}` | Provider-specific API keys captured from interactive configuration flows |
| `agent.custom_api_keys.*` | `string` | no | `-` | - |
| `agent.default_editing_mode` | `string` | no | `"edit"` | Default editing mode on startup: "edit" (default) or "plan" Codex-inspired: Encourages structured planning before execution. |
| `agent.default_model` | `string` | no | `"claude-sonnet-4-5"` | Default model to use |
| `agent.enable_self_review` | `boolean` | no | `false` | Enable an extra self-review pass to refine final responses |
| `agent.enable_split_tool_results` | `boolean` | no | `true` | Enable split tool results for massive token savings (Phase 4) When enabled, tools return dual-channel output: - llm_content: Concise summary sent to LLM (token-optimized, 53-95% reduction) - ui_content: Rich output displayed to user (full details preserved) Applies to: grep_file, list_files, read_file, run_pty_cmd, write_file, edit_file Default: true (opt-out for compatibility), recommended for production use |
| `agent.harness.event_log_path` | `null \| string` | no | `null` | Optional JSONL event log path for harness events |
| `agent.harness.max_tool_calls_per_turn` | `integer` | no | `48` | Maximum number of tool calls allowed per turn |
| `agent.harness.max_tool_retries` | `integer` | no | `2` | Maximum retries for retryable tool errors |
| `agent.harness.max_tool_wall_clock_secs` | `integer` | no | `600` | Maximum wall clock time (seconds) for tool execution in a turn |
| `agent.include_temporal_context` | `boolean` | no | `true` | Include current date/time in system prompt for temporal awareness Helps LLM understand context for time-sensitive tasks (default: true) |
| `agent.include_working_directory` | `boolean` | no | `true` | Include current working directory in system prompt (default: true) |
| `agent.instruction_files` | `array` | no | `[]` | Additional instruction files or globs to merge into the hierarchy |
| `agent.instruction_files[]` | `string` | no | `-` | - |
| `agent.instruction_max_bytes` | `integer` | no | `16384` | Maximum bytes of instruction content to load from AGENTS.md hierarchy |
| `agent.max_conversation_turns` | `integer` | no | `150` | Maximum number of conversation turns before auto-termination |
| `agent.max_review_passes` | `integer` | no | `1` | Maximum number of self-review passes |
| `agent.max_task_retries` | `integer` | no | `2` | Maximum number of retries for agent task execution (default: 2) When an agent task fails due to retryable errors (timeout, network, 503, etc.), it will be retried up to this many times with exponential backoff |
| `agent.onboarding.chat_placeholder` | `null \| string` | no | `null` | Placeholder suggestion for the chat input bar |
| `agent.onboarding.enabled` | `boolean` | no | `true` | Toggle onboarding message rendering |
| `agent.onboarding.guideline_highlight_limit` | `integer` | no | `3` | Maximum number of guideline bullets to surface |
| `agent.onboarding.include_guideline_highlights` | `boolean` | no | `true` | Whether to include AGENTS.md highlights in onboarding message |
| `agent.onboarding.include_language_summary` | `boolean` | no | `false` | Whether to include language summary in onboarding message |
| `agent.onboarding.include_project_overview` | `boolean` | no | `true` | Whether to include project overview in onboarding message |
| `agent.onboarding.include_recommended_actions_in_welcome` | `boolean` | no | `false` | Whether to surface suggested actions inside the welcome text banner |
| `agent.onboarding.include_usage_tips_in_welcome` | `boolean` | no | `false` | Whether to surface usage tips inside the welcome text banner |
| `agent.onboarding.intro_text` | `string` | no | `"Let's get oriented. I preloaded workspace context so we can move fast."` | Introductory text shown at session start |
| `agent.onboarding.recommended_actions` | `array` | no | `["Review the highlighted guidelines and share the task you want to tackle.", "Ask for a workspace tour if you need mo...` | Recommended follow-up actions to display |
| `agent.onboarding.recommended_actions[]` | `string` | no | `-` | - |
| `agent.onboarding.usage_tips` | `array` | no | `["Describe your current coding goal or ask for a quick status overview.", "Reference AGENTS.md guidelines when propos...` | Tips for collaborating with the agent effectively |
| `agent.onboarding.usage_tips[]` | `string` | no | `-` | - |
| `agent.open_responses.emit_events` | `boolean` | no | `true` | Emit Open Responses events to the event sink When true, streaming events follow Open Responses format (response.created, response.output_item.added, response.output_text.delta, etc.) |
| `agent.open_responses.enabled` | `boolean` | no | `false` | Enable Open Responses specification compliance layer When true, VT Code emits semantic streaming events alongside internal events Default: false (opt-in feature) |
| `agent.open_responses.include_extensions` | `boolean` | no | `true` | Include VT Code extension items (vtcode:file_change, vtcode:web_search, etc.) When false, extension items are omitted from the Open Responses output |
| `agent.open_responses.include_reasoning` | `boolean` | no | `true` | Include reasoning items in Open Responses output When true, model reasoning/thinking is exposed as reasoning items |
| `agent.open_responses.map_tool_calls` | `boolean` | no | `true` | Map internal tool calls to Open Responses function_call items When true, command executions and MCP tool calls are represented as function_call items |
| `agent.project_doc_max_bytes` | `integer` | no | `16384` | Maximum bytes of AGENTS.md content to load from project hierarchy |
| `agent.provider` | `string` | no | `"anthropic"` | AI provider for single agent mode (gemini, openai, anthropic, openrouter, xai, zai) |
| `agent.reasoning_effort` | `string` | no | `"medium"` | Reasoning effort level for models that support it (none, low, medium, high) Applies to: Claude, GPT-5, GPT-5.1, Gemini, Qwen3, DeepSeek with reasoning capability |
| `agent.refine_prompts_enabled` | `boolean` | no | `false` | Enable prompt refinement pass before sending to LLM |
| `agent.refine_prompts_max_passes` | `integer` | no | `1` | Max refinement passes for prompt writing |
| `agent.refine_prompts_model` | `string` | no | `""` | Optional model override for the refiner (empty = auto pick efficient sibling) |
| `agent.refine_temperature` | `number` | no | `0.30000001192092896` | Temperature for prompt refinement (0.0-1.0, default: 0.3) Lower values ensure prompt refinement is more deterministic/consistent Keep lower than main temperature for stable prompt improvement |
| `agent.require_plan_confirmation` | `boolean` | no | `true` | Require user confirmation before executing a plan generated in plan mode When true, exiting plan mode shows the implementation blueprint and requires explicit user approval before enabling edit tools. |
| `agent.small_model.enabled` | `boolean` | no | `true` | Enable small model tier for efficient operations |
| `agent.small_model.model` | `string` | no | `""` | Small model to use (e.g., claude-4-5-haiku, "gpt-4-mini", "gemini-2.0-flash") Leave empty to auto-select a lightweight sibling of the main model |
| `agent.small_model.temperature` | `number` | no | `0.30000001192092896` | Temperature for small model responses |
| `agent.small_model.use_for_git_history` | `boolean` | no | `true` | Enable small model for git history processing |
| `agent.small_model.use_for_large_reads` | `boolean` | no | `true` | Enable small model for large file reads (>50KB) |
| `agent.small_model.use_for_web_summary` | `boolean` | no | `true` | Enable small model for web content summarization |
| `agent.system_prompt_mode` | `string` | no | `"default"` | System prompt mode controlling verbosity and token overhead Options: minimal (~500-800 tokens), lightweight (~1-2k), default (~6-7k), specialized (~7-8k) Inspired by pi-coding-agent: modern models often perform well with minimal prompts |
| `agent.temperature` | `number` | no | `0.699999988079071` | Temperature for main LLM responses (0.0-1.0) Lower values = more deterministic, higher values = more creative Recommended: 0.7 for balanced creativity and consistency Range: 0.0 (deterministic) to 1.0 (maximum randomness) |
| `agent.temporal_context_use_utc` | `boolean` | no | `false` | Use UTC instead of local time for temporal context in system prompts |
| `agent.theme` | `string` | no | `"ciapre-dark"` | UI theme identifier controlling ANSI styling |
| `agent.todo_planning_mode` | `boolean` | no | `true` | Enable TODO planning helper mode for structured task management |
| `agent.tool_documentation_mode` | `string` | no | `"full"` | Tool documentation mode controlling token overhead for tool definitions Options: minimal (~800 tokens), progressive (~1.2k), full (~3k current) Progressive: signatures upfront, detailed docs on-demand (recommended) Minimal: signatures only, pi-coding-agent style (power users) Full: all documentation upfront (current behavior, default) |
| `agent.ui_surface` | `string` | no | `"auto"` | Preferred rendering surface for the interactive chat UI (auto, alternate, inline) |
| `agent.user_instructions` | `null \| string` | no | `null` | Custom instructions provided by the user via configuration to guide agent behavior |
| `agent.verbosity` | `string` | no | `"medium"` | Verbosity level for output text (low, medium, high) Applies to: GPT-5.1 and other models that support verbosity control |
| `agent.vibe_coding.enable_conversation_memory` | `boolean` | no | `true` | Enable conversation memory for pronoun resolution |
| `agent.vibe_coding.enable_entity_resolution` | `boolean` | no | `true` | Enable fuzzy entity resolution |
| `agent.vibe_coding.enable_proactive_context` | `boolean` | no | `true` | Enable proactive context gathering |
| `agent.vibe_coding.enable_pronoun_resolution` | `boolean` | no | `true` | Enable pronoun resolution (it, that, this) |
| `agent.vibe_coding.enable_relative_value_inference` | `boolean` | no | `true` | Enable relative value inference (by half, double, etc.) |
| `agent.vibe_coding.enabled` | `boolean` | no | `false` | Enable vibe coding support |
| `agent.vibe_coding.entity_index_cache` | `string` | no | `".vtcode/entity_index.json"` | Entity index cache file path (relative to workspace) |
| `agent.vibe_coding.max_context_files` | `integer` | no | `3` | Maximum files to gather for context (default: 3) |
| `agent.vibe_coding.max_context_snippets_per_file` | `integer` | no | `20` | Maximum code snippets per file (default: 20 lines) |
| `agent.vibe_coding.max_entity_matches` | `integer` | no | `5` | Maximum entity matches to return (default: 5) |
| `agent.vibe_coding.max_memory_turns` | `integer` | no | `50` | Maximum conversation turns to remember (default: 50) |
| `agent.vibe_coding.max_recent_files` | `integer` | no | `20` | Maximum recent files to track (default: 20) |
| `agent.vibe_coding.max_search_results` | `integer` | no | `5` | Maximum search results to include (default: 5) |
| `agent.vibe_coding.min_prompt_length` | `integer` | no | `5` | Minimum prompt length for refinement (default: 5 chars) |
| `agent.vibe_coding.min_prompt_words` | `integer` | no | `2` | Minimum prompt words for refinement (default: 2 words) |
| `agent.vibe_coding.track_value_history` | `boolean` | no | `true` | Track value history for inference |
| `agent.vibe_coding.track_workspace_state` | `boolean` | no | `true` | Track workspace state (file activity, value changes) |
| `agent_teams.default_model` | `null \| string` | no | `null` | Default model for agent team subagents |
| `agent_teams.enabled` | `boolean` | no | `false` | Enable agent teams (experimental) |
| `agent_teams.max_teammates` | `integer` | no | `4` | Maximum number of teammates in a team |
| `agent_teams.storage_dir` | `null \| string` | no | `null` | Override storage directory for team data |
| `agent_teams.teammate_mode` | `string` | no | `"auto"` | Teammate display mode (auto, tmux, in_process) |
| `auth.openrouter.auto_refresh` | `boolean` | no | `true` | Whether to automatically refresh tokens when they expire. If false, the user will be prompted to re-authenticate. |
| `auth.openrouter.callback_port` | `integer` | no | `8484` | Port for the local OAuth callback server. The server listens on localhost for the OAuth redirect. |
| `auth.openrouter.flow_timeout_secs` | `integer` | no | `300` | Timeout in seconds for the OAuth flow. If the user doesn't complete authentication within this time, the flow is cancelled. |
| `auth.openrouter.use_oauth` | `boolean` | no | `false` | Whether to use OAuth instead of API key for authentication. When enabled, VT Code will prompt for OAuth login if no valid token exists. |
| `automation.full_auto.allowed_tools` | `array` | no | `["read_file", "list_files", "grep_file"]` | Allow-list of tools that may execute automatically. |
| `automation.full_auto.allowed_tools[]` | `string` | no | `-` | - |
| `automation.full_auto.enabled` | `boolean` | no | `false` | Enable the runtime flag once the workspace is configured for autonomous runs. |
| `automation.full_auto.max_turns` | `integer` | no | `30` | Maximum number of autonomous agent turns before the exec runner pauses. |
| `automation.full_auto.profile_path` | `null \| string` | no | `null` | Optional path to a profile describing acceptable behaviors. |
| `automation.full_auto.require_profile_ack` | `boolean` | no | `true` | Require presence of a profile/acknowledgement file before activation. |
| `chat.askQuestions.enabled` | `boolean` | no | `true` | Enable the Ask Questions tool in interactive chat |
| `commands.allow_glob` | `array` | no | `[]` | Glob patterns allowed for shell commands (applies to Bash) |
| `commands.allow_glob[]` | `string` | no | `-` | - |
| `commands.allow_list` | `array` | no | `[]` | Commands that can be executed without prompting |
| `commands.allow_list[]` | `string` | no | `-` | - |
| `commands.allow_regex` | `array` | no | `[]` | Regex allow patterns for shell commands |
| `commands.allow_regex[]` | `string` | no | `-` | - |
| `commands.deny_glob` | `array` | no | `[]` | Glob patterns denied for shell commands |
| `commands.deny_glob[]` | `string` | no | `-` | - |
| `commands.deny_list` | `array` | no | `[]` | Commands that are always denied |
| `commands.deny_list[]` | `string` | no | `-` | - |
| `commands.deny_regex` | `array` | no | `[]` | Regex deny patterns for shell commands |
| `commands.deny_regex[]` | `string` | no | `-` | - |
| `commands.extra_path_entries` | `array` | no | `["$HOME/.cargo/bin", "$HOME/.local/bin", "/opt/homebrew/bin", "/usr/local/bin", "$HOME/.asdf/bin", "$HOME/.asdf/shims...` | Additional directories that should be searched/prepended to PATH for command execution |
| `commands.extra_path_entries[]` | `string` | no | `-` | - |
| `context.dynamic.enabled` | `boolean` | no | `true` | Enable dynamic context discovery features |
| `context.dynamic.max_spooled_files` | `integer` | no | `100` | Maximum number of spooled files to keep |
| `context.dynamic.persist_history` | `boolean` | no | `true` | Enable persisting conversation history during summarization |
| `context.dynamic.spool_max_age_secs` | `integer` | no | `3600` | Maximum age in seconds for spooled tool output files before cleanup |
| `context.dynamic.sync_mcp_tools` | `boolean` | no | `true` | Enable syncing MCP tool descriptions to .vtcode/mcp/tools/ |
| `context.dynamic.sync_skills` | `boolean` | no | `true` | Enable generating skill index in .agents/skills/INDEX.md |
| `context.dynamic.sync_terminals` | `boolean` | no | `true` | Enable syncing terminal sessions to .vtcode/terminals/ files |
| `context.dynamic.tool_output_threshold` | `integer` | no | `8192` | Threshold in bytes above which tool outputs are spooled to files |
| `context.ledger.enabled` | `boolean` | no | `true` | - |
| `context.ledger.include_in_prompt` | `boolean` | no | `true` | Inject ledger into the system prompt each turn |
| `context.ledger.max_entries` | `integer` | no | `12` | - |
| `context.ledger.preserve_in_compression` | `boolean` | no | `true` | Preserve ledger entries during context compression |
| `context.max_context_tokens` | `integer` | no | `90000` | Maximum tokens to keep in context (affects model cost and performance) Higher values preserve more context but cost more and may hit token limits This field is maintained for compatibility but no longer used for trimming |
| `context.preserve_recent_turns` | `integer` | no | `10` | Preserve recent turns during context management This field is maintained for compatibility but no longer used for trimming |
| `context.trim_to_percent` | `integer` | no | `60` | Percentage to trim context to when it gets too large This field is maintained for compatibility but no longer used for trimming |
| `debug.debug_log_dir` | `null \| string` | no | `null` | Directory for debug logs |
| `debug.enable_tracing` | `boolean` | no | `false` | Enable structured logging for development and troubleshooting |
| `debug.max_debug_log_age_days` | `integer` | no | `7` | Maximum age of debug logs to keep (in days) |
| `debug.max_debug_log_size_mb` | `integer` | no | `50` | Maximum size of debug logs before rotating (in MB) |
| `debug.trace_level` | `string` | no | `"info"` | Trace level (error, warn, info, debug, trace) |
| `debug.trace_targets` | `array` | no | `[]` | List of tracing targets to enable Examples: "vtcode_core::agent", "vtcode_core::tools", "vtcode::*" |
| `debug.trace_targets[]` | `string` | no | `-` | - |
| `dotfile_protection.additional_protected_patterns` | `array` | no | `[]` | Additional dotfile patterns to protect (beyond defaults). |
| `dotfile_protection.additional_protected_patterns[]` | `string` | no | `-` | - |
| `dotfile_protection.audit_log_path` | `string` | no | `"~/.vtcode/dotfile_audit.log"` | Path to the audit log file. |
| `dotfile_protection.audit_logging_enabled` | `boolean` | no | `true` | Enable immutable audit logging of all dotfile access attempts. |
| `dotfile_protection.backup_directory` | `string` | no | `"~/.vtcode/dotfile_backups"` | Directory for storing dotfile backups. |
| `dotfile_protection.block_during_automation` | `boolean` | no | `true` | Block modifications during automated operations. |
| `dotfile_protection.blocked_operations` | `array` | no | `["dependency_installation", "code_formatting", "git_operations", "project_initialization", "build_operations", "test_...` | Operations that trigger extra protection. |
| `dotfile_protection.blocked_operations[]` | `string` | no | `-` | - |
| `dotfile_protection.create_backups` | `boolean` | no | `true` | Create backup before any permitted modification. |
| `dotfile_protection.enabled` | `boolean` | no | `true` | Enable dotfile protection globally. |
| `dotfile_protection.max_backups_per_file` | `integer` | no | `10` | Maximum number of backups to retain per file. |
| `dotfile_protection.preserve_permissions` | `boolean` | no | `true` | Preserve original file permissions and ownership. |
| `dotfile_protection.prevent_cascading_modifications` | `boolean` | no | `true` | Prevent cascading modifications (one dotfile change triggering others). |
| `dotfile_protection.require_explicit_confirmation` | `boolean` | no | `true` | Require explicit user confirmation for any dotfile modification. |
| `dotfile_protection.require_secondary_auth_for_whitelist` | `boolean` | no | `true` | Secondary authentication required for whitelisted files. |
| `dotfile_protection.whitelist` | `array` | no | `[]` | Whitelisted dotfiles that can be modified (after secondary confirmation). |
| `dotfile_protection.whitelist[]` | `string` | no | `-` | - |
| `hooks.lifecycle.post_tool_use` | `array` | no | `[]` | Commands to run immediately after a tool returns its output |
| `hooks.lifecycle.post_tool_use[].hooks` | `array` | no | `[]` | List of hook commands to execute sequentially in this group |
| `hooks.lifecycle.post_tool_use[].hooks[].command` | `string` | no | `""` | The shell command string to execute |
| `hooks.lifecycle.post_tool_use[].hooks[].timeout_seconds` | `integer \| null` | no | `null` | Optional execution timeout in seconds |
| `hooks.lifecycle.post_tool_use[].hooks[].type` | `string` | no | `"command"` | Type of hook command (currently only 'command' is supported) |
| `hooks.lifecycle.post_tool_use[].matcher` | `null \| string` | no | `null` | Optional regex matcher to filter when this group runs. Matched against context strings (e.g. tool name, project path). |
| `hooks.lifecycle.pre_tool_use` | `array` | no | `[]` | Commands to run immediately before a tool is executed |
| `hooks.lifecycle.pre_tool_use[].hooks` | `array` | no | `[]` | List of hook commands to execute sequentially in this group |
| `hooks.lifecycle.pre_tool_use[].hooks[].command` | `string` | no | `""` | The shell command string to execute |
| `hooks.lifecycle.pre_tool_use[].hooks[].timeout_seconds` | `integer \| null` | no | `null` | Optional execution timeout in seconds |
| `hooks.lifecycle.pre_tool_use[].hooks[].type` | `string` | no | `"command"` | Type of hook command (currently only 'command' is supported) |
| `hooks.lifecycle.pre_tool_use[].matcher` | `null \| string` | no | `null` | Optional regex matcher to filter when this group runs. Matched against context strings (e.g. tool name, project path). |
| `hooks.lifecycle.session_end` | `array` | no | `[]` | Commands to run when an agent session ends |
| `hooks.lifecycle.session_end[].hooks` | `array` | no | `[]` | List of hook commands to execute sequentially in this group |
| `hooks.lifecycle.session_end[].hooks[].command` | `string` | no | `""` | The shell command string to execute |
| `hooks.lifecycle.session_end[].hooks[].timeout_seconds` | `integer \| null` | no | `null` | Optional execution timeout in seconds |
| `hooks.lifecycle.session_end[].hooks[].type` | `string` | no | `"command"` | Type of hook command (currently only 'command' is supported) |
| `hooks.lifecycle.session_end[].matcher` | `null \| string` | no | `null` | Optional regex matcher to filter when this group runs. Matched against context strings (e.g. tool name, project path). |
| `hooks.lifecycle.session_start` | `array` | no | `[]` | Commands to run immediately when an agent session begins |
| `hooks.lifecycle.session_start[].hooks` | `array` | no | `[]` | List of hook commands to execute sequentially in this group |
| `hooks.lifecycle.session_start[].hooks[].command` | `string` | no | `""` | The shell command string to execute |
| `hooks.lifecycle.session_start[].hooks[].timeout_seconds` | `integer \| null` | no | `null` | Optional execution timeout in seconds |
| `hooks.lifecycle.session_start[].hooks[].type` | `string` | no | `"command"` | Type of hook command (currently only 'command' is supported) |
| `hooks.lifecycle.session_start[].matcher` | `null \| string` | no | `null` | Optional regex matcher to filter when this group runs. Matched against context strings (e.g. tool name, project path). |
| `hooks.lifecycle.task_completed` | `array` | no | `[]` | Commands to run after a task is finalized and session is closed |
| `hooks.lifecycle.task_completed[].hooks` | `array` | no | `[]` | List of hook commands to execute sequentially in this group |
| `hooks.lifecycle.task_completed[].hooks[].command` | `string` | no | `""` | The shell command string to execute |
| `hooks.lifecycle.task_completed[].hooks[].timeout_seconds` | `integer \| null` | no | `null` | Optional execution timeout in seconds |
| `hooks.lifecycle.task_completed[].hooks[].type` | `string` | no | `"command"` | Type of hook command (currently only 'command' is supported) |
| `hooks.lifecycle.task_completed[].matcher` | `null \| string` | no | `null` | Optional regex matcher to filter when this group runs. Matched against context strings (e.g. tool name, project path). |
| `hooks.lifecycle.task_completion` | `array` | no | `[]` | Commands to run when the agent indicates task completion (pre-exit) |
| `hooks.lifecycle.task_completion[].hooks` | `array` | no | `[]` | List of hook commands to execute sequentially in this group |
| `hooks.lifecycle.task_completion[].hooks[].command` | `string` | no | `""` | The shell command string to execute |
| `hooks.lifecycle.task_completion[].hooks[].timeout_seconds` | `integer \| null` | no | `null` | Optional execution timeout in seconds |
| `hooks.lifecycle.task_completion[].hooks[].type` | `string` | no | `"command"` | Type of hook command (currently only 'command' is supported) |
| `hooks.lifecycle.task_completion[].matcher` | `null \| string` | no | `null` | Optional regex matcher to filter when this group runs. Matched against context strings (e.g. tool name, project path). |
| `hooks.lifecycle.teammate_idle` | `array` | no | `[]` | Commands to run when a teammate agent remains idle |
| `hooks.lifecycle.teammate_idle[].hooks` | `array` | no | `[]` | List of hook commands to execute sequentially in this group |
| `hooks.lifecycle.teammate_idle[].hooks[].command` | `string` | no | `""` | The shell command string to execute |
| `hooks.lifecycle.teammate_idle[].hooks[].timeout_seconds` | `integer \| null` | no | `null` | Optional execution timeout in seconds |
| `hooks.lifecycle.teammate_idle[].hooks[].type` | `string` | no | `"command"` | Type of hook command (currently only 'command' is supported) |
| `hooks.lifecycle.teammate_idle[].matcher` | `null \| string` | no | `null` | Optional regex matcher to filter when this group runs. Matched against context strings (e.g. tool name, project path). |
| `hooks.lifecycle.user_prompt_submit` | `array` | no | `[]` | Commands to run when the user submits a prompt (pre-processing) |
| `hooks.lifecycle.user_prompt_submit[].hooks` | `array` | no | `[]` | List of hook commands to execute sequentially in this group |
| `hooks.lifecycle.user_prompt_submit[].hooks[].command` | `string` | no | `""` | The shell command string to execute |
| `hooks.lifecycle.user_prompt_submit[].hooks[].timeout_seconds` | `integer \| null` | no | `null` | Optional execution timeout in seconds |
| `hooks.lifecycle.user_prompt_submit[].hooks[].type` | `string` | no | `"command"` | Type of hook command (currently only 'command' is supported) |
| `hooks.lifecycle.user_prompt_submit[].matcher` | `null \| string` | no | `null` | Optional regex matcher to filter when this group runs. Matched against context strings (e.g. tool name, project path). |
| `mcp.allowlist.default.configuration` | `null \| object` | no | `null` | Configuration keys permitted for the provider grouped by category |
| `mcp.allowlist.default.logging` | `array \| null` | no | `null` | Logging channels permitted for the provider |
| `mcp.allowlist.default.logging[]` | `string` | no | `-` | - |
| `mcp.allowlist.default.prompts` | `array \| null` | no | `null` | Prompt name patterns permitted for the provider |
| `mcp.allowlist.default.prompts[]` | `string` | no | `-` | - |
| `mcp.allowlist.default.resources` | `array \| null` | no | `null` | Resource name patterns permitted for the provider |
| `mcp.allowlist.default.resources[]` | `string` | no | `-` | - |
| `mcp.allowlist.default.tools` | `array \| null` | no | `null` | Tool name patterns permitted for the provider |
| `mcp.allowlist.default.tools[]` | `string` | no | `-` | - |
| `mcp.allowlist.enforce` | `boolean` | no | `false` | Whether to enforce allow list checks |
| `mcp.allowlist.providers` | `object` | no | `{}` | Provider-specific allow list rules |
| `mcp.allowlist.providers.*.configuration` | `null \| object` | no | `null` | Configuration keys permitted for the provider grouped by category |
| `mcp.allowlist.providers.*.logging` | `array \| null` | no | `null` | Logging channels permitted for the provider |
| `mcp.allowlist.providers.*.logging[]` | `string` | no | `-` | - |
| `mcp.allowlist.providers.*.prompts` | `array \| null` | no | `null` | Prompt name patterns permitted for the provider |
| `mcp.allowlist.providers.*.prompts[]` | `string` | no | `-` | - |
| `mcp.allowlist.providers.*.resources` | `array \| null` | no | `null` | Resource name patterns permitted for the provider |
| `mcp.allowlist.providers.*.resources[]` | `string` | no | `-` | - |
| `mcp.allowlist.providers.*.tools` | `array \| null` | no | `null` | Tool name patterns permitted for the provider |
| `mcp.allowlist.providers.*.tools[]` | `string` | no | `-` | - |
| `mcp.connection_pooling_enabled` | `boolean` | no | `true` | Enable connection pooling for better performance |
| `mcp.connection_timeout_seconds` | `integer` | no | `30` | Connection timeout in seconds |
| `mcp.enabled` | `boolean` | no | `false` | Enable MCP functionality |
| `mcp.experimental_use_rmcp_client` | `boolean` | no | `true` | Toggle experimental RMCP client features |
| `mcp.max_concurrent_connections` | `integer` | no | `5` | Maximum number of concurrent MCP connections |
| `mcp.providers` | `array` | no | `[]` | Configured MCP providers |
| `mcp.providers[]` | `object` | no | `-` | Configuration for a single MCP provider |
| `mcp.request_timeout_seconds` | `integer` | no | `30` | Request timeout in seconds |
| `mcp.retry_attempts` | `integer` | no | `3` | Connection retry attempts |
| `mcp.security.api_key_env` | `null \| string` | no | `null` | API key for MCP server authentication (environment variable name) |
| `mcp.security.auth_enabled` | `boolean` | no | `false` | Enable authentication for MCP server |
| `mcp.security.rate_limit.concurrent_requests` | `integer` | no | `10` | Maximum concurrent requests per client |
| `mcp.security.rate_limit.requests_per_minute` | `integer` | no | `100` | Maximum requests per minute per client |
| `mcp.security.validation.max_argument_size` | `integer` | no | `1048576` | Maximum argument size in bytes |
| `mcp.security.validation.path_traversal_protection` | `boolean` | no | `true` | Enable path traversal protection |
| `mcp.security.validation.schema_validation_enabled` | `boolean` | no | `true` | Enable JSON schema validation for tool arguments |
| `mcp.server.bind_address` | `string` | no | `"127.0.0.1"` | Bind address for the MCP server |
| `mcp.server.enabled` | `boolean` | no | `false` | Enable vtcode's MCP server capability |
| `mcp.server.exposed_tools` | `array` | no | `[]` | Tools exposed by the vtcode MCP server |
| `mcp.server.exposed_tools[]` | `string` | no | `-` | - |
| `mcp.server.name` | `string` | no | `"vtcode-mcp-server"` | Server identifier |
| `mcp.server.port` | `integer` | no | `3000` | Port for the MCP server |
| `mcp.server.transport` | `string` | no | `"sse"` | Server transport type |
| `mcp.server.version` | `string` | no | `"0.79.4"` | Server version |
| `mcp.startup_timeout_seconds` | `integer \| null` | no | `null` | Optional timeout (seconds) when starting providers |
| `mcp.tool_cache_capacity` | `integer` | no | `100` | Cache capacity for tool discovery results |
| `mcp.tool_timeout_seconds` | `integer \| null` | no | `null` | Optional timeout (seconds) for tool execution |
| `mcp.ui.max_events` | `integer` | no | `50` | Maximum number of MCP events to display |
| `mcp.ui.mode` | `string` | no | `"compact"` | UI mode for MCP events: "compact" or "full" |
| `mcp.ui.renderers` | `object` | no | `{}` | Custom renderer profiles for provider-specific output formatting |
| `mcp.ui.renderers.*` | `string` | no | `-` | Named renderer profiles for MCP tool output formatting |
| `mcp.ui.show_provider_names` | `boolean` | no | `true` | Show MCP provider names in UI |
| `model.loop_detection_interactive` | `boolean` | no | `true` | Enable interactive prompt for loop detection instead of silently halting |
| `model.loop_detection_threshold` | `integer` | no | `2` | Maximum number of identical tool calls (same tool + same arguments) before triggering loop detection |
| `model.skip_loop_detection` | `boolean` | no | `false` | Enable loop hang detection to identify when model is stuck in repetitive behavior |
| `optimization.agent_execution.enable_performance_prediction` | `boolean` | yes | `-` | Enable performance prediction |
| `optimization.agent_execution.idle_backoff_ms` | `integer` | yes | `-` | Back-off duration in milliseconds when no work is pending This reduces CPU usage during idle periods |
| `optimization.agent_execution.idle_timeout_ms` | `integer` | yes | `-` | Idle detection timeout in milliseconds (0 to disable) When the agent is idle for this duration, it will enter a low-power state |
| `optimization.agent_execution.max_execution_time_secs` | `integer` | yes | `-` | Maximum execution time in seconds |
| `optimization.agent_execution.max_idle_cycles` | `integer` | yes | `-` | Maximum consecutive idle cycles before entering deep sleep |
| `optimization.agent_execution.max_memory_mb` | `integer` | yes | `-` | Maximum memory usage in MB |
| `optimization.agent_execution.resource_monitor_interval_ms` | `integer` | yes | `-` | Resource monitoring interval in milliseconds |
| `optimization.agent_execution.state_history_size` | `integer` | yes | `-` | State transition history size |
| `optimization.agent_execution.use_optimized_loop` | `boolean` | yes | `-` | Enable optimized agent execution loop |
| `optimization.async_pipeline.batch_timeout_ms` | `integer` | yes | `-` | Batch timeout in milliseconds |
| `optimization.async_pipeline.cache_size` | `integer` | yes | `-` | Result cache size |
| `optimization.async_pipeline.enable_batching` | `boolean` | yes | `-` | Enable request batching |
| `optimization.async_pipeline.enable_caching` | `boolean` | yes | `-` | Enable result caching |
| `optimization.async_pipeline.max_batch_size` | `integer` | yes | `-` | Maximum batch size for tool requests |
| `optimization.command_cache.allowlist` | `array` | yes | `-` | Allowlist of command prefixes eligible for caching |
| `optimization.command_cache.allowlist[]` | `string` | no | `-` | - |
| `optimization.command_cache.enabled` | `boolean` | yes | `-` | Enable command caching |
| `optimization.command_cache.max_entries` | `integer` | yes | `-` | Maximum number of cached entries |
| `optimization.command_cache.ttl_ms` | `integer` | yes | `-` | Cache TTL in milliseconds |
| `optimization.file_read_cache.enabled` | `boolean` | yes | `-` | Enable file read caching |
| `optimization.file_read_cache.max_entries` | `integer` | yes | `-` | Maximum number of cached entries |
| `optimization.file_read_cache.max_size_bytes` | `integer` | yes | `-` | Maximum cached file size (bytes) |
| `optimization.file_read_cache.min_size_bytes` | `integer` | yes | `-` | Minimum file size (bytes) before caching |
| `optimization.file_read_cache.ttl_secs` | `integer` | yes | `-` | Cache TTL in seconds |
| `optimization.llm_client.cache_ttl_secs` | `integer` | yes | `-` | Response cache TTL in seconds |
| `optimization.llm_client.connection_pool_size` | `integer` | yes | `-` | Connection pool size |
| `optimization.llm_client.enable_connection_pooling` | `boolean` | yes | `-` | Enable connection pooling |
| `optimization.llm_client.enable_request_batching` | `boolean` | yes | `-` | Enable request batching |
| `optimization.llm_client.enable_response_caching` | `boolean` | yes | `-` | Enable response caching |
| `optimization.llm_client.rate_limit_burst` | `integer` | yes | `-` | Rate limit burst capacity |
| `optimization.llm_client.rate_limit_rps` | `number` | yes | `-` | Rate limit: requests per second |
| `optimization.llm_client.response_cache_size` | `integer` | yes | `-` | Response cache size |
| `optimization.memory_pool.enabled` | `boolean` | yes | `-` | Enable memory pool (can be disabled for debugging) |
| `optimization.memory_pool.max_string_pool_size` | `integer` | yes | `-` | Maximum number of strings to pool |
| `optimization.memory_pool.max_value_pool_size` | `integer` | yes | `-` | Maximum number of Values to pool |
| `optimization.memory_pool.max_vec_pool_size` | `integer` | yes | `-` | Maximum number of Vec<String> to pool |
| `optimization.profiling.auto_export_results` | `boolean` | yes | `-` | Auto-export results to file |
| `optimization.profiling.enable_regression_testing` | `boolean` | yes | `-` | Enable regression testing |
| `optimization.profiling.enabled` | `boolean` | yes | `-` | Enable performance profiling |
| `optimization.profiling.export_file_path` | `string` | yes | `-` | Export file path |
| `optimization.profiling.max_history_size` | `integer` | yes | `-` | Maximum benchmark history size |
| `optimization.profiling.max_regression_percent` | `number` | yes | `-` | Maximum allowed performance regression percentage |
| `optimization.profiling.monitor_interval_ms` | `integer` | yes | `-` | Resource monitoring interval in milliseconds |
| `optimization.tool_registry.default_timeout_secs` | `integer` | yes | `-` | Tool execution timeout in seconds |
| `optimization.tool_registry.hot_cache_size` | `integer` | yes | `-` | Hot cache size for frequently used tools |
| `optimization.tool_registry.max_concurrent_tools` | `integer` | yes | `-` | Maximum concurrent tool executions |
| `optimization.tool_registry.use_optimized_registry` | `boolean` | yes | `-` | Enable optimized registry |
| `output_style.active_style` | `string` | no | `"default"` | - |
| `permissions.audit_directory` | `string` | no | `"~/.vtcode/audit"` | Directory for audit logs (created if not exists) Defaults to ~/.vtcode/audit |
| `permissions.audit_enabled` | `boolean` | no | `true` | Enable audit logging of all permission decisions |
| `permissions.cache_enabled` | `boolean` | no | `true` | Enable permission decision caching to avoid redundant evaluations |
| `permissions.cache_ttl_seconds` | `integer` | no | `300` | Cache time-to-live in seconds (how long to cache decisions) Default: 300 seconds (5 minutes) |
| `permissions.enabled` | `boolean` | no | `true` | Enable the enhanced permission system (resolver + audit logger + cache) |
| `permissions.log_allowed_commands` | `boolean` | no | `true` | Log allowed commands to audit trail |
| `permissions.log_denied_commands` | `boolean` | no | `true` | Log denied commands to audit trail |
| `permissions.log_permission_prompts` | `boolean` | no | `true` | Log permission prompts (when user is asked for confirmation) |
| `permissions.resolve_commands` | `boolean` | no | `true` | Enable command resolution to actual paths (helps identify suspicious commands) |
| `prompt_cache.cache_dir` | `string` | no | `"~/.vtcode/cache/prompts"` | Base directory for local prompt cache storage (supports `~` expansion) |
| `prompt_cache.enable_auto_cleanup` | `boolean` | no | `true` | Automatically evict stale entries on startup/shutdown |
| `prompt_cache.enabled` | `boolean` | no | `true` | Enable prompt caching features globally |
| `prompt_cache.max_age_days` | `integer` | no | `30` | Maximum age (in days) before cached entries are purged |
| `prompt_cache.max_entries` | `integer` | no | `1000` | Maximum number of cached prompt entries to retain on disk |
| `prompt_cache.min_quality_threshold` | `number` | no | `0.7` | Minimum quality score required before persisting an entry |
| `prompt_cache.providers.anthropic.cache_system_messages` | `boolean` | no | `true` | Apply cache control to system prompts by default |
| `prompt_cache.providers.anthropic.cache_tool_definitions` | `boolean` | no | `true` | Apply cache control to tool definitions by default Default: true (tools are typically stable and benefit from longer caching) |
| `prompt_cache.providers.anthropic.cache_user_messages` | `boolean` | no | `true` | Apply cache control to user messages exceeding threshold |
| `prompt_cache.providers.anthropic.enabled` | `boolean` | no | `true` | - |
| `prompt_cache.providers.anthropic.extended_ttl_seconds` | `integer \| null` | no | `3600` | Extended TTL for Anthropic prompt caching (in seconds) Set to >= 3600 for 1-hour cache on messages |
| `prompt_cache.providers.anthropic.max_breakpoints` | `integer` | no | `4` | Maximum number of cache breakpoints to use (max 4 per Anthropic spec). Default: 4 |
| `prompt_cache.providers.anthropic.messages_ttl_seconds` | `integer` | no | `300` | TTL for subsequent cache breakpoints (messages). Set to >= 3600 for 1-hour cache on messages. Default: 300 (5 minutes) - recommended for frequently changing messages |
| `prompt_cache.providers.anthropic.min_message_length_for_cache` | `integer` | no | `256` | Minimum message length (in characters) before applying cache control to avoid caching very short messages that don't benefit from caching. Default: 256 characters (~64 tokens) |
| `prompt_cache.providers.anthropic.tools_ttl_seconds` | `integer` | no | `3600` | Default TTL in seconds for the first cache breakpoint (tools/system). Anthropic only supports "5m" (300s) or "1h" (3600s) TTL formats. Set to >= 3600 for 1-hour cache on tools and system prompts. Default: 3600 (1 hour) - recommended for stable tool definitions |
| `prompt_cache.providers.deepseek.enabled` | `boolean` | no | `true` | - |
| `prompt_cache.providers.deepseek.surface_metrics` | `boolean` | no | `true` | Emit cache hit/miss metrics from responses when available |
| `prompt_cache.providers.gemini.enabled` | `boolean` | no | `true` | - |
| `prompt_cache.providers.gemini.explicit_ttl_seconds` | `integer \| null` | no | `3600` | TTL for explicit caches (ignored in implicit mode) |
| `prompt_cache.providers.gemini.min_prefix_tokens` | `integer` | no | `1024` | - |
| `prompt_cache.providers.gemini.mode` | `string` | no | `"implicit"` | Gemini prompt caching mode selection |
| `prompt_cache.providers.moonshot.enabled` | `boolean` | no | `true` | - |
| `prompt_cache.providers.openai.enabled` | `boolean` | no | `true` | - |
| `prompt_cache.providers.openai.idle_expiration_seconds` | `integer` | no | `3600` | - |
| `prompt_cache.providers.openai.min_prefix_tokens` | `integer` | no | `1024` | - |
| `prompt_cache.providers.openai.prompt_cache_retention` | `null \| string` | no | `null` | Optional prompt cache retention string to pass directly into OpenAI Responses API Example: "24h" or "1d". If set, VT Code will include `prompt_cache_retention` in the request body to extend the model-side prompt caching window. |
| `prompt_cache.providers.openai.surface_metrics` | `boolean` | no | `true` | - |
| `prompt_cache.providers.openrouter.enabled` | `boolean` | no | `true` | - |
| `prompt_cache.providers.openrouter.propagate_provider_capabilities` | `boolean` | no | `true` | Propagate provider cache instructions automatically |
| `prompt_cache.providers.openrouter.report_savings` | `boolean` | no | `true` | Surface cache savings reported by OpenRouter |
| `prompt_cache.providers.xai.enabled` | `boolean` | no | `true` | - |
| `prompt_cache.providers.zai.enabled` | `boolean` | no | `false` | - |
| `provider.anthropic.count_tokens_enabled` | `boolean` | no | `false` | Enable token counting via the count_tokens endpoint When enabled, the agent can estimate input token counts before making API calls Useful for proactive management of rate limits and costs |
| `provider.anthropic.effort` | `string` | no | `"low"` | Effort level for token usage (high, medium, low) Controls how many tokens Claude uses when responding, trading off between response thoroughness and token efficiency. Supported by Claude Opus 4.5/4.6 (4.5 requires effort beta header) |
| `provider.anthropic.extended_thinking_enabled` | `boolean` | no | `true` | Enable extended thinking feature for Anthropic models When enabled, Claude uses internal reasoning before responding, providing enhanced reasoning capabilities for complex tasks. Only supported by Claude 4, Claude 4.5, and Claude 3.7 Sonnet models. Claude 4.6 uses adaptive thinking instead of extended thinking. Note: Extended thinking is now auto-enabled by default (31,999 tokens). Set MAX_THINKING_TOKENS=63999 environment variable for 2x budget on 64K models. See: https://docs.anthropic.com/en/docs/build-with-claude/extended-thinking |
| `provider.anthropic.interleaved_thinking_beta` | `string` | no | `"interleaved-thinking-2025-05-14"` | Beta header for interleaved thinking feature |
| `provider.anthropic.interleaved_thinking_budget_tokens` | `integer` | no | `31999` | Budget tokens for extended thinking (minimum: 1024, default: 31999) On 64K output models (Opus 4.5, Sonnet 4.5, Haiku 4.5): default 31,999, max 63,999 On 32K output models (Opus 4): max 31,999 Use MAX_THINKING_TOKENS environment variable to override. |
| `provider.anthropic.interleaved_thinking_type_enabled` | `string` | no | `"enabled"` | Type value for enabling interleaved thinking |
| `provider.anthropic.skip_model_validation` | `boolean` | no | `false` | DEPRECATED: Model name validation has been removed. The Anthropic API validates model names directly, avoiding maintenance burden and allowing flexibility. This field is kept for backward compatibility but has no effect. |
| `provider.anthropic.tool_search.algorithm` | `string` | no | `"regex"` | Search algorithm: "regex" (Python regex patterns) or "bm25" (natural language) |
| `provider.anthropic.tool_search.always_available_tools` | `array` | no | `[]` | Tool names that should never be deferred (always available) |
| `provider.anthropic.tool_search.always_available_tools[]` | `string` | no | `-` | - |
| `provider.anthropic.tool_search.defer_by_default` | `boolean` | no | `true` | Automatically defer loading of all tools except core tools |
| `provider.anthropic.tool_search.enabled` | `boolean` | no | `false` | Enable tool search feature (requires advanced-tool-use-2025-11-20 beta) |
| `provider.anthropic.tool_search.max_results` | `integer` | no | `5` | Maximum number of tool search results to return |
| `pty.command_timeout_seconds` | `integer` | no | `300` | Command timeout in seconds (prevents hanging commands) |
| `pty.default_cols` | `integer` | no | `80` | Default terminal columns for PTY sessions |
| `pty.default_rows` | `integer` | no | `24` | Default terminal rows for PTY sessions |
| `pty.enabled` | `boolean` | no | `true` | Enable PTY support for interactive commands |
| `pty.large_output_threshold_kb` | `integer` | no | `5000` | Threshold (KB) at which to auto-spool large outputs to disk instead of memory |
| `pty.max_scrollback_bytes` | `integer` | no | `25000000` | Maximum bytes of output to retain per PTY session (prevents memory explosion) |
| `pty.max_sessions` | `integer` | no | `10` | Maximum number of concurrent PTY sessions |
| `pty.preferred_shell` | `null \| string` | no | `null` | Preferred shell program for PTY sessions (e.g. "zsh", "bash"); falls back to $SHELL |
| `pty.scrollback_lines` | `integer` | no | `400` | Total scrollback buffer size (lines) retained per PTY session |
| `pty.stdout_tail_lines` | `integer` | no | `20` | Number of recent PTY output lines to display in the chat transcript |
| `sandbox.default_mode` | `string` | no | `"read_only"` | Default sandbox mode |
| `sandbox.enabled` | `boolean` | no | `true` | Enable sandboxing for command execution |
| `sandbox.external.docker.cpu_limit` | `string` | no | `""` | CPU limit for container |
| `sandbox.external.docker.image` | `string` | no | `"ubuntu:22.04"` | Docker image to use |
| `sandbox.external.docker.memory_limit` | `string` | no | `""` | Memory limit for container |
| `sandbox.external.docker.network_mode` | `string` | no | `"none"` | Network mode |
| `sandbox.external.microvm.kernel_path` | `string` | no | `""` | Kernel image path |
| `sandbox.external.microvm.memory_mb` | `integer` | no | `512` | Memory size in MB |
| `sandbox.external.microvm.rootfs_path` | `string` | no | `""` | Root filesystem path |
| `sandbox.external.microvm.vcpus` | `integer` | no | `1` | Number of vCPUs |
| `sandbox.external.microvm.vmm` | `string` | no | `""` | VMM to use (firecracker, cloud-hypervisor) |
| `sandbox.external.sandbox_type` | `string` | no | `"none"` | Type of external sandbox |
| `sandbox.network.allow_all` | `boolean` | no | `false` | Allow any network access (legacy mode) |
| `sandbox.network.allowlist` | `array` | no | `[]` | Domain allowlist for network egress Following field guide: "Default-deny outbound network, then allowlist." |
| `sandbox.network.allowlist[].domain` | `string` | yes | `-` | Domain pattern (e.g., "api.github.com", "*.npmjs.org") |
| `sandbox.network.allowlist[].port` | `integer` | no | `443` | Port (defaults to 443) |
| `sandbox.network.block_all` | `boolean` | no | `false` | Block all network access (overrides allowlist) |
| `sandbox.resource_limits.cpu_time_secs` | `integer` | no | `0` | Custom CPU time limit in seconds (0 = use preset) |
| `sandbox.resource_limits.max_disk_mb` | `integer` | no | `0` | Custom disk write limit in MB (0 = use preset) |
| `sandbox.resource_limits.max_memory_mb` | `integer` | no | `0` | Custom memory limit in MB (0 = use preset) |
| `sandbox.resource_limits.max_pids` | `integer` | no | `0` | Custom max processes (0 = use preset) |
| `sandbox.resource_limits.preset` | `string` | no | `"moderate"` | Preset resource limits profile |
| `sandbox.resource_limits.timeout_secs` | `integer` | no | `0` | Custom wall clock timeout in seconds (0 = use preset) |
| `sandbox.seccomp.additional_blocked` | `array` | no | `[]` | Additional syscalls to block |
| `sandbox.seccomp.additional_blocked[]` | `string` | no | `-` | - |
| `sandbox.seccomp.enabled` | `boolean` | no | `true` | Enable seccomp filtering (Linux only) |
| `sandbox.seccomp.log_only` | `boolean` | no | `false` | Log blocked syscalls instead of killing process (for debugging) |
| `sandbox.seccomp.profile` | `string` | no | `"strict"` | Seccomp profile preset |
| `sandbox.sensitive_paths.additional` | `array` | no | `[]` | Additional paths to block |
| `sandbox.sensitive_paths.additional[]` | `string` | no | `-` | - |
| `sandbox.sensitive_paths.exceptions` | `array` | no | `[]` | Paths to explicitly allow (overrides defaults) |
| `sandbox.sensitive_paths.exceptions[]` | `string` | no | `-` | - |
| `sandbox.sensitive_paths.use_defaults` | `boolean` | no | `true` | Use default sensitive paths (SSH, AWS, etc.) |
| `security.auto_apply_detected_patches` | `boolean` | no | `false` | Automatically apply detected patch blocks in assistant replies when no write tool was executed. Defaults to false for safety. |
| `security.encrypt_payloads` | `boolean` | no | `false` | Encrypt payloads passed across executors. |
| `security.gatekeeper.auto_clear_paths` | `array` | no | `[".vtcode/bin", "~/.vtcode/bin"]` | Paths eligible for quarantine auto-clear |
| `security.gatekeeper.auto_clear_paths[]` | `string` | no | `-` | - |
| `security.gatekeeper.auto_clear_quarantine` | `boolean` | no | `false` | Attempt to clear quarantine automatically (opt-in) |
| `security.gatekeeper.warn_on_quarantine` | `boolean` | no | `true` | Warn when a quarantined executable is detected |
| `security.hitl_notification_bell` | `boolean` | no | `true` | Play terminal bell notification when HITL approval is required. |
| `security.human_in_the_loop` | `boolean` | no | `true` | Require human confirmation for critical actions |
| `security.integrity_checks` | `boolean` | no | `true` | Enable runtime integrity tagging for critical paths. |
| `security.require_write_tool_for_claims` | `boolean` | no | `true` | Require a successful write tool before accepting claims like "I've updated the file" as applied. When true, such claims are treated as proposals unless a write tool executed successfully. |
| `security.zero_trust_mode` | `boolean` | no | `false` | Enable zero-trust checks between components. |
| `skills.enable-auto-trigger` | `boolean` | no | `true` | Enable auto-trigger on $skill-name mentions |
| `skills.enable-description-matching` | `boolean` | no | `true` | Enable description-based keyword matching for auto-trigger |
| `skills.max-skills-in-prompt` | `integer` | no | `10` | Maximum number of skills to show in system prompt |
| `skills.min-keyword-matches` | `integer` | no | `2` | Minimum keyword matches required for description-based trigger |
| `skills.prompt-format` | `string` | no | `"xml"` | Prompt format for skills section (Agent Skills spec) - "xml": XML wrapping for safety (Claude models default) - "markdown": Plain markdown sections |
| `skills.render-mode` | `string` | no | `"lean"` | Rendering mode for skills in system prompt - "lean": Codex-style minimal (name + description + path only, 40-60% token savings) - "full": Full metadata with version, author, native flags |
| `subagents.additional_agent_dirs` | `array` | no | `[]` | Additional directories to search for subagent definitions |
| `subagents.additional_agent_dirs[]` | `string` | no | `-` | - |
| `subagents.default_model` | `null \| string` | no | `null` | Default model for subagents (if not specified in subagent config) |
| `subagents.default_timeout_seconds` | `integer` | no | `300` | Default timeout for subagent execution (seconds) |
| `subagents.enabled` | `boolean` | no | `false` | Enable the subagent system |
| `subagents.max_concurrent` | `integer` | no | `3` | Maximum concurrent subagents |
| `syntax_highlighting.cache_themes` | `boolean` | no | `true` | Enable theme caching for better performance |
| `syntax_highlighting.enabled` | `boolean` | no | `true` | Enable syntax highlighting for tool output |
| `syntax_highlighting.enabled_languages` | `array` | no | `["rust", "python", "javascript", "typescript", "go", "java", "cpp", "c", "php", "html", "css", "sql", "csharp", "bash...` | Languages to enable syntax highlighting for |
| `syntax_highlighting.enabled_languages[]` | `string` | no | `-` | - |
| `syntax_highlighting.highlight_timeout_ms` | `integer` | no | `5000` | Performance settings - highlight timeout in milliseconds |
| `syntax_highlighting.max_file_size_mb` | `integer` | no | `10` | Maximum file size for syntax highlighting (in MB) |
| `syntax_highlighting.theme` | `string` | no | `"base16-ocean.dark"` | Theme to use for syntax highlighting |
| `telemetry.bottleneck_tracing` | `boolean` | no | `false` | Emit bottleneck traces for slow paths |
| `telemetry.dashboards_enabled` | `boolean` | no | `true` | Enable real-time dashboards |
| `telemetry.perf_events` | `boolean` | no | `true` | Emit performance events for file I/O, spawns, and UI latency |
| `telemetry.retention_days` | `integer` | no | `14` | Retention window for historical benchmarking (days) |
| `telemetry.sample_interval_ms` | `integer` | no | `1000` | KPI sampling interval in milliseconds |
| `telemetry.trajectory_enabled` | `boolean` | no | `true` | - |
| `timeouts.adaptive_decay_ratio` | `number` | no | `0.875` | Adaptive timeout decay ratio (0.1-1.0). Lower relaxes faster back to ceiling. |
| `timeouts.adaptive_min_floor_ms` | `integer` | no | `1000` | Minimum timeout floor in milliseconds when applying adaptive clamps. |
| `timeouts.adaptive_success_streak` | `integer` | no | `5` | Number of consecutive successes before relaxing adaptive ceiling. |
| `timeouts.default_ceiling_seconds` | `integer` | no | `180` | Maximum duration (in seconds) for standard, non-PTY tools. |
| `timeouts.mcp_ceiling_seconds` | `integer` | no | `120` | Maximum duration (in seconds) for MCP calls. |
| `timeouts.pty_ceiling_seconds` | `integer` | no | `300` | Maximum duration (in seconds) for PTY-backed commands. |
| `timeouts.streaming_ceiling_seconds` | `integer` | no | `600` | Maximum duration (in seconds) for streaming API responses. |
| `timeouts.warning_threshold_percent` | `integer` | no | `80` | Percentage (0-100) of the ceiling after which the UI should warn. |
| `tools.default_policy` | `string` | no | `"prompt"` | Default policy for tools not explicitly listed |
| `tools.loop_thresholds` | `object` | no | `{}` | Tool-specific loop thresholds (Adaptive Loop Detection) Allows setting higher loop limits for read-only tools (e.g., ls, grep) and lower limits for mutating tools. |
| `tools.loop_thresholds.*` | `integer` | no | `-` | - |
| `tools.max_repeated_tool_calls` | `integer` | no | `3` | Maximum number of times the same tool invocation can be retried with the identical arguments within a single turn. |
| `tools.max_tool_loops` | `integer` | no | `200` | Maximum inner tool-call loops per user turn Prevents infinite tool-calling cycles in interactive chat. This limits how many back-and-forths the agent will perform executing tools and re-asking the model before returning a final answer. |
| `tools.max_tool_rate_per_second` | `integer \| null` | no | `null` | Optional per-second rate limit for tool calls to smooth bursty retries. When unset, the runtime defaults apply. |
| `tools.plugins.allow` | `array` | no | `[]` | Explicit allow-list of plugin identifiers permitted to load. |
| `tools.plugins.allow[]` | `string` | no | `-` | - |
| `tools.plugins.auto_reload` | `boolean` | no | `false` | Enable hot-reload polling for manifests to support rapid iteration. |
| `tools.plugins.default_trust` | `string` | no | `"sandbox"` | Default trust level when a manifest omits trust metadata. |
| `tools.plugins.deny` | `array` | no | `[]` | Explicit block-list of plugin identifiers that must be rejected. |
| `tools.plugins.deny[]` | `string` | no | `-` | - |
| `tools.plugins.enabled` | `boolean` | no | `true` | Toggle the plugin runtime. When disabled, manifests are ignored. |
| `tools.plugins.manifests` | `array` | no | `[]` | Manifest paths (files or directories) that should be scanned for plugins. |
| `tools.plugins.manifests[]` | `string` | no | `-` | - |
| `tools.policies` | `object` | no | `{}` | Specific tool policies |
| `tools.policies.*` | `string` | no | `-` | Tool execution policy |
| `tools.web_fetch.allowed_domains` | `array` | no | `[]` | Inline whitelist - Domains to allow in restricted mode |
| `tools.web_fetch.allowed_domains[]` | `string` | no | `-` | - |
| `tools.web_fetch.audit_log_path` | `string` | no | `""` | Path to audit log file |
| `tools.web_fetch.blocked_domains` | `array` | no | `[]` | Inline blocklist - Additional domains to block |
| `tools.web_fetch.blocked_domains[]` | `string` | no | `-` | - |
| `tools.web_fetch.blocked_patterns` | `array` | no | `[]` | Additional blocked patterns |
| `tools.web_fetch.blocked_patterns[]` | `string` | no | `-` | - |
| `tools.web_fetch.dynamic_blocklist_enabled` | `boolean` | no | `false` | Enable dynamic blocklist loading from external file |
| `tools.web_fetch.dynamic_blocklist_path` | `string` | no | `""` | Path to dynamic blocklist file |
| `tools.web_fetch.dynamic_whitelist_enabled` | `boolean` | no | `false` | Enable dynamic whitelist loading from external file |
| `tools.web_fetch.dynamic_whitelist_path` | `string` | no | `""` | Path to dynamic whitelist file |
| `tools.web_fetch.enable_audit_logging` | `boolean` | no | `false` | Enable audit logging of URL validation decisions |
| `tools.web_fetch.mode` | `string` | no | `"restricted"` | Security mode: "restricted" (blocklist) or "whitelist" (allowlist) |
| `tools.web_fetch.strict_https_only` | `boolean` | no | `true` | Strict HTTPS-only mode |
| `ui.allow_tool_ansi` | `boolean` | no | `false` | Allow ANSI escape sequences in tool output (enables colors but may cause layout issues) |
| `ui.bold_is_bright` | `boolean` | no | `false` | Compatibility mode for legacy terminals that map bold to bright colors. When enabled, avoids using bold styling on text that would become bright colors, preventing visibility issues in terminals with "bold is bright" behavior. |
| `ui.color_scheme_mode` | `string` | no | `"auto"` | Color scheme mode for automatic light/dark theme switching. - "auto": Detect from terminal (via OSC 11 or COLORFGBG env var) - "light": Force light mode theme selection - "dark": Force dark mode theme selection |
| `ui.dim_completed_todos` | `boolean` | no | `true` | Dim completed todo items (- [x]) in agent output |
| `ui.display_mode` | `string` | no | `"minimal"` | UI display mode preset (full, minimal, focused) |
| `ui.inline_viewport_rows` | `integer` | no | `16` | Number of rows to allocate for inline UI viewport |
| `ui.keyboard_protocol.disambiguate_escape_codes` | `boolean` | no | `true` | Resolve Esc key ambiguity (recommended for performance) |
| `ui.keyboard_protocol.enabled` | `boolean` | no | `true` | Enable keyboard protocol enhancements (master toggle) |
| `ui.keyboard_protocol.mode` | `string` | no | `"default"` | Preset mode: "default", "full", "minimal", or "custom" |
| `ui.keyboard_protocol.report_all_keys` | `boolean` | no | `false` | Report all keys, including modifier-only keys (Shift, Ctrl) |
| `ui.keyboard_protocol.report_alternate_keys` | `boolean` | no | `true` | Report alternate key layouts (e.g. for non-US keyboards) |
| `ui.keyboard_protocol.report_event_types` | `boolean` | no | `true` | Report press, release, and repeat events |
| `ui.layout_mode` | `string` | no | `"auto"` | Override the responsive layout mode |
| `ui.message_block_spacing` | `boolean` | no | `true` | Add spacing between message blocks |
| `ui.minimum_contrast` | `number` | no | `4.5` | Minimum contrast ratio for text against background (WCAG 2.1 standard) - 4.5: WCAG AA (default, suitable for most users) - 7.0: WCAG AAA (enhanced, for low-vision users) - 3.0: Large text minimum - 1.0: Disable contrast enforcement |
| `ui.reasoning_display_mode` | `string` | no | `"toggle"` | Reasoning display mode for chat UI ("always", "toggle", or "hidden") |
| `ui.reasoning_visible_default` | `boolean` | no | `false` | Default visibility for reasoning when display mode is "toggle" |
| `ui.reduce_motion_mode` | `boolean` | no | `false` | Reduce motion mode: minimizes shimmer/flashing animations. Can also be enabled via `VTCODE_REDUCE_MOTION=1`. |
| `ui.reduce_motion_keep_progress_animation` | `boolean` | no | `false` | Keep animated progress indicators while `ui.reduce_motion_mode` is enabled. |
| `ui.screen_reader_mode` | `boolean` | no | `false` | Screen reader mode: disables animations, uses plain text indicators, and optimizes output for assistive technology compatibility. Can also be enabled via `VTCODE_SCREEN_READER=1`. |
| `ui.safe_colors_only` | `boolean` | no | `false` | Restrict color palette to the 11 "safe" ANSI colors portable across common themes. Safe colors: red, green, yellow, blue, magenta, cyan + brred, brgreen, brmagenta, brcyan Problematic colors avoided: brblack (invisible in Solarized Dark), bryellow (light themes), white/brwhite (light themes), brblue (Basic Dark). See: https://blog.xoria.org/terminal-colors/ |
| `ui.show_sidebar` | `boolean` | no | `true` | Show the right sidebar (queue, context, tools) |
| `ui.status_line.command` | `null \| string` | no | `null` | - |
| `ui.status_line.command_timeout_ms` | `integer` | no | `200` | - |
| `ui.status_line.mode` | `string` | no | `"Auto"` | - |
| `ui.status_line.refresh_interval_ms` | `integer` | no | `1000` | - |
| `ui.tool_output_max_lines` | `integer` | no | `600` | Maximum number of lines to display in tool output (prevents transcript flooding) |
| `ui.tool_output_mode` | `string` | no | `"compact"` | Tool output display mode ("compact" or "full") |
| `ui.tool_output_spool_bytes` | `integer` | no | `200000` | Maximum bytes of output to display before auto-spooling to disk |
| `ui.tool_output_spool_dir` | `null \| string` | no | `null` | Optional custom directory for spooled tool output logs |
