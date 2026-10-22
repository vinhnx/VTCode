# Command Reference

This guide summarizes common actions and how to invoke them with vtcode. The agent exposes a suite of tools to the LLM;
you interact with them via chat. When you ask to search, read, or edit files, the agent chooses an appropriate tool.

## Core command reference

`vtcode --help` lists every subcommand, including niche utilities such as `benchmark`, `notify`, and `session-store`.
The table below maps the primary subcommands to their purpose; detailed sections for the most-used commands follow.

| Command                                                                | Purpose                                                             |
| ---------------------------------------------------------------------- | ------------------------------------------------------------------- |
| `vtcode` / `vtcode chat`                                               | Interactive TUI session                                             |
| `vtcode ask <prompt>`                                                  | One-shot prompt; prints the reply without tools or a session        |
| `vtcode exec <prompt>`                                                 | Headless execution with the full tool loop; `--json` streams events |
| `vtcode eval --suite <file>`                                           | Run an evaluation suite with pass@k / pass^k metrics                |
| `vtcode review`                                                        | Non-interactive review of the current diff, files, or a git target  |
| `vtcode continue`                                                      | Resume the most recent conversation; `--session-id` forks it        |
| `vtcode init`                                                          | Scaffold `vtcode.toml`, `AGENTS.md`, and ast-grep starter files     |
| `vtcode init-project`                                                  | Register a project entry in the user state directory                |
| `vtcode config`                                                        | Generate or reset `vtcode.toml` configuration layers                |
| `vtcode login <provider>` / `vtcode logout <provider>` / `vtcode auth` | OAuth or API-key login, logout, and status                          |
| `vtcode secret`                                                        | Store provider API keys in the OS keyring                           |
| `vtcode models`                                                        | List, set, test, and inspect providers and models                   |
| `vtcode schedule`                                                      | Durable scheduled tasks by cron, interval, or one-shot time         |
| `vtcode tool-policy`                                                   | Allow or deny specific tools per workspace                          |
| `vtcode mcp`                                                           | Manage Model Context Protocol servers                               |
| `vtcode a2a`                                                           | Agent2Agent protocol client and server                              |
| `vtcode acp`                                                           | Start the Agent Client Protocol bridge for editors                  |
| `vtcode webmcp`                                                        | Authenticated browser editor bridge and local MCP stdio service     |
| `vtcode skills`                                                        | Manage Agent Skills (list, create, load, validate)                  |
| `vtcode plugins`                                                       | Manage Agent Plugins (add, list, validate, remove)                  |
| `vtcode dependencies` (`deps`)                                         | Install or check optional tools (ripgrep, ast-grep)                 |
| `vtcode check`                                                         | Run built-in repository checks such as ast-grep                     |
| `vtcode schema`                                                        | Emit built-in tool schemas for automation discovery                 |
| `vtcode analyze`                                                       | Analyze workspace structure, security, and performance              |
| `vtcode trajectory`                                                    | Pretty-print run logs for debugging and audits                      |
| `vtcode snapshots` / `vtcode revert`                                   | List and roll back to workspace snapshots                           |
| `vtcode cleanup-snapshots`                                             | Prune old snapshots with a retention limit                          |
| `vtcode update`                                                        | Check for and install binary updates from GitHub Releases           |
| `vtcode man`                                                           | Generate or display man pages                                       |
| `vtcode man --all-pages --output man/man1`                             | Write the main page and all visible command pages to a directory    |

For a local MCP connection that exposes Companion coding tasks, use
`vtcode webmcp stdio --companion` with an explicitly selected workspace. The
[VT Code Companion guide](./vtcode-companion.md) covers package setup and host
marketplaces. The CLI command does not install the plugin or configure a host.

## Fast mode

| Command | Purpose |
| --- | --- |
| `/fast` | Toggle native OpenAI ultrafast or Anthropic fast on eligible models |
| `/fast on` | Enable fast processing at premium pricing and save the choice |
| `/fast off` | Clear the active provider override and restore its default speed |

