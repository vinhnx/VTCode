import { InMemoryBackend, VtCodeBackend, buildTurnPrompt, connectVtCode, createBackend, createUnifiedDiff, digest, MAX_FILE_BYTES } from "./backend.ts";
import { $ } from "./app-elements.ts";
import { createEvidenceController } from "./evidence-controller.ts";
import { createSettingsController } from "./settings-controller.ts";
import { CodeEditor } from "./editor.ts";
import { browserOrigin } from "./deployments.ts";
import { loadBrowserSettings, loadBrowserState, saveBrowserState, type StorageLike } from "./persistence.ts";
import { createWebMcpEvidenceRecorder } from "./webmcp-evidence.ts";
import { createWebMcpTools, registerWebMcpTools, replaceExactText, type ModelContext, type StageTextEditInput, type ToolExecutionOptions, type WebMcpRegistration } from "./webmcp.ts";
import { BackendError, errorCode, errorMessage, isRecord, type BackendConnectionEvent, type BackendEvent, type ClientProposal, type EditorStateForWebMcp, type FileSnapshot, type Panel, type PatchProposal, type PersistedBrowserState, type RuntimeStatus, type SearchMatch, type SearchResult, type StatusPayload, type TreeNode, type WebMcpEnvironmentState, type WorkspaceFile } from "./types.ts";
import "../styles.css";
import { mountExplanation } from "./explanation.ts";

type Backend = InMemoryBackend | VtCodeBackend;

interface LastChangeView {
  readonly change_id: string;
  readonly paths: string[];
  readonly before: Array<Pick<FileSnapshot, "path" | "content">>;
}

interface AppState {
  readonly files: Map<string, WorkspaceFile>;
  readonly snapshots: Map<string, FileSnapshot>;
  readonly drafts: Map<string, string>;
  openTabs: string[];
  selected: string | null;
  expandedDirs: Set<string>;
  filter: string;
  clientProposal: ClientProposal | null;
  serverProposal: PatchProposal | null;
  approved: boolean;
  pendingTerminalApproval: boolean;
  lastChange: LastChangeView | null;
  readonly conflicts: Set<string>;
  unsubscribe: (() => void) | null;
  unsubscribeConnection: (() => void) | null;
  unsubscribeStatus: (() => void) | null;
}

const MAX_HYDRATION_EVENTS = 256;
const MAX_SEARCH_FILES = 128;
const MAX_SEARCH_BYTES = 8 * 1024 * 1024;
const MAX_WEBMCP_RESULT_BYTES = 128 * 1024;
const PERSIST_DEBOUNCE_MS = 250;
const APP_INSTANCE = typeof __VTCODE_APP_INSTANCE__ === "string" ? __VTCODE_APP_INSTANCE__ : "development";

function browserStorage(): StorageLike | null {
  try { return globalThis.localStorage; } catch { return null; }
}

const persistedBrowserState = loadBrowserState(browserStorage(), APP_INSTANCE);
const persistedBrowserSettings = loadBrowserSettings(browserStorage(), APP_INSTANCE);
let backend: Backend = createBackend(persistedBrowserState?.fallback_files);
const explanationView = mountExplanation(() => backend);
let toastTimer: ReturnType<typeof setTimeout> | null = null;
let openRequest = 0;
let webMcpRegistration: WebMcpRegistration | null = null;
let fallbackPersistenceTimer: ReturnType<typeof setTimeout> | null = null;
let persistenceWarningShown = false;

const state: AppState = {
  files: new Map(),
  snapshots: new Map(),
  drafts: new Map(),
  openTabs: [],
  selected: null,
  expandedDirs: new Set(),
  filter: "",
  clientProposal: null,
  serverProposal: null,
  approved: false,
  pendingTerminalApproval: false,
  lastChange: null,
  conflicts: new Set(),
  unsubscribe: null,
  unsubscribeConnection: null,
  unsubscribeStatus: null,
};

const editor = new CodeEditor($("editor"), {
  onChange: (path, content) => {
    if (content === state.snapshots.get(path)?.content) state.drafts.delete(path);
    else state.drafts.set(path, content);
    editor.updateDirty(isDirty(path));
    state.clientProposal = null;
    state.serverProposal = null;
    state.approved = false;
    state.pendingTerminalApproval = false;
    persistBrowserWorkspace();
    renderTree();
    renderTabs();
    renderProposal();
    updateEditorFooter();
    status("Draft buffer changed", "Save or review to create a proposal; the workspace is unchanged.");
  },
  onSave: () => { void run(reviewChanges); },
  onSelectionChange: () => updateEditorFooter(),
});

const {
  renderEvidence, recordWebMcpDiscovery, instrumentWebMcpTools, beginWebMcpEvidence,
  copyWebMcpEvidence, downloadWebMcpEvidence, openEvidenceDialog, setRegisteredWebMcpTools,
} = createEvidenceController({
  recorder: createWebMcpEvidenceRecorder(),
  getEditorState: editorStateForWebMcp,
  getEnvironment: webMcpEnvironmentState,
  log, status, toast, copyText,
});

const {
  renderSettings, renderWorkspaceSetup, openWorkspaceSetup, openConnectionPanel,
  openSettingsDialog, persistBrowserSettings, copyPairingCommand, copySetupCommand,
} = createSettingsController({
  getBackend: () => backend,
  getStorage: browserStorage,
  appInstance: APP_INSTANCE,
  log, status, toast, copyText,
});

function message(error: unknown): string { return errorMessage(error); }

const contentSizeBytes = (content: unknown): number => new TextEncoder().encode(typeof content === "string" ? content : "").length;
const snapshotSizeBytes = (file: unknown): number => {
  const size = Number(isRecord(file) ? file.size_bytes : undefined);
  return Number.isSafeInteger(size) && size >= 0 ? size : contentSizeBytes(isRecord(file) ? file.content : undefined);
};

function truncateUtf8(text: string, maxBytes: number): { text: string; truncated: boolean } {
  const bytes = new TextEncoder().encode(text);
  if (bytes.length <= maxBytes) return { text, truncated: false };
  const suffix = "\n[output truncated by the browser tool limit]";
  const suffixBytes = new TextEncoder().encode(suffix).length;
  let end = Math.max(0, maxBytes - suffixBytes);
  while (end > 0 && ((bytes[end] ?? 0) & 0xc0) === 0x80) end -= 1;
  return { text: `${new TextDecoder().decode(bytes.slice(0, end))}${suffix}`, truncated: true };
}

function toast(text: string): void {
  $("toast").textContent = text;
  $("toast").classList.add("show");
  if (toastTimer) clearTimeout(toastTimer);
  toastTimer = setTimeout(() => $("toast").classList.remove("show"), 2800);
}

function log(text: string): void {
  const item = document.createElement("li");
  item.textContent = text;
  const time = document.createElement("time");
  time.textContent = new Date().toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  item.append(time);
  $("activityLog").prepend(item);
}

function recordRuntimeEvent(event: BackendEvent): void {
  explanationView.refresh();
  const sequence = event.type === "event" ? ` #${event.sequence}` : "";
  log(`Runtime event${sequence}`);
  status("Runtime event received", "Refresh a clean file to inspect the latest backend snapshot.");
}

function status(title: string, detail: string): void {
  $("statusText").textContent = title;
  $("statusDetail").textContent = detail;
}

