# GitHub Copilot Managed Auth

VT Code uses the official `copilot` CLI for GitHub Copilot login and logout. VT Code does not run a separate native GitHub OAuth flow for this provider.

## What You Need

- `copilot` in `PATH` for `vtcode login copilot`, `vtcode logout copilot`, `/login copilot`, and `/logout copilot`
- An active GitHub Copilot subscription
- Optional: `gh` in `PATH` if you want VT Code to detect an existing GitHub CLI auth session as a fallback

`gh` is not required for Copilot login/logout. VT Code only uses `gh auth status` as an optional fallback when probing existing GitHub authentication.

## If `copilot` Is Missing

VT Code will show a help note in the TUI transcript and in CLI auth output when the configured Copilot command is not runnable.

Use one of these fixes:

- Install the official `copilot` CLI and make sure `copilot` is on `PATH`
- Point VT Code at a custom binary with `VTCODE_COPILOT_COMMAND`
- Or configure a custom command in `vtcode.toml`

```toml
[auth.copilot]
command = "/absolute/path/to/copilot"
```

After that, rerun one of:

```bash
vtcode login copilot
vtcode logout copilot
```

Or use the TUI slash commands:

```text
/login copilot
/logout copilot
```

## If `gh` Is Missing

Nothing special is required for Copilot login/logout. VT Code can still authenticate through the official `copilot` CLI.

You only need `gh` if you want VT Code to reuse an existing GitHub CLI auth session as a fallback during auth detection.

## Troubleshooting

- If `copilot` is installed but VT Code still cannot find it, check your shell `PATH`
- If VT Code uses the wrong binary, set `VTCODE_COPILOT_COMMAND` or `[auth.copilot].command`
- If login succeeds in the standalone `copilot` CLI but VT Code still reports no auth, run `vtcode auth status copilot` to inspect the detected auth source
