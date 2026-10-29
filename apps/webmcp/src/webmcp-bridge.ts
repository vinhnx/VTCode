import { $ } from "./app-elements.ts";
import { MAX_FILE_BYTES, digest } from "./backend.ts";
import { contentSizeBytes, message, panelFromValue, selectTerminal, truncateUtf8, webMcpFileNotFoundError } from "./app-feedback.ts";
import { createWebMcpTools, registerWebMcpTools, replaceExactText, type ModelContext, type StageTextEditInput, type ToolExecutionOptions, type WebMcpRegistration, type WebMcpTool } from "./webmcp.ts";
import { BackendError, errorCode, type EditorStateForWebMcp, type FileSnapshot, type Panel, type RuntimeStatus, type SearchMatch, type SearchResult, type WebMcpEnvironmentState } from "./types.ts";
import type { CodeEditor } from "./editor.ts";
import { current, dirtyPaths, isDirty, paths, snapshot, type AppBackend, type AppState } from "./workspace-view.ts";

const MAX_SEARCH_FILES = 128;
const MAX_SEARCH_BYTES = 8 * 1024 * 1024;
const MAX_WEBMCP_RESULT_BYTES = 128 * 1024;

export function webMcpEnvironmentState(): WebMcpEnvironmentState {
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

export interface WebMcpBridgeOptions {
  readonly state: AppState;
  readonly editor: CodeEditor;
  readonly getBackend: () => AppBackend;
  readonly runtimeStatus: () => RuntimeStatus | undefined;
  readonly isBridgeConnected: () => boolean;
  readonly openFile: (path: string, record?: boolean) => Promise<void>;
  readonly reviewChanges: () => Promise<void>;
  readonly renderTree: () => void;
  readonly renderTabs: () => void;
  readonly renderProposal: () => void;
  readonly updateEditorFooter: () => void;
  readonly persist: () => boolean;
  readonly recordWebMcpDiscovery: (toolNames: readonly string[], source: string) => void;
  readonly instrumentWebMcpTools: (tools: readonly WebMcpTool[]) => WebMcpTool[];
  readonly setRegisteredWebMcpTools: (names: readonly string[]) => void;
  readonly log: (text: string) => void;
  readonly status: (title: string, detail: string) => void;
  readonly toast: (text: string) => void;
}

/** Browser agent tool surface: read-only inspection plus staged draft edits that always require review. */
export function createWebMcpBridge(options: WebMcpBridgeOptions) {
  const {
    state, editor, getBackend, runtimeStatus, isBridgeConnected,
    openFile, reviewChanges, renderTree, renderTabs, renderProposal, updateEditorFooter,
    persist, recordWebMcpDiscovery, instrumentWebMcpTools, setRegisteredWebMcpTools,
    log, status, toast,
  } = options;
  let webMcpRegistration: WebMcpRegistration | null = null;

  async function readCurrentFile(path: string): Promise<FileSnapshot> {
    if (!state.files.has(path)) throw webMcpFileNotFoundError(path);
    const draft = state.drafts.get(path);
    const base = snapshot(state, path);
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
    const file = await getBackend().readFile(path);
    return { ...file, draft: false };
  }

  function editorStateForWebMcp(): EditorStateForWebMcp {
    const dirtyFiles = dirtyPaths(state);
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
      backend: getBackend().kind,
      connected: isBridgeConnected(),
      workspace_root: runtimeStatus()?.workspace_root || null,
      bridge_settings: getBackend().statusPayload?.settings || null,
      authenticated_origin: getBackend().statusPayload?.authenticated_origin || null,
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
    if (!snapshot(state, path)) await openFile(path, false);
    if (signal?.aborted) throw signal.reason || new Error("The WebMCP edit was aborted");
    if (isDirty(state, path)) {
      throw new BackendError(`Draft already contains changes for ${path}; call review_draft or discard the draft before staging another edit`, "draft_conflict");
    }
    const base = snapshot(state, path);
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
    persist();
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
      for (const path of paths(state)) {
        if (signal?.aborted) throw signal.reason || new Error("The WebMCP search was aborted");
        if (scannedFiles >= MAX_SEARCH_FILES || scannedBytes >= MAX_SEARCH_BYTES) {
          truncated = true;
          break;
        }
        let content = current(state, path);
        if (!snapshot(state, path)) {
          const file = await getBackend().readFile(path);
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
      listFiles: async () => (await getBackend().listFiles()).map((entry) => typeof entry === "string" ? entry : entry.path),
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

  return {
    readCurrentFile, editorStateForWebMcp, stageTextEditForWebMcp,
    reviewDraftForWebMcp, registerWebMcp,
  };
}