function selectTerminal(panel: Panel): void {
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

function openTurnComposer(): void {
  selectTerminal("turn");
  $("promptInput").focus();
}

function runtimeStatus(): RuntimeStatus | undefined { return backend.statusPayload?.runtime; }
function isBridgeConnected(): boolean { return backend.kind !== "websocket" || backend.connected; }
function isHeadlessBridge(): boolean {
  const runtime = runtimeStatus();
  return isBridgeConnected() && backend.kind === "websocket" && runtime?.mutations_allowed === false && runtime.approval_authority?.startsWith("headless");
}
function isActiveRuntime(): boolean {
  return isBridgeConnected() && backend.kind === "websocket" && runtimeStatus()?.turns_available === true;
}

function updateTurnControl(): void {
  const detail = $("promptDetail");
  const button = $("requestTurn");
  const label = $("requestTurnLabel");
  if (backend.kind === "websocket" && !backend.connected) {
    detail.textContent = `The bridge is disconnected. Active sessions rerun \`/webmcp pair ${browserOrigin()}\`; standalone bridges restart \`vtcode webmcp serve\`, then pair with the newest URL and code.`;
    button.disabled = true;
    label.textContent = "Pair with VT Code first";
    button.title = "Open the WebMCP pairing instructions";
    return;
  }
  const headless = backend.kind === "websocket" && runtimeStatus()?.turns_available === false;
  button.disabled = headless;
  if (backend.kind === "fallback") {
    detail.textContent = `For a real turn, run \`/webmcp pair ${browserOrigin()}\` in the same VT Code TUI, then paste its values above.`;
    label.textContent = "Open pairing instructions";
    button.title = "Show the two-step VT Code pairing instructions";
  } else if (headless) {
    detail.textContent = "This standalone bridge exposes workspace operations only. Start `vtcode chat`, run `/webmcp pair <origin>` in that same session, then pair its new URL and code.";
    label.textContent = "Active VT Code session required";
    button.title = "Pair the bridge created by /webmcp pair in an active VT Code session";
  } else {
    detail.textContent = "The reviewed draft diff is attached; VT Code remains the execution and policy authority.";
    label.textContent = "Request VT Code turn";
    button.title = "Send the prompt and reviewed draft diff to VT Code (Cmd/Ctrl+Enter)";
  }
}

function handleBackendConnection(event: BackendConnectionEvent): void {
  explanationView.refresh();
  if (backend.kind !== "websocket") return;
  if (event.state === "connected") {
    $("supportStatus").textContent = "VT Code connected";
    $("supportStatus").classList.add("available");
    status("Connected to VT Code", runtimeStatus()?.turns_available === false
      ? "Workspace bridge connected, but this standalone headless adapter cannot run agent turns."
      : "Active VT Code session connected; prompts and policy remain in the terminal.");
  } else if (event.state === "reconnecting") {
    $("supportStatus").textContent = "Reconnecting…";
    $("supportStatus").classList.add("available");
    status("Reconnecting to VT Code", "The bridge connection dropped; the browser is retrying with the in-memory session token.");
  } else if (event.state === "reauthorize") {
    $("supportStatus").textContent = "Pair again";
    $("supportStatus").classList.remove("available");
    openConnectionPanel();
    $("connectionSummary").textContent = "New pairing required";
    $("pairingCode").value = "";
    updateTurnControl();
    status("VT Code pairing expired", `For an active session rerun \`/webmcp pair ${browserOrigin()}\`; for a standalone bridge restart \`vtcode webmcp serve\`. Then enter the new URL and one-time code.`);
  } else if (event.state === "disconnected") {
    $("supportStatus").textContent = "Bridge disconnected";
    $("supportStatus").classList.remove("available");
    updateTurnControl();
    status("VT Code bridge disconnected", "Keep the current bridge running; the browser will reconnect automatically. If it was restarted, pair again with its newest URL and code.");
  }
  renderSettings();
  renderProposal();
}

function handleBackendStatus(_payload: StatusPayload | null): void {
  explanationView.refresh();
  if (backend.kind !== "websocket") return;
  renderSettings();
  updateTurnControl();
  renderProposal();
}

function paths(): string[] { return [...state.files.keys()].sort(); }
function snapshot(path: string): FileSnapshot | undefined { return state.snapshots.get(path); }
function current(path: string): string { return state.drafts.get(path) ?? snapshot(path)?.content ?? ""; }
function isDirty(path: string): boolean { return state.drafts.has(path) && state.drafts.get(path) !== snapshot(path)?.content; }
function dirtyPaths(): string[] { return paths().filter(isDirty); }

function saveFallbackWorkspaceNow(silent = false): boolean {
  if (backend.kind !== "fallback") return true;
  const saved = saveBrowserState(browserStorage(), APP_INSTANCE, {
    fallback_files: backend.exportFiles(),
    drafts: Object.fromEntries(state.drafts),
    open_tabs: state.openTabs,
    selected: state.selected,
    expanded_dirs: [...state.expandedDirs],
    filter: state.filter,
    workspace_path: $("workspacePath").value.trim(),
  });
  if (!saved && !silent && !persistenceWarningShown) {
    persistenceWarningShown = true;
    status("Browser state not fully saved", "Storage quota was reached; refresh may lose fallback edits. Export or reduce the workspace before refreshing.");
    toast("Browser state could not be saved");
    log("Browser fallback state persistence failed");
  } else if (saved) {
    persistenceWarningShown = false;
  }
  return saved;
}

function flushBrowserPersistence({ silent = false }: { readonly silent?: boolean } = {}): boolean {
  if (fallbackPersistenceTimer) clearTimeout(fallbackPersistenceTimer);
  fallbackPersistenceTimer = null;
  return saveFallbackWorkspaceNow(silent);
}

function persistBrowserWorkspace({ flush = false, silent = false }: { readonly flush?: boolean; readonly silent?: boolean } = {}): boolean {
  if (backend.kind !== "fallback") return true;
  if (flush) return flushBrowserPersistence({ silent });
  if (fallbackPersistenceTimer) clearTimeout(fallbackPersistenceTimer);
  fallbackPersistenceTimer = setTimeout(() => {
    fallbackPersistenceTimer = null;
    saveFallbackWorkspaceNow();
  }, PERSIST_DEBOUNCE_MS);
  return true;
}

function fileName(path: string): string { return path.split("/").at(-1) || path; }

function expandAncestors(path: string): void {
  const parts = path.split("/");
  for (let index = 1; index < parts.length; index += 1) {
    state.expandedDirs.add(parts.slice(0, index).join("/"));
  }
}

function treeFor(filePaths: readonly string[]): TreeNode {
  const root: TreeNode = { directories: new Map(), files: [] };
  for (const path of filePaths) {
    const parts = path.split("/");
    let node = root;
    for (let index = 0; index < parts.length - 1; index += 1) {
      const name = parts[index];
      if (name === undefined) continue;
      let directory = node.directories.get(name);
      if (!directory) {
        directory = { directories: new Map(), files: [] };
        node.directories.set(name, directory);
      }
      node = directory;
    }
    node.files.push(path);
  }
  return root;
}

function appendTreeNode(parent: HTMLElement, node: TreeNode, prefix: string, depth: number, filterActive: boolean): void {
  for (const [name, directory] of [...node.directories.entries()].sort(([left], [right]) => left.localeCompare(right))) {
    const path = prefix ? `${prefix}/${name}` : name;
    const expanded = filterActive || state.expandedDirs.has(path);
    const row = document.createElement("button");
    row.type = "button";
    row.className = "file-tree-row directory-item";
    row.style.paddingInlineStart = `${10 + depth * 14}px`;
    row.setAttribute("aria-expanded", String(expanded));
    row.title = path;
    const icon = document.createElement("span");
    icon.className = "tree-chevron";
    icon.textContent = expanded ? "▾" : "▸";
    const label = document.createElement("span");
    label.className = "tree-label";
    label.textContent = name;
    row.append(icon, label);
    row.onclick = () => {
      if (expanded) state.expandedDirs.delete(path);
      else state.expandedDirs.add(path);
      renderTree();
      persistBrowserWorkspace();
    };
    parent.append(row);
    if (expanded) appendTreeNode(parent, directory, path, depth + 1, filterActive);
  }

  for (const path of [...node.files].sort((left, right) => left.localeCompare(right))) {
    const row = document.createElement("button");
    row.type = "button";
    row.className = `file-tree-row file-item${path === state.selected ? " active" : ""}`;
    row.style.paddingInlineStart = `${10 + depth * 14}px`;
    row.setAttribute("aria-pressed", String(path === state.selected));
    row.title = path;
    const icon = document.createElement("span");
    icon.className = "file-icon";
    icon.textContent = fileName(path).endsWith(".md") ? "MD" : "<>";
    const label = document.createElement("span");
    label.className = "tree-label";
    label.textContent = fileName(path);
    const marker = document.createElement("span");
    marker.className = "dirty-marker";
    marker.textContent = isDirty(path) ? "*" : state.conflicts.has(path) ? "!" : "";
    row.append(icon, label, marker);
    row.onclick = () => run(() => openFile(path));
    parent.append(row);
  }
}

function renderTree(): void {
  const tree = $("fileTree");
  tree.replaceChildren();
  const query = state.filter.toLowerCase();
  const visiblePaths = paths().filter((path) => !query || path.toLowerCase().includes(query));
  if (visiblePaths.length) appendTreeNode(tree, treeFor(visiblePaths), "", 0, Boolean(query));
  else {
    const empty = document.createElement("div");
    empty.className = "tree-empty";
    empty.textContent = query ? "No matching files" : "No files in this workspace";
    tree.append(empty);
  }
  $("fileCount").textContent = query ? `${visiblePaths.length}/${paths().length} files` : `${paths().length} files`;
  const filterStatus = $("searchResults");
  filterStatus.textContent = query ? `${visiblePaths.length} matching files` : "Filter files by name or path";
}

function renderTabs(): void {
  const tabs = $("fileTabs");
  tabs.replaceChildren();
  for (const path of state.openTabs) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `file-tab${path === state.selected ? " active" : ""}`;
    button.onclick = () => run(() => openFile(path));
    const label = document.createElement("span");
    label.className = "tab-label";
    label.textContent = fileName(path);
    label.title = path;
    const marker = document.createElement("span");
    marker.className = "dirty-marker";
    marker.textContent = isDirty(path) ? "*" : "";
    button.append(label, marker);
    tabs.append(button);
  }
}