Changes apply to subsequent requests and persist through the same configuration path as `/model`.
Enabling on another route explains which models to select; it does not switch models. Disabling also works after
switching to an unsupported route or ChatGPT subscription authentication. Each accepted command saves the choice and
rebuilds the client to reconcile configuration reloads; repeated settings produce the same configuration.
See the [fast mode guide](../development/fast-mode.md).

The TUI reminds you once per session about `/fast on` when using an eligible native API-key route and
fast mode is off. Using `/fast` dismisses the reminder. Claude speed changes affect prompt-cache reuse.

## Execution explanations

Enter these slash commands in the interactive VT Code chat input. They summarize recorded execution without another
model call.

| Command | Purpose |
| --- | --- |
| `/explain` | Latest task's goal, changes, decisions, verification, and review priorities |
| `/explain --scope task` | Explicitly select the latest task (the default) |
| `/explain --scope session` | Explain all retained tasks |
| `/explain --details` | Inspect public facts and evidence in Transcript Review |
| `/explain diagram` | Show recorded execution order and file relationships |
| `/explain diagram --scope session` | Show execution relationships across retained tasks |
| `/explain --web` | Open the live Explanation view, or create an offline report |
| `/explain --export html` | Export a private standalone HTML report |

Choose one output mode: the summary, `--details`, `diagram`, `--web`, or `--export html`. Add `--scope task` or
`--scope session` to any mode. The default summary has five sections and at most 20 logical lines; wrapped terminal
text can occupy more rows. Omission counts point to additional facts available in the detailed views.

### Review a task

1. Run `/explain` after the agent finishes. Read **Goal** for the outcome and **Review first** for inspection priorities.
2. Run `/explain --details` for the full public facts. With mouse capture enabled, Alt+click a fact in Transcript Review
   to inspect its recorded evidence. Ordinary click and drag still select text. Use the configured review binding
   (default `Ctrl+T`) to close the viewer.
3. Run `/explain diagram` to inspect recorded action order, failures, delegation, and file relationships.

The latest task includes its initial request, active corrections, approval handoffs, retries, and autonomous
continuations. A new request entered while the agent is idle starts a new task. Use `--scope session` to include earlier
retained tasks.

### Interpret the results

- **Changes** attributes successful recorded file operations to the agent. A separate current workspace diff includes
  staged and unstaged tracked changes against HEAD; it does not establish who made them and excludes untracked files.
- **Decisions** shows optional public rationale reported by the agent. Missing rationale remains unavailable.
- **Verification** passes only for a completed recorded check with exit code zero. Pending checks and absent exit codes
  remain unconfirmed. Checks before later mutation attempts are marked stale, including failed or pending mutations.
- **Review first** lists deterministic inspection signals with reasons and evidence. A priority is not a proven bug or
  a claim of test coverage.
- Missing, malformed, evicted, or unavailable evidence is reported explicitly. Usage, cost, timing, and ancestry appear
  only when recorded.

### Open or save a report

