import { $ } from "./app-elements.ts";
import { browserOrigin } from "./deployments.ts";
import type { WebMcpEvidenceRecorder } from "./webmcp-evidence.ts";
import type { ToolExecutionOptions, WebMcpTool } from "./webmcp.ts";
import type { EditorStateForWebMcp, WebMcpEnvironmentState } from "./types.ts";

interface EvidenceControllerOptions {
  readonly recorder: WebMcpEvidenceRecorder;
  readonly getEditorState: () => EditorStateForWebMcp;
  readonly getEnvironment: () => WebMcpEnvironmentState;
  readonly log: (text: string) => void;
  readonly status: (title: string, detail: string) => void;
  readonly toast: (text: string) => void;
  readonly copyText: (value: string) => Promise<void>;
}

/** Evidence presentation and observation never grant workspace write authority. */
export function createEvidenceController(options: EvidenceControllerOptions) {
  const {
    recorder: webMcpEvidence, getEditorState: editorStateForWebMcp,
    getEnvironment: webMcpEnvironmentState, log, status, toast, copyText,
  } = options;
  let registeredWebMcpToolNames: readonly string[] = [];

  function evidenceNowMs(): number {
    return typeof performance?.now === "function" ? performance.now() : Date.now();
  }

  function renderEvidence(): void {
    const evidence = webMcpEvidence.snapshot();
    const discoveryCount = evidence.records.filter((record) => record.kind === "discovery").length;
    const callCount = evidence.records.filter((record) => record.kind === "tool_call").length;
    const session = evidence.session?.client_label || "No client selected";
    $("evidenceSummary").textContent = `${session} · ${discoveryCount} discovery event${discoveryCount === 1 ? "" : "s"} · ${callCount} tool call${callCount === 1 ? "" : "s"}${evidence.dropped_records ? ` · ${evidence.dropped_records} older record${evidence.dropped_records === 1 ? "" : "s"} dropped` : ""}`;
    $("copyEvidence").disabled = !evidence.session;
    $("downloadEvidence").disabled = !evidence.session;
    $("evidenceOutput").textContent = webMcpEvidence.toJson();
  }

  function recordWebMcpDiscovery(toolNames: readonly string[], source: string): void {
    try {
      webMcpEvidence.recordDiscovery(toolNames, source);
      renderEvidence();
    } catch {
      // Evidence capture must never prevent the browser tool surface from registering.
    }
  }

  function recordWebMcpCall(evidence: {
    readonly tool_name: string;
    readonly input: unknown;
    readonly success: boolean;
    readonly result?: unknown;
    readonly error?: unknown;
    readonly duration_ms: number;
  }): void {
    try {
      webMcpEvidence.recordToolCall({
        ...evidence,
        editor_state: editorStateForWebMcp(),
      });
      renderEvidence();
    } catch {
      // Evidence capture must never change the result or failure of a tool call.
    }
  }

  function instrumentWebMcpTools(tools: readonly WebMcpTool[]): WebMcpTool[] {
    return tools.map((tool) => ({
      ...tool,
      execute: async (input: unknown = {}, options: ToolExecutionOptions = {}) => {
        const startedAt = evidenceNowMs();
        try {
          const result = await tool.execute(input, options);
          recordWebMcpCall({
            tool_name: tool.name,
            input,
            success: true,
            result,
            duration_ms: evidenceNowMs() - startedAt,
          });
          return result;
        } catch (error: unknown) {
          recordWebMcpCall({
            tool_name: tool.name,
            input,
            success: false,
            error,
            duration_ms: evidenceNowMs() - startedAt,
          });
          throw error;
        }
      },
    }));
  }

  function beginWebMcpEvidence(): void {
    const clientLabel = $("evidenceClient").value.trim() || "WebMCP client";
    const environment = webMcpEnvironmentState();
    webMcpEvidence.begin({
      client_label: clientLabel,
      origin: browserOrigin(),
      user_agent: navigator.userAgent,
      webmcp_context: {
        browsing_context_required: environment.browsing_context_required,
        origin_agent_cluster: environment.origin_agent_cluster,
        tools_permission_allowed: environment.tools_permission_allowed,
      },
    });
    if (registeredWebMcpToolNames.length) {
      webMcpEvidence.recordDiscovery(registeredWebMcpToolNames, "registered browser tools");
    }
    renderEvidence();
    log(`Started ${clientLabel} WebMCP evidence run`);
    status("Evidence run started", "Use the selected browser client now, then export the sanitized JSON report.");
    toast("WebMCP evidence run started");
  }

  async function copyWebMcpEvidence(): Promise<void> {
    await copyText(webMcpEvidence.toJson());
    log("Copied sanitized WebMCP evidence JSON");
    toast("Evidence JSON copied");
  }

  function downloadWebMcpEvidence(): void {
    const clientLabel = $("evidenceClient").value.trim() || "webmcp-client";
    const slug = clientLabel.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "") || "webmcp-client";
    const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
    const blob = new Blob([webMcpEvidence.toJson()], { type: "application/json" });
    const url = URL.createObjectURL(blob);
    const link = document.createElement("a");
    link.href = url;
    link.download = `webmcp-evidence-${slug}-${timestamp}.json`;
    link.hidden = true;
    document.body.append(link);
    link.click();
    setTimeout(() => {
      link.remove();
      URL.revokeObjectURL(url);
    }, 1_000);
    log("Downloaded sanitized WebMCP evidence JSON");
    toast("Evidence JSON downloaded");
  }

  function openEvidenceDialog(): void {
    const dialog = $("evidenceDialog");
    if (dialog.open) {
      dialog.close();
      return;
    }
    if ($("confirmDialog").open) return;
    if ($("settingsDialog").open) $("settingsDialog").close();
    if ($("quickActionDialog").open) $("quickActionDialog").close();
    if ($("helpDialog").open) $("helpDialog").close();
    renderEvidence();
    dialog.showModal();
    $("beginEvidence").focus();
  }

  function setRegisteredWebMcpTools(names: readonly string[]): void {
    registeredWebMcpToolNames = [...names];
  }

  return {
    renderEvidence, recordWebMcpDiscovery, instrumentWebMcpTools, beginWebMcpEvidence,
    copyWebMcpEvidence, downloadWebMcpEvidence, openEvidenceDialog, setRegisteredWebMcpTools,
  };
}