function updateEditorFooter(): void {
  if (!editor.view) return;
  const line = editor.view.state.doc.lineAt(editor.view.state.selection.main.head);
  $("cursorPosition").textContent = `Ln ${line.number}, Col ${editor.view.state.selection.main.head - line.from + 1}`;
  const changed = state.selected && isDirty(state.selected);
  const conflict = state.selected && state.conflicts.has(state.selected);
  $("editorState").textContent = conflict ? "External change conflict" : changed ? "Dirty draft · not proposed" : "Workspace snapshot";
  $("editorState").classList.toggle("dirty", Boolean(changed || conflict));
}

function renderSelectedEditor(): void {
  if (!state.selected || !snapshot(state.selected)) return;
  const path = state.selected;
  editor.open(path, current(path), isDirty(path));
  $("fileTitle").textContent = path;
  $("fileType").textContent = path.includes(".") ? path.split(".").pop()?.toUpperCase() || "TEXT" : "TEXT";
  updateEditorFooter();
}

async function openFile(path: string, record = true): Promise<void> {
  if (!state.files.has(path)) throw webMcpFileNotFoundError(path);
  const request = ++openRequest;
  state.selected = path;
  expandAncestors(path);
  if (!snapshot(path)) status("Loading file", path);
  if (!snapshot(path)) await refreshFile(path, false, false);
  if (request !== openRequest || state.selected !== path) return;
  if (!state.openTabs.includes(path)) state.openTabs.push(path);
  renderSelectedEditor();
  renderTree();
  renderTabs();
  if (record) {
    log(`Inspected ${path}`);
    status("Ready for inspection", "Edit a local draft; save opens review instead of writing immediately.");
  }
  persistBrowserWorkspace();
}

function collectChanges(): Array<{ path: string; base_digest: string; content: string }> {
  return dirtyPaths().flatMap((path) => {
    const base = snapshot(path);
    return base ? [{ path, base_digest: base.digest, content: current(path) }] : [];
  });
}

function renderProposal(): void {
  const view = $("diffView");
  view.replaceChildren();
  const proposal = state.serverProposal || state.clientProposal;
  if (!proposal) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    const emptyTitle = document.createElement("strong");
    emptyTitle.textContent = "No changes staged";
    const emptyHint = document.createElement("small");
    emptyHint.textContent = "Edit a buffer, then review the draft.";
    empty.append(emptyTitle, emptyHint);
    view.append(empty);
    $("proposalState").textContent = "No proposal";
    $("proposalState").className = "state-badge";
  } else {
    const diff = proposal.unified_diff || state.clientProposal?.unified_diff || "";
    for (const line of diff.split("\n")) {
      const item = document.createElement("div");
      item.className = `diff-line ${line.startsWith("+") && !line.startsWith("+++") ? "added" : line.startsWith("-") && !line.startsWith("---") ? "removed" : "meta"}`;
      item.textContent = line || " ";
      view.append(item);
    }
    const stateLabel = state.pendingTerminalApproval
      ? "Awaiting terminal"
      : state.approved
        ? "Approved · ready"
        : state.serverProposal
          ? isActiveRuntime() ? "Ready for VT Code turn" : "Proposal sent"
          : "Review ready";
    $("proposalState").textContent = stateLabel;
    $("proposalState").className = `state-badge ${state.approved ? "approved" : state.pendingTerminalApproval ? "pending" : "ready"}`;
  }
  const bridgeUnavailable = backend.kind === "websocket" && !backend.connected;
  $("reviewChanges").disabled = dirtyPaths().length === 0;
  $("approvePatch").disabled = bridgeUnavailable || !state.clientProposal || Boolean(state.serverProposal);
  const requiresAgentTurn = isActiveRuntime() && runtimeStatus()?.mutations_allowed === false;
  $("applyPatch").disabled = bridgeUnavailable || !state.serverProposal || (backend.kind === "fallback" && !state.approved) || requiresAgentTurn;
  $("revertPatch").disabled = bridgeUnavailable || !state.lastChange;
  $("approvePatch").textContent = isActiveRuntime()
    ? "Stage for VT Code turn"
    : backend.kind === "websocket" ? "Request VT Code approval" : "Approve patch";
  $("applyPatch").textContent = requiresAgentTurn
    ? "Apply via VT Code turn"
    : backend.kind === "websocket" ? "Apply after terminal approval" : "Apply approved patch";
  const directChecksUnavailable = isHeadlessBridge() || isActiveRuntime();
  $("runChecks").disabled = bridgeUnavailable || directChecksUnavailable;
  $("runChecks").title = directChecksUnavailable
    ? "Ask VT Code to run checks through the terminal"
    : "Run the selected backend checks";
  if (isHeadlessBridge()) {
    $("approvePatch").textContent = "Stage proposal";
    $("applyPatch").disabled = true;
    $("applyPatch").textContent = "Apply unavailable in headless mode";
  }
}

async function reviewChanges(): Promise<void> {
  const changes = collectChanges();
  if (!changes.length) throw new Error("No browser draft is ready to review; edit a file or call stage_text_edit first");
  const beforeByPath = Object.fromEntries(changes.flatMap((change) => {
    const base = snapshot(change.path);
    return base ? [[change.path, base.content] as const] : [];
  }));
  state.clientProposal = { changes, unified_diff: createUnifiedDiff(changes, beforeByPath) };
  state.serverProposal = null;
  state.approved = false;
  state.pendingTerminalApproval = false;
  selectTerminal("changes");
  renderProposal();
  log(`Reviewed unified diff for ${changes.length} file${changes.length === 1 ? "" : "s"}`);
  status("Review ready", "The diff is client-generated; send it to the selected backend when it looks right.");
  toast("Unified diff ready for review");
}

