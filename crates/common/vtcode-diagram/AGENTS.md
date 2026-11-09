# vtcode-diagram

Offline Mermaid rendering: capped QuickJS sandbox over the vendored Pele bundle.

## Conventions

- Keep this crate GUI-free (`rquickjs` only): theme maps arrive as caller-built data.
- Keep render options fixed (`links`/`images` off); sources over the Pele limit fail closed.
- `assets/pele.global.js` is third-party vendored code; update procedure in `assets/PROVENANCE.md`.
- Sandbox and inliner tests move with the code; pin the bundle globals both consumers call.

## Dependencies

- `rquickjs` (capped JS runtime for the Pele bundle)
