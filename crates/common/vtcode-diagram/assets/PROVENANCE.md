# Pele vendored bundle — provenance

This directory vendors the Pele browser bundle used for offline Mermaid
diagram rendering. It is third-party code, synced from upstream, not
maintained here.

- Package: `pele` 1.13.0 (MIT, © kepano / Obsidian)
- Source: <https://registry.npmjs.org/pele/-/pele-1.13.0.tgz>
- Upstream repo: <https://github.com/obsidianmd/pele>
- File: `pele.global.js` (browser global `pele`: `render`, `mount`, `PeleError`)
- SHA-256 (`pele.global.js`):
  `0b0a46c57a5a6971670c09f5ec4066705bd78505f90fd11adf6260325efa0910`
- Vendored: 2026-11-08
- License: `LICENSE` (copy of the package's MIT license)

## Why vendored

Diagram rendering must work offline: the `vtcode diagram render` CLI and
the desktop app's previews (browser page plus inline images) execute this
bundle locally instead of loading Pele from a CDN. Only `pele.global.js`
is shipped; the ESM tree (`dist/esm/`) is not needed.

## Updating

1. Download the new tarball and verify its checksum out of band:
   `npm pack pele@<version>` (or `npm view pele dist.tarball`).
2. Replace `pele.global.js` and `LICENSE` with the new package's copies.
3. Update the version, date, and SHA-256 above.
4. Update the Pele entry in `scripts/templates/third-party-header.txt` and the
   matching block in `THIRD-PARTY-NOTICES` (or regenerate with
   `scripts/generate-notices.sh`).
5. Re-run the render tests:
   `cargo nextest run --locked -p vtcode-diagram` and
   `cargo nextest run --locked -p vtcode-desktop -E 'test(diagram)'`.