function proposalPaths(proposal: ClientProposal | PatchProposal | null | undefined): string[] {
  return [...new Set((proposal?.changes || [])
    .map((change) => change.path)
    .filter((path) => path.length > 0))];
}

async function recoverStaleProposal(proposal: ClientProposal | PatchProposal | null | undefined): Promise<string> {
  const affectedPaths = proposalPaths(proposal);
  state.clientProposal = null;
  state.serverProposal = null;
  state.approved = false;
  state.pendingTerminalApproval = false;
  for (const path of affectedPaths) state.conflicts.add(path);
  renderProposal();
  renderTree();
  renderTabs();
  updateEditorFooter();

  let refreshed = 0;
  for (const path of affectedPaths) {
    try {
      const fresh = await backend.readFile(path);
      if (typeof fresh?.content !== "string") throw new Error(`Backend returned invalid content for ${path}`);
      if (snapshotSizeBytes(fresh) > MAX_FILE_BYTES) throw new Error(`File exceeds the browser size limit: ${path}`);
      state.snapshots.set(path, fresh);
      if (state.drafts.get(path) === fresh.content) state.drafts.delete(path);
      state.conflicts.delete(path);
      refreshed += 1;
    } catch (error: unknown) {
      log(`Could not refresh ${path} after the stale proposal: ${message(error)}`);
    }
  }

  if (refreshed > 0 && state.selected !== null && affectedPaths.includes(state.selected)) renderSelectedEditor();
  renderTree();
  renderTabs();
  updateEditorFooter();
  persistBrowserWorkspace();
  log(`Cleared stale proposal for ${affectedPaths.length} file${affectedPaths.length === 1 ? "" : "s"}`);
  const detail = refreshed === affectedPaths.length
    ? "The latest snapshots are loaded and your draft is preserved. Review changes again before requesting the VT Code turn."
    : "Some files could not be refreshed. Reconnect or reload them, then review changes again before requesting the VT Code turn.";
  status("Proposal became stale", detail);
  return detail;
}

async function requestApproval(): Promise<void> {
  if (!state.clientProposal) await reviewChanges();
  if (state.serverProposal) throw new Error("This proposal has already been sent");
  const clientProposal = state.clientProposal;
  if (!clientProposal) throw new Error("No browser draft is ready to request approval");
  selectTerminal("changes");
  try {
    state.serverProposal = await backend.proposeChanges(clientProposal.changes);
  } catch (error: unknown) {
    if (error instanceof BackendError && error.code === "conflict") error.proposalRecovery = await recoverStaleProposal(clientProposal);
    throw error;
  }
  if (backend.kind === "fallback") {
    state.approved = true;
    log("Fallback proposal approved in the browser");
    status("Patch approved", "Fallback mode changes only the in-memory project; apply is still explicit.");
  } else if (isHeadlessBridge()) {
    log("Proposal staged; headless bridge cannot authorize a filesystem apply");
    status("Proposal staged", "This standalone bridge has no terminal runtime for approval. Use explicit full-auto only for a disposable workspace.");
  } else if (isActiveRuntime()) {
    log("Proposal staged for the active VT Code session");
    status("Proposal staged", "Use Request VT Code turn; any write is performed through the terminal permission flow.");
  } else {
    state.pendingTerminalApproval = true;
    log("Requested terminal approval from VT Code");
    status("Awaiting terminal approval", "The browser cannot authorize a real filesystem write.");
  }
  renderProposal();
  if (isActiveRuntime()) openTurnComposer();
}

async function applyProposal(): Promise<void> {
  const serverProposal = state.serverProposal;
  if (!serverProposal) throw new Error("Send a proposal before applying it");
  if (backend.kind === "fallback" && !state.approved) throw new Error("Explicit approval is required");
  if (isActiveRuntime() && runtimeStatus()?.mutations_allowed === false) {
    throw new Error("Ask the active VT Code session to apply this proposal through a turn");
  }
  selectTerminal("changes");
  const before = serverProposal.changes.flatMap((change) => {
    const file = snapshot(change.path);
    return file ? [{ path: change.path, content: file.content }] : [];
  });
  try {
    const result = await backend.applyProposal(serverProposal.proposal_id);
    state.lastChange = { change_id: result.change_id, paths: result.paths, before };
    for (const path of serverProposal.changes.map((change) => change.path)) await refreshFile(path, true);
    state.clientProposal = null;
    state.serverProposal = null;
    state.approved = false;
    state.pendingTerminalApproval = false;
    persistBrowserWorkspace();
    renderProposal();
    log(`Applied approved patch (${backend.kind === "fallback" ? "in memory" : "VT Code"})`);
    status("Change applied", backend.kind === "fallback" ? "Only the deterministic in-memory project changed." : "VT Code completed the authorized workspace change.");
    toast("Patch applied");
  } catch (error: unknown) {
    if (errorCode(error) === "approval_required") {
      state.pendingTerminalApproval = true;
      renderProposal();
      status("Still awaiting terminal approval", "VT Code rejected browser-only authorization; no file was changed.");
    }
    throw error;
  }
}

async function revertLastChange(): Promise<void> {
  if (!state.lastChange) throw new Error("There is no applied change to revert");
  selectTerminal("changes");
  const result = await backend.revertLastChange(state.lastChange.change_id);
  for (const path of result.paths) await refreshFile(path, true);
  log(`Reverted ${result.paths.join(", ")}`);
  state.lastChange = null;
  persistBrowserWorkspace();
  renderProposal();
  status("Change reverted", "The backend validated the current file before restoring its prior snapshot.");
  toast("Last change reverted");
}

async function runChecks(): Promise<void> {
  if (isHeadlessBridge()) {
    throw new Error("Checks are unavailable in the headless bridge; enable explicit full-auto policy for a disposable workspace");
  }
  const result = await backend.runChecks(backend.kind === "fallback" ? undefined : "cargo check --locked");
  selectTerminal("activity");
  $("checkOutput").textContent = [result.stdout, result.stderr].filter(Boolean).join("\n") || JSON.stringify(result, null, 2);
  log(result.exit_code === 0 ? "Checks passed" : "Checks found an issue");
  status(result.exit_code === 0 ? "Checks passed" : "Checks need attention", backend.kind === "fallback" ? "Deterministic browser checks · no filesystem" : "Command executed by VT Code policy");
  toast(result.exit_code === 0 ? "Checks passed" : "A check failed");
}

async function requestTurn(): Promise<void> {
  openTurnComposer();
  if (backend.kind === "fallback") openConnectionPanel();
  if (dirtyPaths().length && !state.clientProposal) await reviewChanges();
  if (isActiveRuntime() && state.clientProposal && !state.serverProposal) await requestApproval();
  const proposal = state.serverProposal || state.clientProposal;
  const prompt = backend.kind === "websocket"
    ? $("promptInput").value
    : buildTurnPrompt($("promptInput").value, proposal?.unified_diff);
  let result;
  try {
    const proposalId = proposal && "proposal_id" in proposal && typeof proposal.proposal_id === "string"
      ? proposal.proposal_id
      : undefined;
    result = await backend.requestTurn(prompt, proposalId);
  } catch (error: unknown) {
    if (error instanceof BackendError && error.code === "conflict") error.proposalRecovery = await recoverStaleProposal(proposal);
    throw error;
  }
  if (!result.accepted) {
    const reason = result.reason || "The selected backend did not accept the agent turn.";
    $("turnOutput").textContent = reason;
    log(`VT Code turn unavailable: ${reason}`);
    status("VT Code turn unavailable", reason);
    toast("VT Code turn unavailable");
    return;
  }
  const turnId = result.turn_id || "accepted";
  log(`Requested agent turn (${turnId})`);
  $("turnOutput").textContent = result.output || "VT Code accepted the bounded request; waiting for runtime events.";
  status("Turn requested", "VT Code received the prompt and reviewed draft diff; runtime events will appear in the activity panel.");
  toast("VT Code turn requested");
}

