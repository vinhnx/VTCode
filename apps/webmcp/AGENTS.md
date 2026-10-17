# WebMCP browser app

[Root guidance](../../AGENTS.md) | Strict TypeScript, Vite, and Bun.

## Module ownership

- `main.ts` owns backend replacement, editor/draft/proposal state, approval actions, event wiring, and browser tool registration.
- `app-elements.ts` owns typed required-element lookup; keep DOM IDs aligned with `index.html`.
- `evidence-controller.ts` owns evidence dialogs and tool observation over `webmcp-evidence.ts`;
  capture failures must preserve tool results and original errors.
- `settings-controller.ts` reads live backend connection facts through a getter; it owns setup presentation
  and setup-value persistence, never pairing or write authority.
- Keep backend operations in `backend.ts`, protocol validation in `protocol.ts`, and storage codecs in `persistence.ts`.

## Verification and boundaries

- Run `bun run typecheck`, `bun run test`, and `bun run build` from this directory; use the committed `bun.lock`.
- Keep pairing codes and session tokens in memory. Persist only permitted setup/workspace values through the existing codecs.
- Render external text through `textContent`; preserve shell quoting in copied workspace setup commands.
- Preserve terminal-authoritative approvals and proposal revalidation. Browser tools may edit drafts and request review,
  never approve/apply/revert filesystem changes.
- Controller DOM tests cover ownership and state transitions; separately verify rendered dialogs, keyboard focus,
  and draft review in an available browser.
