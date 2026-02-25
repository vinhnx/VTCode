# Agent Teams (Experimental)

Agent teams let you coordinate multiple VT Code sessions as a lightweight team.
The lead session creates tasks, assigns work, and exchanges messages with
teammates. Team state is stored on disk so teammate sessions can attach
independently.

## Enablement

Enable agent teams in `vtcode.toml`:

```toml
[agent_teams]
enabled = true
# max_teammates = 4
# default_model = ""
# teammate_mode = "auto" # auto|tmux|in_process
# storage_dir = "~/.vtcode"
```

Optional override:

```bash
export VTCODE_EXPERIMENTAL_AGENT_TEAMS=1
```

## Commands

- `/team start [name] [count] [subagent_type] [--model MODEL]`
- `/team add <name> [subagent_type] [--model MODEL]`
- `/team remove <name>`
- `/team task add <description> [--depends-on 1,2]`
- `/team task claim <task_id>`
- `/team task complete <task_id> [summary]`
- `/team task fail <task_id> [summary]`
- `/team assign <task_id> <teammate>`
- `/team message <teammate|lead> <message>`
- `/team broadcast <message>`
- `/team tasks`
- `/team teammates`
- `/team model`
- `/team stop`

`/team model` opens the model picker and saves the selection to
`[agent_teams] default_model` in `vtcode.toml`.

## Keybindings (Inline UI)

- `Shift+Up/Down`: cycle the active teammate (lead only).
- `Shift+Tab`: toggle delegate mode (lead only). In delegate mode, tools are blocked.

When a teammate is active, plain text input from the lead is sent as a team
message instead of the main agent prompt. Use slash commands to continue
working in the lead session.

## Modes

`teammate_mode` controls how teammates are displayed:

- `auto`: use tmux if running inside tmux, otherwise in-process
- `tmux`: spawn each teammate in a tmux split pane
- `in_process`: run teammates in the same terminal (message-based only)

When using tmux mode, VT Code spawns new panes with `tmux split-window`.

## Teammate Sessions (CLI)

Teammates can attach from a separate terminal:

```bash
vtcode --team <team> --teammate <name> --team-role teammate
```

Optional overrides:

```bash
vtcode --team <team> --teammate <name> --team-role teammate --teammate-mode tmux --model <model>
```

## Storage

Team data is stored under the configured storage directory:

- `teams/<team>/config.json`
- `teams/<team>/mailbox/*.jsonl`
- `tasks/<team>/tasks.json`

Default storage is `~/.vtcode` unless overridden by `storage_dir`.

## Hooks

Lifecycle hooks can respond to team events:

- `task_completion` / `task_completed`
- `teammate_idle`

See `docs/guides/lifecycle-hooks.md` for payload details.

## Limitations

- Agent teams are experimental and disabled by default.
- tmux split panes require tmux to be installed and running.
- Teammate sessions must be started with `--team`/`--teammate` to attach.