async function refreshFile(path: string, force = false, render = true): Promise<void> {
  const fresh = await backend.readFile(path);
  if (typeof fresh?.content !== "string") throw new Error(`Backend returned invalid content for ${path}`);
  if (snapshotSizeBytes(fresh) > MAX_FILE_BYTES) throw new Error(`File exceeds the browser size limit: ${path}`);
  const previous = snapshot(path);
  if (isDirty(path) && !force && previous && fresh.digest !== previous.digest) {
    state.conflicts.add(path);
    throw new Error(`External change conflict for ${path}; discard the draft before reloading`);
  }
  state.snapshots.set(path, fresh);
  state.conflicts.delete(path);
  if (force || !isDirty(path)) state.drafts.delete(path);
  if (render && state.selected === path) renderSelectedEditor();
  renderTree();
  renderTabs();
  persistBrowserWorkspace();
}

async function reloadFile(): Promise<void> {
  if (!state.selected) return;
  await refreshFile(state.selected);
  log(`Reloaded ${state.selected} from ${backend.kind}`);
  status("File reloaded", "The draft buffer was compared with the backend snapshot.");
}

function discardDraft(): void {
  if (!state.selected || !isDirty(state.selected)) throw new Error("The selected file has no draft");
  const path = state.selected;
  state.drafts.delete(path);
  state.conflicts.delete(path);
  state.clientProposal = null;
  state.serverProposal = null;
  state.approved = false;
  const base = snapshot(path);
  if (!base) throw new Error(`No snapshot is loaded for ${path}`);
  editor.open(path, base.content, false);
  persistBrowserWorkspace();
  renderTree();
  renderTabs();
  renderProposal();
  updateEditorFooter();
  log(`Discarded draft for ${path}`);
  status("Draft discarded", "The editor now shows the last backend snapshot.");
}

async function runSelfCheck(): Promise<void> {
  if (backend.kind !== "fallback") {
    throw new Error("The self-check runs only in fallback mode; it must not write to a paired workspace");
  }
  if (dirtyPaths().length || state.serverProposal || state.lastChange) {
    throw new Error("Finish the current change before running the self-check");
  }
  if (!state.selected) throw new Error("Select a file before running the self-check");
  const path = state.selected;
  state.drafts.set(path, `${current(path)}\n`);
  editor.open(path, current(path), true);
  await reviewChanges();
  await requestApproval();
  await applyProposal();
  await runChecks();
  await revertLastChange();
  toast("Editor self-check passed");
}

function search(): void {
  state.filter = $("searchInput").value.trim();
  renderTree();
  persistBrowserWorkspace();
}

async function connect(): Promise<void> {
  const url = $("bridgeUrl").value.trim();
  const code = $("pairingCode").value.trim().toUpperCase();
  if (!url || !code) throw new Error("Enter the WebMCP WebSocket URL and the terminal pairing code");
  flushBrowserPersistence({ silent: true });
  persistBrowserSettings();
  const nextBackend = await connectVtCode(url, code);
  try {
    await loadWorkspace(nextBackend);
  } catch (error) {
    nextBackend.close();
    throw error;
  }
  $("settingsDialog").close();
  if ($("quickActionDialog").open) $("quickActionDialog").close();
  $("connectionPanel").open = false;
  $("pairingCode").value = "";
  $("connectionSummary").textContent = isActiveRuntime() ? "Active TUI connected" : "Workspace bridge connected";
  log("Connected to authenticated VT Code WebMCP");
  status("Connected to VT Code", runtimeStatus()?.turns_available === false
    ? "Workspace bridge connected, but this standalone headless adapter cannot run agent turns."
    : "Active VT Code session connected; prompts and policy remain in the terminal.");
  toast("Connected to VT Code");
  editor.focus();
}

async function loadWorkspace(nextBackend: Backend, restoreState: PersistedBrowserState | null = null): Promise<void> {
  const nextFiles = new Map<string, WorkspaceFile>();
  const hydrationEvents: BackendEvent[] = [];
  let hydrationOverflow = false;
  let hydrationComplete = false;
  const stopHydration = nextBackend.subscribeToEvents((event: BackendEvent) => {
    if (hydrationComplete) recordRuntimeEvent(event);
    else if (hydrationEvents.length < MAX_HYDRATION_EVENTS) hydrationEvents.push(event);
    else hydrationOverflow = true;
  });
  const stopConnection = nextBackend.subscribeToConnection(handleBackendConnection);
  const stopStatus = nextBackend.subscribeToStatus(handleBackendStatus);
  try {
    for (const entry of await nextBackend.listFiles()) {
      const path = typeof entry === "string" ? entry : entry?.path;
      if (typeof path !== "string" || !path) throw new Error("Backend returned an invalid workspace path");
      nextFiles.set(path, typeof entry === "string" ? { path } : entry);
    }
  } catch (error) {
    stopHydration();
    stopConnection();
    stopStatus();
    throw error;
  }
  state.unsubscribe?.();
  state.unsubscribeConnection?.();
  state.unsubscribeStatus?.();
  if (nextBackend !== backend) backend.close?.();
  backend = nextBackend;
  explanationView.refresh();
  openRequest += 1;
  state.files.clear();
  for (const [path, file] of nextFiles) state.files.set(path, file);
  state.snapshots.clear();
  state.drafts.clear();
  state.openTabs = [];
  state.selected = null;
  state.expandedDirs.clear();
  state.filter = "";
  $("searchInput").value = "";
  state.clientProposal = null;
  state.serverProposal = null;
  state.approved = false;
  state.pendingTerminalApproval = false;
  state.lastChange = null;
  state.conflicts.clear();
  $("turnOutput").textContent = "No VT Code turn requested.";
  const restoreFallback = nextBackend.kind === "fallback" && restoreState?.app_instance === APP_INSTANCE;
  if (restoreFallback) {
    const available = (items: readonly string[]): string[] => items.filter((path) => nextFiles.has(path));
    const nextDirectories = new Set();
    for (const path of nextFiles.keys()) {
      let separator = path.indexOf("/");
      while (separator > 0) {
        nextDirectories.add(path.slice(0, separator));
        separator = path.indexOf("/", separator + 1);
      }
    }
    const availableDirectories = (items: readonly string[]): string[] => items.filter((path) => nextDirectories.has(path));
    state.openTabs = available(restoreState.open_tabs);
    state.selected = restoreState.selected !== null && nextFiles.has(restoreState.selected) ? restoreState.selected : null;
    state.expandedDirs = new Set(availableDirectories(restoreState.expanded_dirs));
    state.filter = restoreState.filter;
    $("searchInput").value = state.filter;
    for (const [path, content] of Object.entries(restoreState.drafts)) {
      if (!nextFiles.has(path)) continue;
      try {
        const fresh = await nextBackend.readFile(path);
        state.snapshots.set(path, fresh);
        if (content !== fresh.content) state.drafts.set(path, content);
      } catch {
        // The persisted draft is discarded when its fallback file no longer exists.
      }
    }
  }
  hydrationComplete = true;
  state.unsubscribe = stopHydration;
  state.unsubscribeConnection = stopConnection;
  state.unsubscribeStatus = stopStatus;
  for (const event of hydrationEvents) recordRuntimeEvent(event);
  if (hydrationEvents.length || hydrationOverflow) {
    status("Runtime event received", hydrationOverflow
      ? "Events arrived during workspace load; refresh files to reconcile the latest snapshot."
      : "Refresh a clean file to inspect the latest backend snapshot.");
  }
  const runtime = runtimeStatus();
  const workspaceRoot = runtime?.workspace_root;
  if (backend.kind === "fallback") {
    $("connectionPanel").open = true;
    $("connectionSummary").textContent = "Pair to connect VT Code";
  }
  renderSettings();
  $("modeTitle").textContent = workspaceRoot
    ? workspaceRoot.split(/[\\/]/).filter(Boolean).at(-1) || "workspace"
    : "hello-world";
  $("modeDetail").textContent = backend.kind === "fallback"
    ? "In-memory fallback · no filesystem; state survives refreshes until Vite restarts."
    : runtime?.turns_available === false
      ? "Authenticated workspace bridge · no active agent runtime."
      : "Active VT Code session · real workspace and agent turns.";
  $("boundaryDetail").textContent = backend.kind === "fallback"
    ? "In-memory · no filesystem"
    : isHeadlessBridge()
      ? "Headless policy · explicit full-auto only"
      : "Terminal policy · explicit origin";
  $("supportStatus").textContent = backend.kind === "fallback"
    ? "Fallback mode"
    : runtime?.turns_available === false ? "Workspace bridge only" : "VT Code connected";
  $("supportStatus").classList.toggle("available", backend.kind !== "fallback");
  updateTurnControl();
  renderTree();
  renderTabs();
  renderProposal();

  const first = state.selected || state.openTabs[0] || paths()[0];
  if (first) {
    try {
      await openFile(first, false);
    } catch (error) {
      const detail = message(error);
      log(`Initial file read failed: ${detail}`);
      status("Workspace loaded with read error", detail);
    }
  }
}

