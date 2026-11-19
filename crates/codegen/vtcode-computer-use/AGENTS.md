# vtcode-computer-use

[Root AGENTS.md](../../../AGENTS.md) | Provider-neutral computer-use types and turn-local driver lifecycle.

- Bind a driver to one approved target and use the active turn's cancellation token.
- Keep screenshot and action payloads out of durable events, history, traces, and debug formatting.
- Driver implementations own target verification and held-input cleanup across cancellation.
- Start browser containers via `browser::container` and terminate them on Stop.
- `browser/` builds with bun (`bun install --frozen-lockfile`); keep `browser/bun.lock` committed and refreshed on dep bumps.
- Keep provider wire formats, approval UI, and native platform APIs in their respective consumers.
