# vtcode-diagram

[Root AGENTS.md](../../../AGENTS.md) | Offline Mermaid rendering: sandbox, inliner, rasterizer.

## Conventions

- Keep this crate GUI-free: headless `rquickjs` sandbox plus `resvg` rasterizer; theme maps arrive as caller-built data.
- Keep render options fixed (`links`/`images` off); sources over the Pele limit and rasters over the pixel gate fail closed.
- `assets/pele.global.js` is third-party vendored code; update procedure in `assets/PROVENANCE.md`.
- Sandbox, inliner, and raster tests move with the code; pin the bundle globals both consumers call.

## Dependencies

- `rquickjs` (capped JS runtime for the Pele bundle)
- `resvg` with `text` + `system-fonts` only (PNG rasterizer; `tiny-skia`/`usvg`/`fontdb` come via its re-exports)

## Gotchas

- Never add `tiny-skia`, `usvg`, or `fontdb` as direct deps: use `resvg::` re-exports so versions cannot drift.
- `encode_png` needs tiny-skia defaults, which flow through resvg; `default-features = false` on resvg only drops `raster-images`/`memmap-fonts`.