async function readCurrentFile(path: string): Promise<FileSnapshot> {
  if (!state.files.has(path)) throw webMcpFileNotFoundError(path);
  const draft = state.drafts.get(path);
  const base = snapshot(path);
  if (draft !== undefined) {
    const draftDigest = await digest(draft);
    if (base) {
      return {
        ...base,
        content: draft,
        digest: draftDigest,
        size_bytes: contentSizeBytes(draft),
        base_digest: base.digest,
        draft: true,
      };
    }
    return {
      path,
      content: draft,
      digest: draftDigest,
      size_bytes: contentSizeBytes(draft),
      base_digest: null,
      draft: true,
    };
  }
  const file = await backend.readFile(path);
  return { ...file, draft: false };
}

function webMcpFileNotFoundError(path: string): BackendError {
  return new BackendError(`No workspace file named "${path}"; call list_project_files or search_code and retry with a returned path`, "not_found");
}

function webMcpEnvironmentState(): WebMcpEnvironmentState {
  const originAgentCluster = (window as Window & { readonly originAgentCluster?: unknown }).originAgentCluster;
  const permissionsPolicy = (document as Document & {
    readonly permissionsPolicy?: { readonly allowsFeature?: (feature: string) => boolean };
  }).permissionsPolicy;
  return {
    browsing_context_required: true,
    origin_agent_cluster: typeof originAgentCluster === "boolean" ? originAgentCluster : null,
    tools_permission_allowed: typeof permissionsPolicy?.allowsFeature === "function"
      ? permissionsPolicy.allowsFeature("tools")
      : null,
  };
}

function panelFromValue(value: string | undefined): Panel | null {
  return value === "activity" || value === "changes" || value === "turn" ? value : null;
}

function editorStateForWebMcp(): EditorStateForWebMcp {
  const dirtyFiles = dirtyPaths();
  const workflowState = dirtyFiles.length
    ? "draft_needs_review"
    : state.selected
      ? "file_selected"
      : "workspace_ready";
  const recommendedNextTools = dirtyFiles.length
    ? ["review_draft", "open_panel"]
    : state.selected
      ? ["read_file", "search_code"]
      : ["list_project_files", "search_code"];
  return {
    backend: backend.kind,
    connected: isBridgeConnected(),
    workspace_root: runtimeStatus()?.workspace_root || null,
    bridge_settings: backend.statusPayload?.settings || null,
    authenticated_origin: backend.statusPayload?.authenticated_origin || null,
    selected: state.selected,
    open_tabs: [...state.openTabs],
    dirty_files: dirtyFiles,
    has_client_proposal: Boolean(state.clientProposal),
    has_server_proposal: Boolean(state.serverProposal),
    active_panel: panelFromValue(document.querySelector<HTMLElement>("[data-terminal].active")?.dataset.terminal) || "activity",
    workflow_state: workflowState,
    recommended_next_tools: recommendedNextTools,
    webmcp_context: webMcpEnvironmentState(),
  };
}

async function stageTextEditForWebMcp(
  { path, find, replace, expected_digest: expectedDigest }: StageTextEditInput,
  { signal }: ToolExecutionOptions = {},
): Promise<Record<string, unknown>> {
  if (!state.files.has(path)) throw webMcpFileNotFoundError(path);
  if (!snapshot(path)) await openFile(path, false);
  if (signal?.aborted) throw signal.reason || new Error("The WebMCP edit was aborted");
  if (isDirty(path)) {
    throw new BackendError(`Draft already contains changes for ${path}; call review_draft or discard the draft before staging another edit`, "draft_conflict");
  }
  const base = snapshot(path);
  if (!base) throw new BackendError(`No snapshot is loaded for ${path}; read the file and retry`, "not_found");
  if (base.digest !== expectedDigest) {
    throw new BackendError(`Stale edit for ${path}; call read_file again and use its fresh digest before retrying`, "conflict");
  }
  const content = base.content;
  let next: string;
  try {
    next = replaceExactText(content, find, replace);
  } catch (error: unknown) {
    throw new BackendError(`${message(error)} in ${path}`, errorCode(error) || "text_not_found");
  }
  if (contentSizeBytes(next) > MAX_FILE_BYTES) {
    throw new BackendError(`Edited file exceeds the browser size limit: ${path}; shorten the replacement and retry`, "limit_exceeded");
  }
  const draftDigest = await digest(next);
  if (signal?.aborted) throw signal.reason || new Error("The WebMCP edit was aborted");

  state.drafts.set(path, next);
  state.clientProposal = null;
  state.serverProposal = null;
  state.approved = false;
  state.pendingTerminalApproval = false;
  editor.open(path, next, true);
  persistBrowserWorkspace();
  renderTree();
  renderTabs();
  renderProposal();
  updateEditorFooter();
  selectTerminal("changes");
  log(`Staged browser draft edit for ${path}`);
  status("Agent draft staged", "Review the unified diff before requesting VT Code approval.");
  toast("Agent draft ready for review");
  return {
    staged: true,
    path,
    base_digest: base.digest,
    draft_digest: draftDigest,
    replacement_count: 1,
    requires_review: true,
  };
}

async function reviewDraftForWebMcp(): Promise<Record<string, unknown>> {
  await reviewChanges();
  const proposal = state.clientProposal;
  if (!proposal) throw new Error("No browser draft is ready to review");
  const diff = truncateUtf8(proposal.unified_diff, MAX_WEBMCP_RESULT_BYTES);
  return {
    reviewed: true,
    files: proposal.changes.map(({ path, base_digest, content }) => ({
      path,
      base_digest,
      size_bytes: contentSizeBytes(content),
    })),
    unified_diff: diff.text,
    diff_truncated: diff.truncated,
  };
}

function openPanelForWebMcp(panel: Panel): void {
  selectTerminal(panel);
}

