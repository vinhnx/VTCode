import { connectVtCode, createBackend } from "./backend.ts";
import { $ } from "./app-elements.ts";
import { copyText, isTypingTarget, log, message, panelFromValue, selectTerminal, status, toast } from "./app-feedback.ts";
import { createAppState, createWorkspaceView, current, dirtyPaths, isDirty, paths, snapshot, type AppBackend } from "./workspace-view.ts";
import { diagramSourceFor, isDiagramTooLarge, mountDiagramPreview } from "./diagrams.ts";
import type { Mounted } from "pele";
import { createProposalFlow } from "./proposal-flow.ts";
import { createWebMcpBridge, webMcpEnvironmentState } from "./webmcp-bridge.ts";
import { createEvidenceController } from "./evidence-controller.ts";
import { createSettingsController } from "./settings-controller.ts";
import { CodeEditor } from "./editor.ts";
import { browserOrigin } from "./deployments.ts";
import { loadBrowserSettings, loadBrowserState, saveBrowserState, type StorageLike } from "./persistence.ts";
import { createWebMcpEvidenceRecorder } from "./webmcp-evidence.ts";
import { BackendError, errorCode, type BackendConnectionEvent, type BackendEvent, type PersistedBrowserState, type RuntimeStatus, type StatusPayload, type WorkspaceFile } from "./types.ts";
import "../styles.css";
import { mountExplanation } from "./explanation.ts";

const MAX_HYDRATION_EVENTS = 256;
const PERSIST_DEBOUNCE_MS = 250;
const APP_INSTANCE = typeof __VTCODE_APP_INSTANCE__ === "string" ? __VTCODE_APP_INSTANCE__ : "development";

function browserStorage(): StorageLike | null {
  try { return globalThis.localStorage; } catch { return null; }
}

const persistedBrowserState = loadBrowserState(browserStorage(), APP_INSTANCE);
const persistedBrowserSettings = loadBrowserSettings(browserStorage(), APP_INSTANCE);
let backend: AppBackend = createBackend(persistedBrowserState?.fallback_files);
const explanationView = mountExplanation(() => backend);
let fallbackPersistenceTimer: ReturnType<typeof setTimeout> | null = null;
let persistenceWarningShown = false;

const state = createAppState();

const editor = new CodeEditor($("editor"), {
  onChange: (path, content) => {
    if (content === state.snapshots.get(path)?.content) state.drafts.delete(path);
    else state.drafts.set(path, content);
    editor.updateDirty(isDirty(state, path));
    state.clientProposal = null;
    state.serverProposal = null;
    state.approved = false;
    state.pendingTerminalApproval = false;
    persistBrowserWorkspace();
    renderTree();
    renderTabs();
    renderProposal();
    updateEditorFooter();
    refreshDiagramAction();
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
  getEditorState: () => bridge.editorStateForWebMcp(),
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

const {
  renderTree, renderTabs, updateEditorFooter, renderSelectedEditor, refreshDiagramAction,
  openFile, refreshFile, invalidatePendingOpens,
} = createWorkspaceView({
  state, editor,
  getBackend: () => backend,
  persist: () => persistBrowserWorkspace(),
  run, log, status,
});

const {
  renderProposal, reviewChanges, requestApproval, applyProposal,
  revertLastChange, runChecks, requestTurn, runSelfCheck,
} = createProposalFlow({
  state, editor,
  getBackend: () => backend,
  runtimeStatus, isHeadlessBridge, isActiveRuntime,
  refreshFile, renderTree, renderTabs, renderSelectedEditor, updateEditorFooter,
  openConnectionPanel,
  persist: () => persistBrowserWorkspace(),
  log, status, toast,
});

const bridge = createWebMcpBridge({
  state, editor,
  getBackend: () => backend,
  runtimeStatus, isBridgeConnected,
  openFile, reviewChanges, renderTree, renderTabs, renderProposal, updateEditorFooter,
  persist: () => persistBrowserWorkspace(),
  recordWebMcpDiscovery, instrumentWebMcpTools, setRegisteredWebMcpTools,
  log, status, toast,
});
const { registerWebMcp } = bridge;

function recordRuntimeEvent(event: BackendEvent): void {
  explanationView.refresh();
  const sequence = event.type === "event" ? ` #${event.sequence}` : "";
  log(`Runtime event${sequence}`);
  status("Runtime event received", "Refresh a clean file to inspect the latest backend snapshot.");
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

async function reloadFile(): Promise<void> {
  if (!state.selected) return;
  await refreshFile(state.selected);
  log(`Reloaded ${state.selected} from ${backend.kind}`);
  status("File reloaded", "The draft buffer was compared with the backend snapshot.");
}

function discardDraft(): void {
  if (!state.selected || !isDirty(state, state.selected)) throw new Error("The selected file has no draft");
  const path = state.selected;
  state.drafts.delete(path);
  state.conflicts.delete(path);
  state.clientProposal = null;
  state.serverProposal = null;
  state.approved = false;
  const base = snapshot(state, path);
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

async function loadWorkspace(nextBackend: AppBackend, restoreState: PersistedBrowserState | null = null): Promise<void> {
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
  invalidatePendingOpens();
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

  const first = state.selected || state.openTabs[0] || paths(state)[0];
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

let activeDiagram: Mounted | null = null;

function openDiagramPreview(): void {
  if ($("diagramDialog").open) return;
  const selected = state.selected;
  const source = selected ? diagramSourceFor(selected, current(state, selected)) : null;
  if (!source) return;
  activeDiagram?.destroy();
  activeDiagram = null;
  const container = $("diagramContainer");
  const errorBox = $("diagramError");
  container.replaceChildren();
  if (isDiagramTooLarge(source)) {
    errorBox.hidden = false;
    errorBox.textContent = "Diagram is too large to preview.";
  } else {
    activeDiagram = mountDiagramPreview(container, errorBox, source);
  }
  $("diagramDialog").showModal();
  $("closeDiagram").focus();
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
    enabled: () => dirtyPaths(state).length > 0,
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
    enabled: () => Boolean(state.selected && isDirty(state, state.selected)),
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
    enabled: () => !dirtyPaths(state).length
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
$("previewDiagram").onclick = () => run(openDiagramPreview);
$("closeDiagram").onclick = () => $("diagramDialog").close();
$("diagramDialog").addEventListener("close", () => { activeDiagram?.destroy(); activeDiagram = null; });
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
