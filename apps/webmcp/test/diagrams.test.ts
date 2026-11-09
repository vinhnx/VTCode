import test from "node:test";
import assert from "node:assert/strict";
import {
  MAX_DIAGRAM_SOURCE_UNITS,
  PELE_VAR_ENTRIES,
  diagramActionState,
  diagramSourceFor,
  extractMermaidFence,
  isDiagramTooLarge,
  isMermaidPath,
  renderDiagramSvg,
} from "../src/diagrams.ts";
import { CIAPRE_COLORS } from "../src/ciapre-theme.ts";

test("mermaid paths match by extension, mirroring the editor language idiom", () => {
  for (const path of ["flow.mmd", "flow.mermaid", "FLOW.MMD", "docs/flow.mmd", "a.b.mmd"]) {
    assert.equal(isMermaidPath(path), true, path);
  }
  for (const path of ["flow.md", "flow.txt", "mmd.txt", "flow.mermaidjs", "flowmmd"]) {
    assert.equal(isMermaidPath(path), false, path);
  }
  // Same pop()-based idiom as editor language detection: extensionless names match.
  assert.equal(isMermaidPath("mermaid"), true);
});

test("fence extraction takes the first closed mermaid block only", () => {
  assert.equal(extractMermaidFence("```mermaid\nflowchart TD\n  A --> B\n```"), "flowchart TD\n  A --> B");
  assert.equal(extractMermaidFence("```MERMAID\nA\n```"), "A");
  assert.equal(extractMermaidFence("```mermaid title here\nA\n```"), "A");
  assert.equal(extractMermaidFence("```rust\ncode\n```\n```mermaid\nA\n```"), "A");
  assert.equal(
    extractMermaidFence("```mermaid\nfirst\n```\n```mermaid\nsecond\n```"),
    "first",
  );
  assert.equal(extractMermaidFence("```mermaid\nA\n````"), "A");
  // First closing run wins, even mid-block: documented, not nested.
  assert.equal(extractMermaidFence("```mermaid\nA\n```\nB\n```"), "A");
  assert.equal(extractMermaidFence("```mermaidjs\nA\n```"), null);
  assert.equal(extractMermaidFence("```mermaid\nunclosed"), null);
  assert.equal(extractMermaidFence("no fences here"), null);
});

test("diagram sources resolve per file kind and reject blanks", () => {
  assert.equal(diagramSourceFor("flow.mmd", "flowchart TD\n  A --> B"), "flowchart TD\n  A --> B");
  assert.equal(diagramSourceFor("notes.MD", "```mermaid\nA\n```"), "A");
  assert.equal(diagramSourceFor("notes.md", "no fence"), null);
  assert.equal(diagramSourceFor("flow.txt", "```mermaid\nA\n```"), null);
  assert.equal(diagramSourceFor("flow.mmd", "  \n "), null);
  assert.equal(diagramSourceFor("notes.md", "```mermaid\n  \n```"), null);
});

test("toolbar action enables only with a previewable source", () => {
  assert.deepEqual(diagramActionState(null, ""), {
    disabled: true,
    title: "Open a Markdown or Mermaid file to preview its diagram",
  });
  assert.deepEqual(diagramActionState("flow.txt", "```mermaid\nA\n```"), {
    disabled: true,
    title: "Open a Markdown or Mermaid file to preview its diagram",
  });
  assert.deepEqual(diagramActionState("flow.mmd", "   "), {
    disabled: true,
    title: "Open a Markdown or Mermaid file to preview its diagram",
  });
  assert.deepEqual(diagramActionState("flow.mmd", "flowchart TD\n  A --> B"), {
    disabled: false,
    title: "Preview the diagram in this file",
  });
  assert.deepEqual(diagramActionState("notes.md", "```mermaid\nA\n```"), {
    disabled: false,
    title: "Preview the diagram in this file",
  });
  // Oversized sources stay enabled: the dialog reports the limit on click.
  assert.equal(diagramActionState("flow.mmd", "x".repeat(60_000)).disabled, false);
});

test("size gate counts UTF-16 units like Pele", () => {
  assert.equal(MAX_DIAGRAM_SOURCE_UNITS, 50_000);
  assert.equal(isDiagramTooLarge("x".repeat(50_000)), false);
  assert.equal(isDiagramTooLarge("x".repeat(50_001)), true);
  // Astral characters count double, matching source.length.
  assert.equal(isDiagramTooLarge(`${"x".repeat(49_998)}🎨`), false);
  assert.equal(isDiagramTooLarge(`${"x".repeat(49_999)}🎨`), true);
});

test("theme entries cover every Pele token with ciapre values", () => {
  const expected = [
    "--pele-bg",
    "--pele-fg",
    "--pele-muted",
    "--pele-line",
    "--pele-surface",
    "--pele-surface-alt",
    "--pele-border",
    "--pele-accent",
    "--pele-critical",
    "--pele-error",
    "--pele-series-1",
    "--pele-series-2",
    "--pele-series-3",
    "--pele-series-4",
    "--pele-series-5",
    "--pele-series-6",
    "--pele-series-7",
    "--pele-series-8",
  ];
  assert.equal(PELE_VAR_ENTRIES.length, expected.length);
  const seen = new Map(PELE_VAR_ENTRIES.map(([name, value]) => [name, value] as const));
  for (const name of expected) {
    const value = seen.get(name);
    assert.match(value ?? "", /^#[0-9a-f]{6}$/, `${name} is #rrggbb`);
  }
  assert.equal(seen.get("--pele-bg"), CIAPRE_COLORS.background);
  assert.equal(seen.get("--pele-fg"), CIAPRE_COLORS.foreground);
  assert.equal(seen.get("--pele-series-3"), CIAPRE_COLORS.added);
});

test("real render produces SVG geometry for valid diagrams", () => {
  const result = renderDiagramSvg("flowchart TD\n  A[Mermaid] --> B[Pele]");
  assert.ok(result.svg.includes("<svg"), "svg element present");
  assert.ok(result.width > 0 && result.height > 0, `nonzero size: ${result.width}x${result.height}`);
  const sequence = renderDiagramSvg("sequenceDiagram\n  Alice->>Bob: Hello");
  assert.ok(sequence.svg.includes("<svg"), "second diagram type renders");
});

test("real render honors links:false by dropping anchors", () => {
  const result = renderDiagramSvg(
    'flowchart TD\n  A --> B\n  click B "https://example.test" "tooltip"\n',
  );
  assert.ok(!result.svg.includes("<a "), `no anchors with links:false:\n${result.svg}`);
});

test("real render rejects invalid sources with a coded error", () => {
  assert.throws(() => renderDiagramSvg("Hello"), (error: unknown) => {
    assert.equal((error as { code?: unknown }).code, "unsupported-diagram");
    return true;
  });
});

test("real render escapes hostile labels without script breakout", () => {
  const result = renderDiagramSvg('flowchart TD\n  A["</script><script>alert(1)</script>"] --> B["<!--x"]');
  assert.ok(!result.svg.includes("<script"), "no script element");
  assert.ok(!result.svg.includes("<!--x"), "no comment breakout");
  assert.ok(result.svg.includes("&lt;"), "label escaped");
});