async function registerWebMcp(): Promise<void> {
  setRegisteredWebMcpTools([]);
  const modelContext = document.modelContext as ModelContext | undefined;
  const environment = webMcpEnvironmentState();
  if (environment.origin_agent_cluster === false) {
    $("webmcpCapability").textContent = "WebMCP unavailable: this document is not origin-isolated; open the top-level page in a supported browser. Editor fallback remains active.";
    return;
  }
  if (environment.tools_permission_allowed === false) {
    $("webmcpCapability").textContent = "WebMCP unavailable: the tools Permissions Policy denies this browsing context; open the page directly or delegate tools from a trusted embedder. Editor fallback remains active.";
    return;
  }
  if (!modelContext?.registerTool) {
    $("webmcpCapability").textContent = "WebMCP browser API unavailable in this browsing context; use Chrome 150+ with the testing flag or Chrome 149+ with the origin trial. The editor remains fully usable.";
    return;
  }
  const searchCode = async (query = "", { signal }: ToolExecutionOptions = {}): Promise<SearchResult> => {
    const results: SearchMatch[] = [];
    const normalizedQuery = typeof query === "string" ? query.toLowerCase() : "";
    let scannedFiles = 0;
    let scannedBytes = 0;
    let resultBytes = 0;
    let truncated = false;
    const output = () => ({
      matches: results,
      truncated,
      scanned_files: scannedFiles,
      scanned_bytes: scannedBytes,
      ...(truncated
        ? { hint: "Results are truncated; narrow the query or inspect the returned paths." }
        : results.length === 0
          ? { hint: "No matches found; try a shorter or different query." }
          : {}),
    });
    for (const path of paths()) {
      if (signal?.aborted) throw signal.reason || new Error("The WebMCP search was aborted");
      if (scannedFiles >= MAX_SEARCH_FILES || scannedBytes >= MAX_SEARCH_BYTES) {
        truncated = true;
        break;
      }
      let content = current(path);
      if (!snapshot(path)) {
        const file = await backend.readFile(path);
        if (typeof file?.content !== "string") throw new Error(`Backend returned invalid content for ${path}`);
        content = file.content;
      }
      const bytes = contentSizeBytes(content);
      if (scannedBytes + bytes > MAX_SEARCH_BYTES) {
        truncated = true;
        break;
      }
      scannedFiles += 1;
      scannedBytes += bytes;
      for (const [line, text] of content.split("\n").entries()) {
        if (text.toLowerCase().includes(normalizedQuery)) {
          const result: SearchMatch = { path, line: line + 1, text };
          const nextBytes = contentSizeBytes(JSON.stringify(result));
          if (resultBytes + nextBytes > MAX_WEBMCP_RESULT_BYTES) {
            truncated = true;
            return output();
          }
          resultBytes += nextBytes;
          results.push(result);
        }
        if (results.length >= 200) {
          truncated = true;
          return output();
        }
      }
    }
    return output();
  };
  const tools = instrumentWebMcpTools(createWebMcpTools({
    listFiles: async () => (await backend.listFiles()).map((entry) => typeof entry === "string" ? entry : entry.path),
    readFile: readCurrentFile,
    searchCode,
    getEditorState: editorStateForWebMcp,
    openFile: (path: string) => openFile(path),
    stageTextEdit: stageTextEditForWebMcp,
    reviewDraft: reviewDraftForWebMcp,
    openPanel: openPanelForWebMcp,
  }));
  webMcpRegistration?.dispose();
  webMcpRegistration = null;
  try {
    const registration = await registerWebMcpTools(modelContext, tools, {
      onToolChange: (names) => {
        $("webmcpCapability").textContent = `WebMCP tools available: ${names.length}; browser agent tool set changed.`;
      },
      onToolsDiscovered: (names) => {
        setRegisteredWebMcpTools(names);
        recordWebMcpDiscovery(names, "document.modelContext.getTools");
      },
    });
    webMcpRegistration = registration;
    $("webmcpCapability").textContent = registration
      ? `WebMCP tools registered: ${registration.names.length} editor and workspace tools.`
      : "WebMCP browser API unavailable in this browsing context; editor fallback remains active.";
  } catch (error: unknown) {
    $("webmcpCapability").textContent = `WebMCP registration failed; editor fallback remains active (${message(error)}).`;
  }
}

async function run(action: () => void | Promise<void>): Promise<void> {
  try {
    await action();
  } catch (error) {
    const detail = message(error);
    const code = errorCode(error);
    if (code === "unsupported") $("turnOutput").textContent = detail;
    log(`Action failed: ${detail}`);
    if (code === "unauthorized" || code === "pairing_expired") {
      openConnectionPanel();
      $("connectionSummary").textContent = "New pairing required";
      $("pairingCode").value = "";
      status("VT Code pairing expired", `For an active session rerun \`/webmcp pair ${browserOrigin()}\`; for a standalone bridge restart \`vtcode webmcp serve\`. Then enter the new URL and one-time code.`);
    } else if (code === "connection_closed") {
      status("VT Code bridge disconnected", "Keep the current bridge running; the browser will reconnect automatically. If it was restarted, pair again with its newest URL and code.");
    } else if (error instanceof BackendError && error.proposalRecovery) {
      status("Proposal became stale", error.proposalRecovery);
    } else {
      status(code === "unsupported" ? "VT Code turn unavailable" : "Action could not complete", detail);
    }
    toast(detail);
  }
}

function confirmAction(title: string, copy: string, label: string, action: () => void | Promise<void>): void {
  const dialog = $("confirmDialog");
  if ($("settingsDialog").open) $("settingsDialog").close();
  if ($("evidenceDialog").open) $("evidenceDialog").close();
  $("dialogTitle").textContent = title;
  $("dialogCopy").textContent = copy;
  $("dialogConfirm").textContent = label;
  $("dialogConfirm").onclick = (event: MouseEvent) => { event.preventDefault(); dialog.close(); void run(action); };
  dialog.showModal();
}

interface QuickAction {
  readonly id: string;
  readonly label: string;
  readonly description: string;
  readonly shortcut?: string;
  readonly enabled?: () => boolean;
  readonly execute: () => void | Promise<void>;
}

const QUICK_ACTIONS: readonly QuickAction[] = [
  {
    id: "review",
    label: "Review changes",
    description: "Create a unified diff from the current draft.",
    shortcut: "⌘/Ctrl S",
    enabled: () => dirtyPaths().length > 0,
    execute: reviewChanges,
  },
  {
    id: "reload",
    label: "Reload file",
    description: "Read the selected file from the current backend.",
    enabled: () => Boolean(state.selected),
    execute: reloadFile,
  },
  {
    id: "discard",
    label: "Discard draft",
    description: "Remove the selected unsent browser draft.",
    enabled: () => Boolean(state.selected && isDirty(state.selected)),
    execute: discardDraft,
  },
  {
    id: "run-checks",
    label: "Run checks",
    description: "Run the checks allowed by the selected backend.",
    enabled: () => !$('runChecks').disabled,
    execute: runChecks,
  },
  {
    id: "filter",
    label: "Focus file filter",
    description: "Search workspace paths in the explorer.",
    shortcut: "⌘/Ctrl K",
    execute: () => $("searchInput").focus(),
  },
  {
    id: "settings",
    label: "Open settings",
    description: "Manage workspace setup and VT Code pairing.",
    shortcut: "⌘/Ctrl ,",
    execute: openSettingsDialog,
  },
  {
    id: "changes",
    label: "Open changes panel",
    description: "Review staged and proposed changes.",
    execute: () => selectTerminal("changes"),
  },
  {
    id: "turn",
    label: "Open VT Code panel",
    description: "Compose an agent turn for an active VT Code session.",
    execute: () => selectTerminal("turn"),
  },
  {
    id: "pair",
    label: "Open pairing settings",
    description: "Connect this editor to a local VT Code bridge.",
    execute: openConnectionPanel,
  },
  {
    id: "workspace",
    label: "Open workspace settings",
    description: "Generate terminal commands for a workspace.",
    execute: openWorkspaceSetup,
  },
  {
    id: "self-check",
    label: "Run self-check",
    description: "Exercise the fallback review, apply, check, and revert flow.",
    enabled: () => !dirtyPaths().length
      && !state.serverProposal
      && !state.lastChange
      && Boolean(state.selected)
      && !$('runChecks').disabled,
    execute: runSelfCheck,
  },
  {
    id: "help",
    label: "Show keyboard help",
    description: "Open shortcut and workflow help.",
    shortcut: "?",
    execute: openHelp,
  },
];

