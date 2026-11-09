# vtcode-diagram

Offline Mermaid diagram rendering through the vendored Pele bundle. This
crate is the single headless render engine: the `vtcode diagram render`
CLI and the desktop app's inline previews both render through it.

Untrusted diagram source executes inside a capped QuickJS runtime (bounded
heap, stack, and wall-clock budget, no module loader, no I/O intrinsics),
so rendering can never touch the network or the filesystem. Sources over
`MAX_DIAGRAM_SOURCE_CHARS` (50,000 UTF-16 units, Pele's own default limit)
fail closed before any JS runs, and render options stay fixed with links
and images off, so the SVG output is inert by construction.

```rust
let sandbox = vtcode_diagram::DiagramSandbox::new(vtcode_diagram::PELE_BUNDLE_JS)?;
let rendered = sandbox.render(
    "flowchart TD\n  A --> B",
    std::time::Duration::from_secs(10),
)?;
std::fs::write("diagram.svg", rendered.svg)?;
```

`DiagramSandbox::render_png` rasterizes the same output to PNG (2x
supersampled, system fonts, no background fill), as does
`vtcode diagram render input.mmd -o out.png`.

Although the render engine is only consumed inside the VT Code workspace
today, the crate is published to crates.io because the published `vtcode`
binary depends on it. Bundle provenance lives in
[assets/PROVENANCE.md](./assets/PROVENANCE.md).
