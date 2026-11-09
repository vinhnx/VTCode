import { $ } from "./app-elements.ts";
import { MAX_FILE_BYTES, type InMemoryBackend, type VtCodeBackend } from "./backend.ts";
import { fileName, snapshotSizeBytes, webMcpFileNotFoundError } from "./app-feedback.ts";
import { diagramActionState } from "./diagrams.ts";
import type { CodeEditor } from "./editor.ts";
import type { ClientProposal, FileSnapshot, PatchProposal, TreeNode, WorkspaceFile } from "./types.ts";

export type AppBackend = InMemoryBackend | VtCodeBackend;

export interface LastChangeView {
  readonly change_id: string;
  readonly paths: string[];
  readonly before: Array<Pick<FileSnapshot, "path" | "content">>;
}

export interface AppState {
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

export function createAppState(): AppState {
  return {
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
}

export function paths(state: AppState): string[] { return [...state.files.keys()].sort(); }
export function snapshot(state: AppState, path: string): FileSnapshot | undefined { return state.snapshots.get(path); }
export function current(state: AppState, path: string): string { return state.drafts.get(path) ?? snapshot(state, path)?.content ?? ""; }
export function isDirty(state: AppState, path: string): boolean { return state.drafts.has(path) && state.drafts.get(path) !== snapshot(state, path)?.content; }
export function dirtyPaths(state: AppState): string[] { return paths(state).filter((path) => isDirty(state, path)); }

export function expandAncestors(state: AppState, path: string): void {
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

export interface WorkspaceViewOptions {
  readonly state: AppState;
  readonly editor: CodeEditor;
  readonly getBackend: () => AppBackend;
  readonly persist: () => boolean;
  readonly run: (action: () => void | Promise<void>) => Promise<void>;
  readonly log: (text: string) => void;
  readonly status: (title: string, detail: string) => void;
}

/** File explorer, tabs, and editor buffer presentation. Draft review and backend pairing stay in main. */
export function createWorkspaceView(options: WorkspaceViewOptions) {
  const { state, editor, getBackend, persist, run, log, status } = options;
  let openRequest = 0;

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
        persist();
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
      marker.textContent = isDirty(state, path) ? "*" : state.conflicts.has(path) ? "!" : "";
      row.append(icon, label, marker);
      row.onclick = () => run(() => openFile(path));
      parent.append(row);
    }
  }

  function renderTree(): void {
    const tree = $("fileTree");
    tree.replaceChildren();
    const query = state.filter.toLowerCase();
    const visiblePaths = paths(state).filter((path) => !query || path.toLowerCase().includes(query));
    if (visiblePaths.length) appendTreeNode(tree, treeFor(visiblePaths), "", 0, Boolean(query));
    else {
      const empty = document.createElement("div");
      empty.className = "tree-empty";
      empty.textContent = query ? "No matching files" : "No files in this workspace";
      tree.append(empty);
    }
    $("fileCount").textContent = query ? `${visiblePaths.length}/${paths(state).length} files` : `${paths(state).length} files`;
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
      marker.textContent = isDirty(state, path) ? "*" : "";
      button.append(label, marker);
      tabs.append(button);
    }
  }

  function updateEditorFooter(): void {
    if (!editor.view) return;
    const line = editor.view.state.doc.lineAt(editor.view.state.selection.main.head);
    $("cursorPosition").textContent = `Ln ${line.number}, Col ${editor.view.state.selection.main.head - line.from + 1}`;
    const changed = state.selected && isDirty(state, state.selected);
    const conflict = state.selected && state.conflicts.has(state.selected);
    $("editorState").textContent = conflict ? "External change conflict" : changed ? "Dirty draft · not proposed" : "Workspace snapshot";
    $("editorState").classList.toggle("dirty", Boolean(changed || conflict));
  }

  function refreshDiagramAction(): void {
    const selected = state.selected;
    const action = diagramActionState(selected, selected ? current(state, selected) : "");
    const button = $("previewDiagram");
    button.disabled = action.disabled;
    button.title = action.title;
  }

  function renderSelectedEditor(): void {
    refreshDiagramAction();
    if (!state.selected || !snapshot(state, state.selected)) return;
    const path = state.selected;
    editor.open(path, current(state, path), isDirty(state, path));
    $("fileTitle").textContent = path;
    $("fileType").textContent = path.includes(".") ? path.split(".").pop()?.toUpperCase() || "TEXT" : "TEXT";
    updateEditorFooter();
  }

  async function openFile(path: string, record = true): Promise<void> {
    if (!state.files.has(path)) throw webMcpFileNotFoundError(path);
    const request = ++openRequest;
    state.selected = path;
    expandAncestors(state, path);
    if (!snapshot(state, path)) status("Loading file", path);
    if (!snapshot(state, path)) await refreshFile(path, false, false);
    if (request !== openRequest || state.selected !== path) return;
    if (!state.openTabs.includes(path)) state.openTabs.push(path);
    renderSelectedEditor();
    renderTree();
    renderTabs();
    if (record) {
      log(`Inspected ${path}`);
      status("Ready for inspection", "Edit a local draft; save opens review instead of writing immediately.");
    }
    persist();
  }

  async function refreshFile(path: string, force = false, render = true): Promise<void> {
    const fresh = await getBackend().readFile(path);
    if (typeof fresh?.content !== "string") throw new Error(`Backend returned invalid content for ${path}`);
    if (snapshotSizeBytes(fresh) > MAX_FILE_BYTES) throw new Error(`File exceeds the browser size limit: ${path}`);
    const previous = snapshot(state, path);
    if (isDirty(state, path) && !force && previous && fresh.digest !== previous.digest) {
      state.conflicts.add(path);
      throw new Error(`External change conflict for ${path}; discard the draft before reloading`);
    }
    state.snapshots.set(path, fresh);
    state.conflicts.delete(path);
    if (force || !isDirty(state, path)) state.drafts.delete(path);
    if (render && state.selected === path) renderSelectedEditor();
    renderTree();
    renderTabs();
    persist();
  }

  function invalidatePendingOpens(): void {
    openRequest += 1;
  }

  return {
    renderTree, renderTabs, updateEditorFooter, renderSelectedEditor, refreshDiagramAction,
    openFile, refreshFile, invalidatePendingOpens,
  };
}
