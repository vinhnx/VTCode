scan for large monolithic files and functions and break them down into smaller, more focused functions that adhere to the Single Responsibility Principle. This will enhance readability, maintainability, and testability of the codebase.

---

it seems markdown systax higlight is wrong

'/Users/vinhnguyenxuan/Desktop/Screenshot 2026-04-09 at 12.09.03 PM.png'

---

1. fix `vtcode resume` should use cli `vtcode --resume`, don't send raw transcript to llm.

currently:

```
~/Developer/learn-by-doing/vtcode main* ⇡
15:56:34 ❯ vtcode resume
Clarify what you mean:

1. **Your resume** - paste a resume and I'll help with it
2. **My resume** - you're asking about my capabilities
3. **Resume a task** - you had an interrupted conversation
4. **Something else** - describe what you need

Which?
```

expected:

```

~/Developer/learn-by-doing/vtcode main* ⇡ 7s
15:56:51 ❯ vtcode --resume
✔ Select a session to resume
```

---

CRITICAL: don't accept raw cli repl arugment like this. every commands are being used as prompt and send to llm, which is not what we want. we should handle some command in the cli level, and only send relevant information to llm if needed. for example, for `vtcode resume`, we should handle the session selection in the cli, and then only send the selected session information to llm if necessary, instead of sending the entire command as a prompt.

```
vtcode hellp
How can I help? Describe what you're working on or what you need.
```

THIS IS CRITICAL because it can lead to unintended consequences, such as sending sensitive information to the LLM or causing confusion in the conversation. By properly handling commands at the CLI level, we can ensure a better user experience and maintain the integrity of the interactions with the LLM.

Double check clap command arg parser to make sure we are not sending raw command inputs to the LLM and that we are properly handling commands at the CLI level.

---

implement `resume --last` command. This command will allow users to quickly resume their last session without having to specify the session ID. It will check for the most recent session and automatically load it, providing a seamless experience for users who want to continue their work without interruption. This feature will enhance productivity and convenience for users, allowing them to easily pick up where they left off without having to navigate through multiple sessions or remember specific session IDs.

---

fix copy, yank text is broken block on scroll. for example: when user already select some text, then scroll the trascript up and down in the terminal, the highlight block is broken and becomes unaligned with the text, making it difficult for users to see what they have selected. This issue can be frustrating for users who rely on the copy and yank functionality to quickly capture important information from the transcript. To fix this, we need to ensure that the highlight block remains properly aligned with the selected text even when the user scrolls through the transcript. This may involve adjusting the way the terminal handles text rendering and selection, ensuring that the highlight block is dynamically updated as the user scrolls, and testing across different terminal environments to ensure consistent behavior. By addressing this issue, we can improve the overall user experience and make it easier for users to interact with the transcript effectively.

---

---

remove branch and tools count.

            if let Some(body) = trimmed.strip_prefix(ui::HEADER_TOOLS_PREFIX) {
                let compact_tools = compact_tools_format(body.trim());
                values.push(format!("Tools: {}", compact_tools));
                continue;
            }

            if let Some(body) = trimmed.strip_prefix(ui::HEADER_GIT_PREFIX) {
                let body = body.trim();
                if !body.is_empty() {
                    values.push(format!("⎇ {}", body));
                }
                continue;
            }

from header

---

revamp and cleanup input placeholder text hint:

pub const CHAT_INPUT_PLACEHOLDER_BOOTSTRAP: &str = "Type your message, or @files, /commands, Alt+P: suggest, Shift+Tab: choose Edit/Auto/Plan, Enter: queue, Tab: accept/queue, Ctrl+Enter: run/steer now, /stop: stop task";
pub const CHAT_INPUT_PLACEHOLDER_FOLLOW_UP: &str = "Continue, or @files, /commands, Alt+P: suggest, Shift+Tab: choose Edit/Auto/Plan, Enter: queue, Tab: accept/queue, Ctrl+Enter: run/steer now, /stop: stop task";
pub const HEADER_SHORTCUT_HINT: &str = "Shortcuts: Alt+P suggest • Enter queue • Tab accept/queue • Ctrl+Enter run/steer • Shift+Enter newline • Esc cancel • Ctrl+C interrupt";

---

in header. if memory is on -> show status on header instead of "/memory"

---

check for vtcode.toml, any special fields is enabled, and show in header. For example, if [memory] is enabled, show "Memory: On" in header. If [optimization] is enabled, show "Optimization: Bandit" or "Optimization: Actor-Critic" based on the strategy. Make it concise and clean. also looks for any custom hint and tips for users in the config and show in header if exists. Also show useful commands and hints in the header for user reference.

---

