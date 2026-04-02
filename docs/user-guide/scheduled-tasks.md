# Scheduled Tasks

VT Code has two scheduling layers:

- Session-scoped tasks for the current interactive session via `/loop` and narrow reminder phrases
- Durable tasks via `vtcode schedule` that persist on disk, run while VT Code is open, and can keep running after restart when the local scheduler daemon is installed

Use session tasks for quick polling while you are actively in chat. Use durable tasks when you want VT Code to keep checking or reminding you outside the current session.

## Session-scoped scheduling

Session scheduling is attached to the active interactive session only.

- `/loop [interval] <prompt>` schedules a prompt repeatedly
- `/loop <prompt> every <interval>` is also supported
- If no interval is provided, VT Code defaults to every 10 minutes
- Natural-language one-shot reminders are intentionally narrow in v1:
  - `remind me at 3pm to push the release branch`
  - `in 45 minutes, check whether the integration tests passed`
  - `what scheduled tasks do I have?`
  - `cancel <job id|name>`

Examples:

```text
/loop 5m check whether the deployment finished
/loop /review-pr 1234 every 20m
remind me at 15:00 to push the release branch
in 45 minutes, check whether the integration tests passed
```

Behavior:

- Tasks stay in memory only and disappear when the interactive VT Code session exits
- VT Code checks for due tasks once per second
- Due prompts are injected only at idle boundaries, after the current turn finishes, so human input stays higher priority
- Times are interpreted in your local timezone
- Recurring session tasks expire after 72 hours
- A session can hold at most 50 scheduled tasks
- Recurring tasks use deterministic jitter of up to 10% of the period, capped at 15 minutes
- One-shot tasks scheduled at `:00` or `:30` may fire up to 90 seconds early

Intervals accept `s`, `m`, `h`, and `d`. Since the scheduler runs at minute granularity, seconds are rounded up. Uneven cadences such as `7m` or `90m` are normalized to the nearest clean interval and VT Code tells you what it picked.

VT Code also exposes three built-in session tools for parity with the scheduler runtime:

- `cron_create`
- `cron_list`
- `cron_delete`

## Durable scheduling

Durable tasks are managed with `vtcode schedule`. Unlike `/loop`, they are not tied to a chat session.

Examples:

```bash
vtcode schedule create --prompt "check the deployment" --every 10m
vtcode schedule create --prompt "review the nightly build" --cron "0 9 * * 1-5"
vtcode schedule create --reminder "push the release branch" --at "15:00"
vtcode schedule list
vtcode schedule delete 1a2b3c4d
```

Command summary:

- `vtcode schedule create` creates a durable prompt task or reminder
- `vtcode schedule list` lists durable tasks
- `vtcode schedule delete <id>` deletes a durable task
- `vtcode schedule serve` runs the local scheduler daemon
- `vtcode schedule install-service` installs a user service for the daemon
- `vtcode schedule uninstall-service` removes the installed user service

In the interactive `/schedule` wizard, pressing `Enter` on an empty inline input accepts the displayed default value for fields such as prompt cadence, label, and workspace path.

Behavior:

- Task definitions are stored under the VT Code config directory in `scheduled_tasks/tasks/`
- Runtime state and claim files are stored under the VT Code data directory in `scheduled_tasks/`
- Prompt tasks run by spawning a fresh `vtcode exec` process in the configured workspace
- Durable tasks are polled while an interactive VT Code session is open
- `vtcode schedule serve` or an installed service keeps durable tasks running when VT Code is not open
- Reminder tasks surface a local VT Code notification without invoking the model
- macOS uses a user LaunchAgent
- Linux uses a `systemd --user` service
- Windows durable scheduler service management is not supported in v1

One-shot durable tasks keep their run state after firing so `vtcode schedule list` can still show the last outcome.

## Config and disable flags

Scheduled tasks are disabled by default.

```toml
[automation.scheduled_tasks]
enabled = false
```

Set `VTCODE_DISABLE_CRON=1` to disable all VT Code scheduler entry points, including `/loop`, reminder interception, scheduler tools, and `vtcode schedule`.

## Security model

VT Code supports its internal scheduler, but still blocks OS-level task schedulers such as `crontab` and `at` from agent-issued shell commands.

- Use `/loop` for session-scoped work
- Use `vtcode schedule` for durable local automation
- Do not route automation through raw shell scheduling commands
