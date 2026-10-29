import { $ } from "./app-elements.ts";
import { BackendError, errorMessage, isRecord, type Panel } from "./types.ts";

export function message(error: unknown): string { return errorMessage(error); }

export const contentSizeBytes = (content: unknown): number => new TextEncoder().encode(typeof content === "string" ? content : "").length;
export const snapshotSizeBytes = (file: unknown): number => {
  const size = Number(isRecord(file) ? file.size_bytes : undefined);
  return Number.isSafeInteger(size) && size >= 0 ? size : contentSizeBytes(isRecord(file) ? file.content : undefined);
};

export function truncateUtf8(text: string, maxBytes: number): { text: string; truncated: boolean } {
  const bytes = new TextEncoder().encode(text);
  if (bytes.length <= maxBytes) return { text, truncated: false };
  const suffix = "\n[output truncated by the browser tool limit]";
  const suffixBytes = new TextEncoder().encode(suffix).length;
  let end = Math.max(0, maxBytes - suffixBytes);
  while (end > 0 && ((bytes[end] ?? 0) & 0xc0) === 0x80) end -= 1;
  return { text: `${new TextDecoder().decode(bytes.slice(0, end))}${suffix}`, truncated: true };
}

let toastTimer: ReturnType<typeof setTimeout> | null = null;

export function toast(text: string): void {
  $("toast").textContent = text;
  $("toast").classList.add("show");
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => $("toast").classList.remove("show"), 2800);
}

export function log(text: string): void {
  const item = document.createElement("li");
  item.textContent = text;
  const time = document.createElement("time");
  time.textContent = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  item.append(time);
  $("activityLog").prepend(item);
}

export function status(title: string, detail: string): void {
  $("statusText").textContent = title;
  $("statusDetail").textContent = detail;
}

export function selectTerminal(panel: Panel): void {
  for (const tab of document.querySelectorAll<HTMLElement>("[data-terminal]")) {
    const active = tab.dataset.terminal === panel;
    tab.classList.toggle("active", active);
    tab.setAttribute("aria-selected", String(active));
  }
  for (const pane of ["activity", "changes", "turn"] as const) {
    const element = $(`terminal${pane.charAt(0).toUpperCase()}${pane.slice(1)}`);
    const active = pane === panel;
    element.hidden = !active;
    element.classList.toggle("active", active);
  }
}

export function openTurnComposer(): void {
  selectTerminal("turn");
  $("promptInput").focus();
}

export function fileName(path: string): string { return path.split("/").at(-1) || path; }

export function panelFromValue(value: string | undefined): Panel | null {
  return value === "activity" || value === "changes" || value === "turn" ? value : null;
}

export function webMcpFileNotFoundError(path: string): BackendError {
  return new BackendError(`No workspace file named "${path}"; call list_project_files or search_code and retry with a returned path`, "not_found");
}

export function isTypingTarget(target: EventTarget | null): boolean {
  return target instanceof Element && Boolean(
    target.matches("input, textarea, select, [contenteditable='true']") || target.closest(".cm-editor"),
  );
}

export async function copyText(value: string): Promise<void> {
  if (!navigator.clipboard?.writeText) throw new Error("Clipboard access is unavailable");
  await navigator.clipboard.writeText(value);
}