`/explain --web` opens the existing WebMCP app's Explanation view when the live bridge is active. Complete browser
pairing if needed; see the [WebMCP guide](./webmcp.md#execution-explanations). Without an active bridge, the command
creates and opens an offline HTML report.

`/explain --export html` saves a report without opening the browser and prints its path:
`.vtcode/sessions/<session-id>/derived/explain/explanation-<id>.html`. Open that file directly in a browser. It embeds
redacted public facts and available evidence, supports local evidence navigation, and needs no network connection or
pairing credentials. Large or expired evidence is marked incomplete. A browser-opening failure leaves the report at
the printed path.

See the [development guide](../development/execution-explanations.md) for event and evidence limits.

## Headless / CI

`exec`, `ask --print`, and `review` are safe with no TTY. `stdout` carries only the result (`--json` streams events);
diagnostics go to `stderr`. Failures exit non-zero and never block on a prompt.

```bash
vtcode exec "summarize this repo" </dev/null
echo "fix the failing test" | vtcode exec --json - | jq .
VTCODE_TRUST_WORKSPACE=full-auto vtcode exec "run the checks" --workspace /tmp/work
vtcode ask "what is a monad?" </dev/null
NO_COLOR=1 vtcode --print "summarize this repo" | cat
```

Workspace trust is fail-closed: untrusted workspaces error out. Grant once interactively, or set
`VTCODE_TRUST_WORKSPACE=full-auto` in CI (`VTCODE_TRUST_WORKSPACE_QUIET=1` silences the notice). Missing credentials
error out with `vtcode secret add <provider>` / env-var guidance; no silent provider fallback. `[automation.full_auto]`
must be enabled for `exec`.

## Search

Use `exec_command.cmd` with `rg` or `grep` for flexible shell text search.

The default model-visible tools are `exec_command`, `write_stdin`, and `apply_patch`. `code_search` is available through
the advanced VT Code profile. It accepts a required literal `query` plus optional `path`, `file_types`, `result_types`,
and `max_results`. Its four result categories are recognised definitions, exact syntactic usages, literal text, and
matching paths. A usage is a same-spelling syntax occurrence, not a resolved reference. Queries use literal smart-case.
When `truncated` is true, narrow a filter in another call; the response does not claim an exact repository-wide total.

Use `exec_command` or the specialised ast-grep skill for arbitrary structural patterns.

## Storage diagnostics

Use `vtcode --version` to print the active config, data, state, cache, runtime, executable, and legacy roots, as well as
the migration marker and report paths. This is the quickest way to diagnose an XDG override, a sandbox environment, or a
legacy `~/.vtcode` migration without relying on platform-specific guesses.

### Examples

- Find TODO/FIXME with 2 lines of context in Rust files only:

```text
Ask: Search for TODO or FIXME across the repo with 2 lines of context in Rust files.
{
  "cmd": "rg -n -C 2 'TODO|FIXME' -g '*.rs' ."
}
```

- Literal search for `unsafe {` anywhere (hidden files ignored):

```text
{
  "cmd": "rg -n -C 1 -F 'unsafe {' ."
}
```

- Search JavaScript files for a function name, case-insensitive:

```text
{
  "cmd": "rg -n -i 'doSomethingImportant' -g '*.js' ."
}
```

## File operations

- Inspect files with shell commands through `exec_command.cmd`, such as `rg --files`, `sed -n`, `cat`, `head`, and
  `tail`.
- Edit files with `apply_patch`.
- Continue live shell sessions with `write_stdin`.

## Session resume and forks

VT Code can reopen archived sessions, continue the latest one, or fork a previous session into a new archive.

### Resume the latest or a specific session

```bash
vtcode --continue
vtcode --resume session-123
vtcode --resume          # interactive picker
```

### Fork from an archived session

```bash
vtcode --fork-session session-123
vtcode --fork-session session-123 --session-id bugfix-branch
vtcode --resume session-123 --session-id bugfix-branch
vtcode --continue --session-id followup-branch
```

Notes:

- `--session-id` turns `--resume ...` or `--continue` into a fork instead of an in-place resume.
- `--resume` with no ID plus `--session-id ...` opens the interactive picker and then forks the selected session.
- `--all` expands the picker/search scope across workspaces for resume and fork flows.

### Start a summarized fork

```bash
vtcode --fork-session session-123 --summarize
vtcode --resume session-123 --session-id handoff --summarize
vtcode --resume --session-id handoff --summarize   # interactive picker, summarized fork
```

Summarized forks do not copy the full transcript. VT Code starts the child session from:

- one structured conversation summary
- retained recent real user messages
- the session memory envelope

Normal forks keep the full archived transcript unchanged.

## Quick Actions in Chat Input

VT Code provides several quick actions directly in the chat input for faster workflow:

- **File Picker (`@`)** — Type `@` anywhere in your input to open the file picker and select files to reference in your
  message. This allows you to quickly mention files without typing full paths.

  The picker is hierarchical: with an empty query it browses one directory at a time (`Enter` opens a folder, `←`
  ascends, `PageUp`/`PageDown` jump a page), and once you type it switches to a fuzzy search across the workspace. Rows
  show a type glyph (`▸` folder, `▧` image, `⚙` executable) and symlinks render their target, with dangling links
  flagged as `(broken)`. Press `Alt+Enter` on a folder to reference it (`@dir/`) instead of opening it. Files and
  folders ignored by `.gitignore`, `.vtcodegitignore`, and the built-in sensitive-file list are never shown.

- **Slash Commands (`/`)** — Type `/` at the start of input to access all available slash commands including `/files`,
  `/status`, and many more.

## Configuration quick reference

| Action                         | Command                         | Result                                                                       |
| ------------------------------ | ------------------------------- | ---------------------------------------------------------------------------- |
| Open settings                  | `/config` (or `/settings`)      | Browse curated groups, or search the complete schema under Advanced settings |
| Open a section or field        | `/config <path>`                | Jump directly to a group, Advanced index, or nested setting                  |
| Reset the active layer         | `/config reset`                 | Show the target file and request confirmation                                |
| Generate configuration         | `vtcode config`                 | Print a generated configuration document                                     |
| Reset workspace/explicit layer | `vtcode config reset`           | Clear one workspace-layer file and reload the stack                          |
| Reset user layer               | `vtcode config reset --global`  | Clear the canonical user config only                                         |
| Reset project layer            | `vtcode config reset --project` | Clear the current project profile only                                       |

Configuration resets preserve lower-precedence values and secure credentials. Interactive sessions live-reload safe
changes from watched layers; malformed edits retain the last valid snapshot and show a warning. See the
[configuration guide](../config/config.md) for layer precedence and live-reload details.

### Slash-command notes

- Slash commands are skill-backed. Each command routes through a namespaced command skill such as `cmd-status` or
  `cmd-review`.
- The `/name` form remains the compatibility alias. You can also inspect or execute the same behavior through
  `/skills info cmd-name` and `/skills use cmd-name ...`.
- Prompt-oriented slash commands such as `/review` are shipped as bundled system skills in the release binary.
- `/review` accepts free-form instructions (`/review Review the full diff for correctness and regressions`) or legacy
  flags (`--last-diff`, `--target <expr>`, `--file <path>`, positional files, `--style <style>`). Instructions describe
  focus only and are never used as shell arguments. The command stays read-only unless the text explicitly asks to
  implement fixes.
- To keep the default prompt lean, command skills are not injected into the runtime `## Skills` prompt section; use
  slash completion or `/skills` discovery when you need them.
- `/resume` opens archived sessions when the current run is idle.
- `/fork` opens the session picker and then lets you choose between a full-copy fork and a summarized fork.
- `/compact` manually compacts the current conversation context immediately. The UI shows `Compacting context...` async
  with elapsed time, then `Context compacted · {duration} ({orig} -> {compacted} messages, {mode} compaction)`. Use
  `/compact edit-prompt` or `/compact reset-prompt` to manage the saved default prompt for manual compaction requests.
  On the local fallback path, VT Code keeps a structured summary plus retained user prompts instead of a mixed recent
  tail.
- `/agent` inspects agent definitions and delegated child runs. `@agent-name` remains a delegated child-agent control.
  Primary agents are switched from the TUI with `Shift+Tab` (see
  [Keyboard Shortcuts](./keyboard-shortcuts.md#agent-and-mode-switching)). The active primary agent is shown in the
  session header badge and influences the session's instructions, model, granular permission policy, and tool access.
- `/agent list` shows all agent definitions with their availability (`mode: primary`, `mode: subagent`, or `mode: all`).
  `/agent create` scaffolds a new agent definition in `.vtcode/agents/`.
- `/plan` starts or continues the planning workflow and switches to the `plan` primary agent so the header badge,
  prompt, and tools match Plan mode. Execution agents may also suggest it for demanding or multi-phase tasks;
  interactive policies confirm the suggestion, while full-auto and skip-confirmations policies accept it automatically.
  After confirmation the header shows Plan immediately. When the plan agent needs a material clarification, the inline
  interview wizard presents selectable answers and resumes planning with the chosen answer. Use `/plan off` to cancel an
  active planning workflow without implementing its draft and restore the previous execution agent (`build`/`auto`).
- `/config checkup` runs configuration diagnostics and suggests reversible optimizations. Use
  `/config checkup [--quick|--full]` (defaults to a full pass); optimizations are confirmed via the selection modal
  before any config is mutated. The old `/checkup` name still works as a hidden alias.
- `/model` works while a turn is running, including reasoning-effort selection in the model picker. The selection is
  shown immediately as pending the next request; the in-flight request keeps its original settings and the next model
  request uses the new provider, model, context budget, and reasoning effort (the same turn when it makes another
  request, otherwise the next turn). Header, session metadata, and persisted defaults reflect the effective selection
  once applied.
- `/feedback` opens the VT Code GitHub issue form in your browser to report a bug or request a feature.

## WebMCP browser bridge

`/webmcp` displays the active-session bridge status, prints a command guide, and can start the bridge inside an
interactive TUI session. Use `/webmcp help` to print the guide without the status details:

```text
/webmcp
/webmcp help
/webmcp pair http://localhost:5173
/webmcp pair --replace http://localhost:5173
/webmcp tools
/webmcp roots
/webmcp unpair
```

Use `/webmcp pair <origin>` when the browser must submit real agent turns to this same VT Code session. The printed
WebSocket URL and one-time pairing code belong to that running TUI process.

For the published WebMCP app, use `https://vtcode.vinhnx.chatgpt.site` for the ChatGPT Site or
`https://vinhnx.github.io` for the GitHub Pages page at `https://vinhnx.github.io/VTCode/`. The path is not part of the
origin. Add both exact origins to `[webmcp].allowed_origins` when one listener should serve both pages.

Pair from the two sides in this order:

1. In the VT Code TUI, run `/webmcp pair http://localhost:5173`.
2. In the WebMCP editor, open **Connect to a local VT Code bridge**.
3. Paste the TUI's WebSocket URL and one-time pairing code into the browser, then select **Pair with VT Code**.

Keep the TUI running. A URL and code from `vtcode webmcp serve` are for the headless workspace bridge and cannot receive
active-session agent turns.

If `/webmcp pair <origin>` reports that WebMCP is already listening, it prints the active WebSocket URL, exact browser
origin, pairing code, and expiry. A second configured origin receives a new code while existing sessions stay connected.
To revoke current sessions and replace the pairing, run `/webmcp pair --replace <origin>` and confirm **Disconnect and
re-pair** in the terminal. `/webmcp unpair` uses the same confirmation and closes the current browser connections.

The standalone command starts the opt-in authenticated WebSocket bridge for headless workspace operations:

```bash
vtcode webmcp serve --origin http://localhost:5173
vtcode webmcp pair --origin http://localhost:5173
vtcode webmcp status
vtcode webmcp tools
vtcode webmcp roots
vtcode webmcp unpair
```

The standalone `vtcode webmcp unpair` command cannot revoke an already running server's in-memory pairing state. It only
reports that revocation is owned by the server process. Stop that process separately with `Ctrl-C` to revoke its active
sessions.

The server binds to loopback and chooses an available port by default. Enter its one-time pairing code in the browser
editor. Exact origins are required; remote access must use `--allow-remote --public-url wss://...` behind a
TLS-terminating reverse proxy, and direct non-loopback binding is rejected. Browser confirmation cannot authorize a real
write: terminal permission or the existing explicit full-auto allowlist remains authoritative. See
[WebMCP bridge development](../development/webmcp.md).

For a local coding companion, `vtcode webmcp stdio --companion` serves the same tools over stdin/stdout without a network
listener or OAuth setup. The host must launch it with a fixed `--workspace`; workspace trust and permissions still apply.
The optional `vtcode webmcp serve --mcp --companion` adds `start_task`, `get_task`, and
`cancel_task` to the remote MCP surface. It requires preconfigured full-auto workspace trust and retains exec
policy. Plan first, explicitly request implementation, then inspect recorded verification. See [companion setup and
limits](../development/vtcode-companion.md).
Read-only planning returns the final plan when inspection is sufficient; distinct shell inspections do not count as
repetitions merely because they use `exec_command`. Task completion still does not imply verified implementation.

For the browser walkthrough, including the WebMCP app, local pairing, connected drafts, and troubleshooting, see the
[WebMCP browser bridge user guide](./webmcp.md).

## Scheduled tasks

Use `vtcode schedule` when the task should survive restarts.

```bash
vtcode schedule create --prompt "check the deployment" --every 10m
vtcode schedule create --prompt "review the nightly build" --cron "0 9 * * 1-5"
vtcode schedule create --reminder "push the release branch" --at "15:00"
vtcode schedule list
vtcode schedule delete 1a2b3c4d
vtcode schedule serve
```

See [Scheduled Tasks](./scheduled-tasks.md) for session reminders, durable daemon behavior, and service installation
details.

## stats (session metrics)

Display current configuration, available tools, and live performance metrics for the running session. Use `--format` to
choose `text`, `json`, or `html` output and `--detailed` to list each tool.

## schema (runtime tool introspection)

Inspect VT Code's built-in tool schemas at runtime so automation can discover exact tool names and input parameters
without relying on stale docs.

### Usage

```bash
# Full JSON document (default)
vtcode schema tools

# Compact schema descriptions for tighter context windows
vtcode schema tools --mode minimal

# NDJSON output for streaming parsers
vtcode schema tools --format ndjson

# Filter to specific tools
vtcode schema tools --name exec_command --name apply_patch
```

### Options

- `--mode` — `minimal`, `progressive` (default), or `full`
- `--format` — `json` (default) or `ndjson`
- `--name` — repeatable exact tool-name filter

## update (binary updates)

Check for and install binary updates of VT Code from GitHub Releases. Updates are downloaded and verified against
checksums for security.

### Usage

```bash
# Check for available updates without installing
vtcode update --check

# Check for updates (same as above, default behavior)
vtcode update
```

### Options

- `--check` — Check for updates and display release notes without installing
- `--force` — Force update even if already on the latest version

### How it works

1. The command checks the GitHub API for the latest VT Code release
2. It compares the remote version with your current version
3. If a new version is available, it shows release notes and download information
4. Interactive TUI sessions automatically check for updates on launch (short cached interval)
5. Managed installs (Homebrew/cargo) show package-manager-specific update guidance

Standalone releases download the exact platform archive, verify a published SHA-256 checksum when available, safely
extract `vtcode`/`vtcode.exe`, and replace the running binary natively. Binaries built with the former updater must be
bootstrapped once with the native installer before this replacement flow can update them.

### Examples

- Check for updates:

  ```bash
  vtcode update --check
  ```

- Check for updates and show if you're on the latest version:

  ```bash
  vtcode update
  ```

## dependencies

Manage optional VT Code dependencies such as ripgrep and ast-grep.

### Usage

```bash
# Install both optional search tools in one step
vtcode dependencies install search-tools

# Check whether VT Code can resolve the optional search tools
vtcode dependencies status search-tools

# Install ripgrep using a supported system installer
vtcode dependencies install ripgrep

# Install the managed ast-grep binary into the canonical executable directory
vtcode dependencies install ast-grep

# Materialize the bundled ast-grep scaffold in the current workspace
vtcode init

# Run ast-grep rule tests and scan for the current workspace
vtcode check ast-grep

# Check whether VT Code can resolve ast-grep
vtcode dependencies status ast-grep
```

### Notes

- `vtcode deps ...` is a short alias for `vtcode dependencies ...`
- `vtcode dependencies install search-tools` bundles the recommended `ripgrep` + `ast-grep` setup after any install
  method
- `vtcode dependencies install ripgrep` installs `rg` through a supported system installer and keeps startup
  non-blocking when you skip it
- `vtcode init` materializes VT Code's bundled ast-grep starter files into the current workspace: `sgconfig.yml`,
  `rules/`, and `rule-tests/`
- `vtcode check ast-grep` is the first-class replacement for the repo-only `./scripts/check.sh ast-grep` flow
- VT Code does not auto-edit your shell profile; on Linux/BSD, add
  `export PATH="${XDG_BIN_HOME:-$HOME/.local/bin}:$PATH"` yourself if you want managed binaries available outside VT
  Code
- On Linux, prefer `ast-grep` over `sg`
- The curl installer includes the search-tools bundle by default; use `--without-search-tools` to skip it

## check

Run built-in repository checks from VT Code.

### Usage

```bash
# Install ast-grep if needed
vtcode dependencies install ast-grep

# Materialize the bundled ast-grep scaffold in the current workspace
vtcode init

# Run ast-grep rule tests and scan
vtcode check ast-grep
```

### Notes

- `vtcode check ast-grep` runs `ast-grep test --config sgconfig.yml` and then `ast-grep scan --config sgconfig.yml`
- The command expects `sgconfig.yml` in the workspace root and points you to `vtcode init` when the scaffold has not
  been materialized yet

## plugins

Manage [Agent Plugins](agent-plugins.md) — portable packages that bundle Agent Skills and MCP servers under a
`plugin.json` manifest.

```bash
# Install a plugin from a git URL or local directory into ~/.agents/plugins
vtcode plugins add https://github.com/example/my-plugin

# List installed plugins with skill and MCP server counts
vtcode plugins list

# Show plugin details
vtcode plugins info my-plugin

# Validate a plugin directory without installing it
vtcode plugins validate ./my-plugin

# Uninstall a plugin
vtcode plugins remove my-plugin
```

### Notes

- `plugins add` clones git URLs with `git clone --depth=1` and requires local directories to contain a valid
  `plugin.json`
- Plugin MCP servers are exposed as `<plugin>.<server>` providers and connect at session startup

## pods

Manage remote GPU-backed model pods over SSH. See the full feature guide in
[GPU Pod Manager](../features/GPU_POD_MANAGER.md).

### Usage

```bash
# Start a pod-backed model
vtcode pods start --name llama \
  --model meta-llama/Llama-3.1-8B-Instruct \
  --ssh "ssh root@gpu.example.com" \
  --gpu 0:A100 --gpu 1:A100 \
  --gpus 2

# Inspect tracked pods
vtcode pods list

# Stream logs for one model
vtcode pods logs --name llama
```

### Commands

- `vtcode pods start` - Launch a model on the active pod
- `vtcode pods stop` - Stop one tracked model
- `vtcode pods stop-all` - Stop every tracked model on the active pod
- `vtcode pods list` - Show tracked model status
- `vtcode pods logs` - Stream the remote log for a model
- `vtcode pods known-models` - Show compatible and incompatible profiles

## Did You Mean?

When you type an unrecognized `vtcode` subcommand, VT Code suggests the closest match:

```bash
$ vtcode initilize
# Did you mean?
#   vtcode init
```

Suggestions use fuzzy matching and are colorized. This also applies to slash commands inside interactive sessions.

## Continue / Resume

Resume the most recent archived session:

```bash
vtcode --continue
# or in interactive mode:
# /resume
```

This starts a fresh conversation with the context preserved from the last session. Use it to pick up where you left off
after closing VT Code.

## Tips

- The agent respects `.vtcodegitignore` to exclude files from search and I/O.
- Prefer `exec_command.cmd` with `rg` for fast, focused text searches with glob filters and context.
- Ask for “N lines of context” when searching to understand usage in-place.
- Shell commands are filtered by allow/deny lists and can be extended via `VTCODE_<AGENT>_COMMANDS_*` environment
  variables.
- Use `vtcode update --check` regularly to stay informed about new features and security updates.