apply to protect vtcode repo (https://github.com/vinhnx/VTCode)

https://astral.sh/blog/open-source-security-at-astral

---

improve --help with clap cli, cleanup commands if needed and revise the wording quick start, improve color

```

16:52:58 ❯ cargo run -- --help
    Finished `dev` profile [unoptimized] target(s) in 0.70s
     Running `target/debug/vtcode --help`
Quick start:
  1. Set your API key: export ANTHROPIC_API_KEY="your_key"
  2. Run: vtcode chat
  3. First-time setup will run automatically

For help: vtcode --help

VT Code - AI coding assistant

Usage: vtcode [OPTIONS] [WORKSPACE] [COMMAND]

Commands:
  acp                Start Agent Client Protocol bridge for IDE integrations
  chat               Interactive AI coding assistant
  ask                Single prompt mode - prints model reply without tools
  exec               Headless execution mode
  schedule           Manage durable scheduled tasks
  review             Headless code review for the current diff, selected files, or a custom git target
  schema             Runtime schema introspection for built-in tools
  chat-verbose       Verbose interactive chat with debug output
  analyze            Analyze workspace (structure, security, performance)
  trajectory         Pretty-print trajectory logs
  notify             Send a VT Code notification using the built-in notification system
  benchmark          Benchmark against SWE-bench evaluation framework
  create-project     Create complete Rust project
  revert             Revert agent to a previous snapshot
  snapshots          List all available snapshots
  cleanup-snapshots  Clean up old snapshots
  init               Initialize project guidance and workspace scaffolding
  init-project       Initialize project in ~/.vtcode/projects/
  config             Generate configuration file
  login              Authenticate with a supported provider
  logout             Clear stored authentication credentials for a provider
  auth               Show authentication status for one provider or all supported providers
  tool-policy        Manage tool execution policies
  mcp                Manage Model Context Protocol providers
  a2a                Agent2Agent (A2A) Protocol
  app-server         Proxy to the official Codex app-server
  models             Manage models and providers
  pods               Manage GPU pod deployments
  man                Generate or display man pages
  skills             Manage Agent Skills
  list-skills        List available skills (alias for `vtcode skills list`)
  dependencies       Manage optional VT Code dependencies [aliases: deps]
  check              Run built-in repository checks
  update             Check for and install binary updates from GitHub Releases
  anthropic-api      Start Anthropic API compatibility server
  help               Print this message or the help of the given subcommand(s)

Arguments:
  [WORKSPACE]  Optional positional path to run vtcode against a different workspace

Options:
      --color <WHEN>
          Controls when to use color [default: auto] [possible values: auto, always, never]
      --model <MODEL>
          LLM Model ID (e.g., gpt-5, claude-sonnet-4-5, gemini-3-flash-preview)
      --provider <PROVIDER>
          LLM Provider (gemini, openai, anthropic, deepseek, openrouter, codex, zai, moonshot, minimax, ollama, lmstudio)
      --api-key-env <API_KEY_ENV>
          API key environment variable (auto-detects GEMINI_API_KEY, OPENAI_API_KEY, etc.) [default: OPENAI_API_KEY]
      --workspace <PATH>
          Workspace root directory (default: current directory)
      --research-preview
          Enable research-preview features
      --security-level <SECURITY_LEVEL>
          Security level for tool execution (strict, moderate, permissive) [default: moderate]
      --show-file-diffs
          Show diffs for file changes in chat interface
      --max-concurrent-ops <MAX_CONCURRENT_OPS>
          Maximum concurrent async operations [default: 5]
      --api-rate-limit <API_RATE_LIMIT>
          Maximum API requests per minute [default: 30]
      --max-tool-calls <MAX_TOOL_CALLS>
          Maximum tool calls per session [default: 10]
      --debug
          Enable debug output for troubleshooting
      --verbose
          Enable verbose logging
  -q, --quiet
          Suppress all non-essential output (for scripting, CI/CD)
  -c, --config <KEY=VALUE|PATH>
          Configuration overrides or file path (KEY=VALUE or PATH)
      --log-level <LOG_LEVEL>
          Log level (error, warn, info, debug, trace) [default: info]
      --no-color
          Disable color output (for log files, CI/CD)
      --theme <THEME>
          Select UI theme (e.g., ciapre-dark, ciapre-blue)
  -t, --tick-rate <TICK_RATE>
          App tick rate in milliseconds (default: 250) [default: 250]
  -f, --frame-rate <FRAME_RATE>
          Frame rate in FPS (default: 60) [default: 60]
      --enable-skills
          Enable skills system
      --chrome
          Enable Chrome browser integration for web automation
      --no-chrome
          Disable Chrome browser integration
      --skip-confirmations
          Skip safety confirmations (use with caution)
      --codex-experimental
          Enable experimental Codex app-server features for this run
      --no-codex-experimental
          Disable experimental Codex app-server features for this run
  -p, --print [<PROMPT>]
          Print response without launching the interactive TUI
      --full-auto [<PROMPT>]
          Enable full-auto mode (no interaction) or run a headless task
  -r, --resume [<SESSION_ID>]
          Resume a previous conversation (use without ID for interactive picker)
      --continue
          Continue the most recent conversation automatically [aliases: --continue-session]
      --fork-session <SESSION_ID>
          Fork an existing session with a new session ID
      --all
          Show archived sessions from every workspace when resuming or forking
      --session-id <CUSTOM_SUFFIX>
          Custom suffix for session identifier (alphanumeric, dash, underscore only, max 64 chars)
      --summarize
          Use summarized history when forking a session
      --agent <AGENT>
          Override the default agent model for this session
      --allowed-tools <TOOLS>
          Tools that execute without prompting (comma-separated, supports patterns like "Bash(git:*)")
      --disallowed-tools <TOOLS>
          Tools that cannot be used by the agent
      --dangerously-skip-permissions
          Skip all permission prompts (reduces security - use with caution)
      --ide
          Explicitly connect to IDE on startup (auto-detects available IDEs)
      --permission-mode <MODE>
          Begin in a specified permission mode (default, accept_edits, auto, dont_ask, bypass_permissions, plus legacy ask/suggest/auto-approved/full-auto/trusted_auto/plan)
  -h, --help
          Print help
  -V, --version
          Print version



Slash commands (type / in chat):
  /init     - Guided AGENTS.md + workspace setup
  /config   - Browse settings sections
  /status   - Show current configuration
  /doctor   - Diagnose setup issues (inline picker, or use --quick/--full)
  /update   - Check for VT Code updates (use --list, --pin, --channel)
  /plan     - Toggle read-only planning mode
  /loop     - Schedule a recurring prompt in this session
  /schedule - Open the durable scheduled-task manager
  /theme    - Switch UI theme
  /title    - Configure terminal title items
  /history  - Open command history picker
  /help     - Show all slash commands
``
```