function actionEnabled(action: QuickAction): boolean {
  return !action.enabled || action.enabled();
}

function executeQuickAction(action: QuickAction): void {
  if (!actionEnabled(action)) {
    renderQuickActions($("quickActionSearch").value);
    return;
  }
  $("quickActionDialog").close();
  void run(action.execute);
}

function renderQuickActions(query = ""): void {
  const normalized = query.trim().toLowerCase();
  const actions = QUICK_ACTIONS.filter((action) => {
    const searchable = `${action.id} ${action.label} ${action.description}`.toLowerCase();
    return !normalized || searchable.includes(normalized);
  });
  const list = $("quickActionList");
  list.replaceChildren();
  if (!actions.length) {
    const empty = document.createElement("div");
    empty.className = "quick-action-empty";
    empty.textContent = "No matching actions";
    list.append(empty);
    return;
  }
  for (const action of actions) {
    const button = document.createElement("button");
    button.type = "button";
    button.className = "quick-action";
    button.disabled = !actionEnabled(action);
    button.title = action.description;
    button.onclick = () => executeQuickAction(action);

    const copy = document.createElement("span");
    copy.className = "quick-action-copy";
    const label = document.createElement("span");
    label.className = "quick-action-label";
    label.textContent = action.label;
    const description = document.createElement("span");
    description.className = "quick-action-description";
    description.textContent = action.description;
    copy.append(label, description);
    button.append(copy);
    if (action.shortcut) {
      const shortcut = document.createElement("kbd");
      shortcut.className = "quick-action-shortcut";
      shortcut.textContent = action.shortcut;
      button.append(shortcut);
    }
    list.append(button);
  }
}

function openQuickActions() {
  const dialog = $("quickActionDialog");
  if (dialog.open) {
    dialog.close();
    return;
  }
  if ($("confirmDialog").open) return;
  if ($("helpDialog").open) $("helpDialog").close();
  if ($("settingsDialog").open) $("settingsDialog").close();
  if ($("evidenceDialog").open) $("evidenceDialog").close();
  $("quickActionSearch").value = "";
  renderQuickActions();
  dialog.showModal();
  $("quickActionSearch").focus();
}

function openHelp() {
  const dialog = $("helpDialog");
  if (dialog.open) {
    dialog.close();
    return;
  }
  if ($("confirmDialog").open) return;
  if ($("quickActionDialog").open) $("quickActionDialog").close();
  if ($("settingsDialog").open) $("settingsDialog").close();
  if ($("evidenceDialog").open) $("evidenceDialog").close();
  dialog.showModal();
  dialog.querySelector("button")?.focus();
}

function isTypingTarget(target: EventTarget | null): boolean {
  return target instanceof Element && Boolean(
    target.matches("input, textarea, select, [contenteditable='true']") || target.closest(".cm-editor"),
  );
}

async function copyText(value: string): Promise<void> {
  if (!navigator.clipboard?.writeText) throw new Error("Clipboard access is unavailable");
  await navigator.clipboard.writeText(value);
}

$("workspacePath").value = persistedBrowserSettings?.workspace_path || persistedBrowserState?.workspace_path || "";
$("bridgeUrl").value = persistedBrowserSettings?.bridge_url || "";
$("workspacePath").oninput = () => {
  renderWorkspaceSetup();
  persistBrowserSettings();
  persistBrowserWorkspace();
};
$("bridgeUrl").oninput = persistBrowserSettings;
renderWorkspaceSetup();
renderSettings();
renderEvidence();
$("searchInput").oninput = search;
for (const tab of document.querySelectorAll<HTMLElement>("[data-terminal]")) {
  tab.onclick = () => {
    const panel = panelFromValue(tab.dataset.terminal);
    if (panel) selectTerminal(panel);
  };
}
$("reviewChanges").onclick = () => run(reviewChanges);
$("approvePatch").onclick = () => backend.kind === "fallback"
  ? confirmAction("Approve this patch?", "Approval stages an in-memory proposal; it does not write to a local filesystem.", "Approve patch", requestApproval)
  : run(requestApproval);
$("applyPatch").onclick = () => backend.kind === "fallback"
  ? confirmAction("Apply this patch?", "The approved proposal will change only the in-memory fallback project.", "Apply patch", applyProposal)
  : run(applyProposal);
$("revertPatch").onclick = () => backend.kind === "fallback"
  ? confirmAction("Revert last change?", "The backend will validate the current snapshot before restoring it.", "Revert change", revertLastChange)
  : run(revertLastChange);
$("reloadFile").onclick = () => run(reloadFile);
$("discardDraft").onclick = () => run(discardDraft);
$("runChecks").onclick = () => run(runChecks);
$("requestTurn").onclick = () => run(requestTurn);
$("connectBridge").onclick = () => run(connect);
$("closeSettings").onclick = () => $("settingsDialog").close();
$("copyPairingCommand").onclick = () => run(copyPairingCommand);
$("settingsButton").onclick = openSettingsDialog;
$("copyActiveSetup").onclick = () => run(() => copySetupCommand("activeSetupCommand", "Active"));
$("copyHeadlessSetup").onclick = () => run(() => copySetupCommand("headlessSetupCommand", "Workspace"));
$("showConnection").onclick = openConnectionPanel;
$("selfCheck").onclick = () => run(runSelfCheck);
$("quickActions").onclick = openQuickActions;
$("helpButton").onclick = openHelp;
$("evidenceButton").onclick = openEvidenceDialog;
$("beginEvidence").onclick = beginWebMcpEvidence;
$("copyEvidence").onclick = () => run(copyWebMcpEvidence);
$("downloadEvidence").onclick = downloadWebMcpEvidence;
$("closeEvidence").onclick = () => $("evidenceDialog").close();
$("quickActionSearch").oninput = () => renderQuickActions($("quickActionSearch").value);
$("quickActionSearch").onkeydown = (event: KeyboardEvent) => {
  if (event.key === "ArrowDown") {
    event.preventDefault();
    $("quickActionList").querySelector<HTMLElement>(".quick-action:not(:disabled)")?.focus();
  } else if (event.key === "Enter") {
    const available = [...$("quickActionList").querySelectorAll<HTMLButtonElement>(".quick-action:not(:disabled)")];
    if (available.length === 1) {
      event.preventDefault();
      available[0]?.click();
    }
  }
};
document.addEventListener("keydown", (event) => {
  if ((event.metaKey || event.ctrlKey) && event.shiftKey && event.code === "KeyP") {
    event.preventDefault();
    openQuickActions();
    return;
  }
  if ((event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey
    && event.key === "Enter" && event.target === $("promptInput") && !$("requestTurn").disabled) {
    event.preventDefault();
    $("requestTurn").click();
    return;
  }
  if ((event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey
    && event.code === "Comma" && (!isTypingTarget(event.target) || $("settingsDialog").open)) {
    event.preventDefault();
    openSettingsDialog();
    return;
  }
  if (event.key === "?" && !isTypingTarget(event.target)) {
    event.preventDefault();
    openHelp();
    return;
  }
  if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
    event.preventDefault();
    $("searchInput").focus();
  }
});

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "hidden") flushBrowserPersistence({ silent: true });
});
globalThis.addEventListener?.("pagehide", () => flushBrowserPersistence({ silent: true }));

async function init() {
  try {
    await loadWorkspace(backend, persistedBrowserState);
    log(persistedBrowserState ? "WebMCP app ready · restored browser workspace" : "WebMCP app ready · deterministic fallback workspace");
    status("Ready for inspection", persistedBrowserState
      ? "Restored the fallback workspace and drafts for this Vite app instance."
      : "This is a real editor; fallback mode keeps all changes in page memory.");
    await registerWebMcp();
    if (backend.kind === "fallback") openConnectionPanel();
    else openQuickActions();
  } catch (error) {
    status("Backend unavailable", message(error));
    toast(message(error));
  }
}

void init();
