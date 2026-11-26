# Terminal program resume

VT Code advertises its session resume command through the
[Program Resume Protocol (OSC 7577)](https://www.superlogical.com/rex/docs/build/program-resume):
the program that knows how to resume itself reports the command, and the
terminal decides whether to run it after a restart. Reporting is a hint, never
a guarantee. Reporting is on by default. Disable it in any normal configuration layer:

```toml
[ui.program_resume]
enabled = false
```

Reports go to the interactive TUI's stderr terminal stream, only when stdin,
stdout, and stderr are terminals and the terminal exports `OSC7577_TOKEN`
(the protocol's whole feature detection: no token, no report, no query).
Valid live configuration reloads apply this setting. Disabling withdraws the
outstanding record with `clear`. Terminals whose `VTE_VERSION` predates the
parser that safely ignores unknown OSC sequences (pre-0.54, mirroring Codex)
stay silent.

## Adoption and existing telemetry

The adapter projects the current session identity. It does not write another
history file or feed ATIF or the trajectory logger. Keep these responsibilities
distinct:

| Surface | Responsibility |
| --- | --- |
| `ThreadEvent` and canonical `events.jsonl` | Authoritative runtime event history |
| Session archives (`session-*.json`) | Resumable conversation snapshots |
| OSC 7577 | Bounded current resume advertisement (command + directory) |
| OSC 7501 | Bounded current terminal state with generic labels |

Like OSC 7501, resume reporting adds no `ThreadEvent` variants: it is terminal
presentation state, not execution evidence.

## Lifecycle

| Session event | Terminal record |
| --- | --- |
| Interactive session starts (history enabled) | Report `vtcode --workspace <abs> --resume <archive-id>` with `cwd` = workspace |
| `/new`, fork, or resume switch (new archive) | Re-report with the new identifier |
| Suspend to shell (Ctrl-Z) and continue | Re-report the unchanged target (another program may have overwritten it) |
| User exit (`exit`, `/quit`, Ctrl-C out) | `clear` (queued ahead of shutdown on the same FIFO channel) |
| Session error, signal, crash, or panic | Record kept: the session stays resumable |
| History persistence disabled | Nothing reported (no archive to resume) |
| Headless `exec`, `--print`, ask one-shots | Nothing reported (re-running an agent prompt has side effects, so it is not safe to repeat) |

The resume command pins the workspace explicitly (`--workspace <abs>`) so a
restart lands back in the right session without prompts even when the terminal
restores a different directory; the `cwd` key carries the same directory for
terminals that restore it. The archive identifier is the session file stem
(ASCII letters, digits, `-`, `_`), safe on the protocol wire.

Two deliberate deviations from spec SHOULDs, both fail-closed:

- A resume against a missing or unknown archive errors with a clear message
  instead of starting a fresh session, so a typo or a restart before the first
  turn snapshot never silently drops the user into the wrong session.
- Clean exits keep the archive on disk (it is conversation history, not only a
  resume file); the 50-session / 30-day retention policy reclaims it.

## Ownership and privacy

`vtcode-commons::program_resume` owns the bounded encoder, token gate, and
typed projection commands. `vtcode-ui` owns the single-record adapter,
deduplication, terminal writes, and the SIGCONT re-announce. The runloop owns
the lifecycle edges (`advertise_program_resume` at session setup,
`clear_program_resume` on user exit) and the resume argv builder in
`src/agent/runloop/unified/program_resume.rs`.

Reports contain the `vtcode` program name, the workspace path, and the archive
identifier. They exclude prompts, commands, summaries, previews, credentials,
and exception text. Every argv element and the working directory are standard
base64; controls are rejected, relative program paths are rejected, and the
2048-byte / 64-element protocol limits are enforced. Only changed reports are
written; there are no heartbeats. Writes use the existing terminal-operation
lock, failures log context and remain retryable, and writes stop once terminal
restoration is claimed. Panic and signal cleanup never emit `clear`.

Token hygiene: reporting programs must remove `OSC7577_TOKEN` from every child.
Sandboxed launches already rebuild from an allowlist, and the `*_TOKEN` filter
already covers provider children; the token is additionally stripped from
non-sandboxed PTY and pipe children, shell snapshot capture and application,
the pod SSH transport (never forwarded off-machine), and the zsh exec bridge.
The token is never logged. Self re-launch (updates) keeps it: same program,
new report. The Codex app-server proxy passes it through like a shell (the
proxy never reports; the sidecar is the foreground program). Fully piped
children (companion exec tasks) cannot write to the terminal, so the token is
unusable there; every child with terminal stdio is stripped.

## Verification

Run focused regressions with:

```sh
cargo nextest run --locked -p vtcode-commons -E 'test(program_resume)'
cargo nextest run --locked -p vtcode-ui -E 'test(program_resume)'
cargo nextest run --locked -p vtcode-config -E 'test(program_resume)'
cargo nextest run --locked -p vtcode -E 'test(program_resume) or test(program_status_live_reload)'
cargo nextest run --locked -p vtcode-safety -p vtcode-core -E 'test(should_filter) or test(excluded_env) or test(pod_ssh_removes)'
cargo nextest run --locked -p vtcode-bash-runner -E 'test(pipe_child_never_receives)'
```

Encoder tests pin all four spec vectors byte-for-byte (nvim, clear, lazygit
with cwd, wrapper) against `dummy-token-replace-me`. The PTY suite
(`tests/program_resume_pty_tests.rs`) drives a live TUI session in a pty and
asserts the exact report + clear bytes (MAC-verified against the test token),
emission count, ordering before terminal restoration, and silence when
disabled or tokenless.

Manual acceptance should cover:

| Host path | Check |
| --- | --- |
| Rex (or another 7577 terminal) | Kill the terminal mid-session, restart, confirm it offers/runs `vtcode --workspace <ws> --resume <id>` and lands back in the session |
| Clean exit | `exit` withdraws the record (no resume offered afterwards) |
| `/new` | Restart offers the new session, not the previous one |
| Ctrl-Z / `fg` | Record still current after suspend cycle |
| Terminal without protocol support | Input, navigation, and restoration remain usable; no visible escape text |
| Redirected stdin, stdout, or stderr | No OSC 7577 reports |
| `ui.program_resume.enabled = false` | No reports; previously advertised record withdrawn |

Record the terminal version and host path with results. Treat unavailable hosts as unverified rather than passed.
