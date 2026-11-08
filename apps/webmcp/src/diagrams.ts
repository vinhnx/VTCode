import { mount, render, type Mounted, type RenderOptions, type RenderResult } from "pele";
import { CIAPRE_COLORS } from "./ciapre-theme.ts";

/** Largest diagram source accepted, in UTF-16 code units (mirrors Pele's default limit and the desktop preview). */
export const MAX_DIAGRAM_SOURCE_UNITS = 50_000;

/** Fixed render options shared by SVG rendering and dialog mounting. */
const PELE_OPTIONS: RenderOptions = { links: false, images: false, idPrefix: "webmcp-diagram" };

/** Theme mapping onto Pele's `--pele-*` CSS variables. Series entries are ordered so adjacent entries stay distinct. */
export const PELE_VAR_ENTRIES: ReadonlyArray<readonly [string, string]> = Object.freeze([
  ["--pele-bg", CIAPRE_COLORS.background],
  ["--pele-fg", CIAPRE_COLORS.foreground],
  ["--pele-muted", CIAPRE_COLORS.muted],
  ["--pele-line", CIAPRE_COLORS.foreground],
  ["--pele-surface", CIAPRE_COLORS.surface],
  ["--pele-surface-alt", CIAPRE_COLORS.surfaceRaised],
  ["--pele-border", CIAPRE_COLORS.border],
  ["--pele-accent", CIAPRE_COLORS.secondary],
  ["--pele-critical", CIAPRE_COLORS.alert],
  ["--pele-error", CIAPRE_COLORS.alertForeground],
  ["--pele-series-1", CIAPRE_COLORS.secondary],
  ["--pele-series-2", CIAPRE_COLORS.alert],
  ["--pele-series-3", CIAPRE_COLORS.added],
  ["--pele-series-4", CIAPRE_COLORS.keyword],
  ["--pele-series-5", CIAPRE_COLORS.type],
  ["--pele-series-6", CIAPRE_COLORS.variable],
  ["--pele-series-7", CIAPRE_COLORS.number],
  ["--pele-series-8", CIAPRE_COLORS.muted],
] as const);

export function isMermaidPath(path: string): boolean {
  const extension = path.split(".").pop()?.toLowerCase();
  return extension === "mmd" || extension === "mermaid";
}

const FENCE_OPEN = /^```[ \t]*mermaid([ \t]+.*)?$/i;
const FENCE_CLOSE = /^\s*`{3,}\s*$/;

/** First closed ```mermaid fence body, or null. Unclosed fences never match: only complete blocks preview. */
export function extractMermaidFence(content: string): string | null {
  const lines = content.split("\n");
  let start = -1;
  for (const [index, line] of lines.entries()) {
    if (start < 0) {
      if (FENCE_OPEN.test(line)) start = index + 1;
    } else if (FENCE_CLOSE.test(line)) {
      return lines.slice(start, index).join("\n");
    }
  }
  return null;
}

/** Previewable diagram source for an open file: whole buffer for mermaid files, first fence for Markdown, else null. Blank sources never match. */
export function diagramSourceFor(path: string, content: string): string | null {
  let source: string | null = null;
  if (isMermaidPath(path)) source = content;
  else if (path.toLowerCase().endsWith(".md")) source = extractMermaidFence(content);
  if (!source || !source.trim()) return null;
  return source;
}

export function isDiagramTooLarge(source: string): boolean {
  return source.length > MAX_DIAGRAM_SOURCE_UNITS;
}

export interface DiagramActionState {
  readonly disabled: boolean;
  readonly title: string;
}

/** Toolbar button state for an open file. Oversized sources stay enabled: the dialog reports the limit. */
export function diagramActionState(path: string | null, content: string): DiagramActionState {
  const source = path ? diagramSourceFor(path, content) : null;
  if (!source) return { disabled: true, title: "Open a Markdown or Mermaid file to preview its diagram" };
  return { disabled: false, title: "Preview the diagram in this file" };
}

/** Render to SVG with the shared options. Thin by design: it pins the exact options the dialog uses. */
export function renderDiagramSvg(source: string): RenderResult {
  return render(source, PELE_OPTIONS);
}

function showDiagramError(errorBox: HTMLElement, error: unknown): void {
  errorBox.hidden = false;
  errorBox.textContent = error instanceof Error ? error.message : String(error);
}

/** Mount a themed, zoomable diagram into a dialog container. Returns null when mounting throws (the error is already shown); otherwise the caller owns the handle and must destroy it on close. */
export function mountDiagramPreview(container: HTMLElement, errorBox: HTMLElement, source: string): Mounted | null {
  for (const [name, value] of PELE_VAR_ENTRIES) container.style.setProperty(name, value);
  errorBox.hidden = true;
  errorBox.textContent = "";
  try {
    const mounted = mount(container, source, {
      ...PELE_OPTIONS,
      zoom: "auto",
      onError: (error: unknown) => showDiagramError(errorBox, error),
    });
    if (mounted.error) showDiagramError(errorBox, mounted.error);
    return mounted;
  } catch (error: unknown) {
    // Mount reports undrawable text via onError, but limit violations may throw.
    showDiagramError(errorBox, error);
    return null;
  }
}
