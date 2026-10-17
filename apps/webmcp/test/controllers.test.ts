import test, { type TestContext } from "node:test";
import assert from "node:assert/strict";
import { createEvidenceController } from "../src/evidence-controller.ts";
import { createSettingsController } from "../src/settings-controller.ts";
import { createWebMcpEvidenceRecorder } from "../src/webmcp-evidence.ts";
import { loadBrowserSettings, type StorageLike } from "../src/persistence.ts";
import type { EditorStateForWebMcp, StatusPayload, WebMcpEnvironmentState } from "../src/types.ts";
import type { WebMcpTool } from "../src/webmcp.ts";

class Control {
  textContent = "";
  value = "";
  className = "";
  disabled = false;
  open = false;
  focused = false;
  showModal(): void { this.open = true; }
  close(): void { this.open = false; }
  focus(): void { this.focused = true; }
}

function installDom(t: TestContext) {
  const controls = new Map<string, Control>();
  const control = (id: string): Control => {
    let element = controls.get(id);
    if (!element) { element = new Control(); controls.set(id, element); }
    return element;
  };
  for (const [key, value] of Object.entries({
    document: { getElementById: control },
    location: { origin: "https://example.test" },
    navigator: { userAgent: "Controller test" },
  })) {
    const original = Object.getOwnPropertyDescriptor(globalThis, key);
    Object.defineProperty(globalThis, key, { configurable: true, value });
    t.after(() => {
      if (original) Object.defineProperty(globalThis, key, original);
      else Reflect.deleteProperty(globalThis, key);
    });
  }
  return control;
}

function feedback() {
  const copied: string[] = [];
  const statuses: string[] = [];
  const toasts: string[] = [];
  return {
    copied, statuses, toasts,
    log: (_text: string) => {},
    status: (title: string, _detail: string) => { statuses.push(title); },
    toast: (text: string) => { toasts.push(text); },
    copyText: async (value: string) => { copied.push(value); },
  };
}

const environment: WebMcpEnvironmentState = {
  browsing_context_required: true, origin_agent_cluster: true, tools_permission_allowed: true,
};
function editorState(): EditorStateForWebMcp {
  return {
    backend: "fallback", connected: false, workspace_root: null, bridge_settings: null,
    authenticated_origin: null, selected: "src/old.ts", open_tabs: [], dirty_files: [],
    has_client_proposal: false, has_server_proposal: false, active_panel: "activity",
    workflow_state: "file_selected", recommended_next_tools: [], webmcp_context: environment,
  };
}
function tool(execute: WebMcpTool["execute"]): WebMcpTool {
  return {
    name: "read_file", title: "Read file", description: "Read a bounded snapshot",
    inputSchema: { type: "object", properties: {} },
    annotations: { readOnlyHint: true, untrustedContentHint: true }, execute,
  };
}

const bridgeStatus: StatusPayload = {
  protocol_version: "1", connected: true, latest_sequence: 0,
  authenticated_origin: "https://example.test",
  runtime: {
    workspace_root: "/workspace/paired", connected: true, turns_available: true,
    mutations_allowed: false, checks_allowed: false, approval_authority: "terminal",
  },
  settings: {
    host: "127.0.0.1", port: 0, pairing_ttl_secs: 300,
    max_frame_bytes: 1048576, max_in_flight_requests: 8, remote_enabled: false,
  },
};

test("evidence instrumentation forwards tool metadata and observes live post-call state", async (t) => {
  const control = installDom(t);
  const recorder = createWebMcpEvidenceRecorder();
  let state = editorState();
  const ui = createEvidenceController({
    recorder, getEditorState: () => state, getEnvironment: () => environment, ...feedback(),
  });
  const input = { path: "src/new.ts", content: "private source" };
  const options = { signal: new AbortController().signal };
  const result = { path: "src/new.ts", content: "private result", size_bytes: 17 };
  const original = tool(async (actualInput, actualOptions) => {
    assert.equal(actualInput, input);
    assert.equal(actualOptions, options);
    state = { ...state, selected: "src/new.ts", active_panel: "changes" };
    return result;
  });
  const wrapped = ui.instrumentWebMcpTools([original])[0]!;
  assert.equal(wrapped.inputSchema, original.inputSchema);
  assert.equal(wrapped.annotations, original.annotations);
  assert.equal(wrapped.title, original.title);
  assert.equal(await wrapped.execute(input, options), result);
  const call = recorder.snapshot().records[0]!;
  assert.equal(call.kind, "tool_call");
  if (call.kind !== "tool_call") return;
  assert.deepEqual(call.input, { path: "src/new.ts", content: "[omitted]" });
  assert.deepEqual(call.result_metadata, { path: "src/new.ts", size_bytes: 17 });
  assert.equal((call.editor_state as Record<string, unknown>).selected, "src/new.ts");
  assert.equal((call.editor_state as Record<string, unknown>).active_panel, "changes");
  assert.ok(call.duration_ms >= 0);
  assert.equal(control("evidenceOutput").textContent, recorder.toJson());
});

test("evidence capture failures preserve successful results and original tool errors", async (t) => {
  installDom(t);
  const recorder = createWebMcpEvidenceRecorder();
  const ui = createEvidenceController({
    recorder: { ...recorder, recordToolCall: () => { throw new Error("Recorder failed"); } },
    getEditorState: editorState, getEnvironment: () => environment, ...feedback(),
  });
  const result = { unchanged: true };
  const failure = new Error("Original tool failure");
  const wrapped = ui.instrumentWebMcpTools([
    tool(async () => result), tool(async () => { throw failure; }),
  ]);
  assert.equal(await wrapped[0]!.execute(), result);
  await assert.rejects(wrapped[1]!.execute(), (error) => error === failure);
  const unavailableState = createEvidenceController({
    recorder, getEditorState: () => { throw new Error("State unavailable"); },
    getEnvironment: () => environment, ...feedback(),
  });
  assert.equal(await unavailableState.instrumentWebMcpTools([tool(async () => result)])[0]!.execute(), result);
});

test("evidence sessions copy current discovery names, reset stale names, and render literal text", async (t) => {
  const control = installDom(t);
  const recorder = createWebMcpEvidenceRecorder();
  const ui = createEvidenceController({
    recorder, getEditorState: editorState, getEnvironment: () => environment, ...feedback(),
  });
  const names = ["read_file"];
  ui.setRegisteredWebMcpTools(names);
  names.push("should_not_appear");
  control("evidenceClient").value = "<script>client label</script>";
  ui.beginWebMcpEvidence();
  assert.match(control("evidenceSummary").textContent, /^<script>client label<\/script> · 1 discovery event · 0 tool calls$/);
  const discovery = recorder.snapshot().records[0]!;
  assert.equal(discovery.kind, "discovery");
  if (discovery.kind !== "discovery") return;
  assert.deepEqual(discovery.tool_names, ["read_file"]);
  assert.equal(control("copyEvidence").disabled, false);
  ui.setRegisteredWebMcpTools([]);
  ui.beginWebMcpEvidence();
  assert.equal(recorder.snapshot().records.length, 0);
  control("confirmDialog").open = true;
  ui.openEvidenceDialog();
  assert.equal(control("evidenceDialog").open, false);
  control("confirmDialog").open = false;
  control("settingsDialog").open = true;
  ui.openEvidenceDialog();
  assert.equal(control("settingsDialog").open, false);
  assert.equal(control("evidenceDialog").open, true);
  assert.equal(control("beginEvidence").focused, true);
  ui.openEvidenceDialog();
  assert.equal(control("evidenceDialog").open, false);
});

test("settings render the current backend after pairing and disconnection", (t) => {
  const control = installDom(t);
  type Backend = Parameters<typeof createSettingsController>[0]["getBackend"];
  let current: ReturnType<Backend> = { kind: "fallback", statusPayload: null };
  const ui = createSettingsController({
    getBackend: () => current, getStorage: () => null, appInstance: "test", ...feedback(),
  });
  ui.renderSettings();
  assert.equal(control("settingsRuntime").textContent, "Fallback");
  assert.equal(control("settingsConnection").textContent, "In-memory fallback");
  current = { kind: "websocket", connected: true, url: "ws://127.0.0.1:9021/webmcp", statusPayload: bridgeStatus };
  ui.renderSettings();
  assert.equal(control("settingsConnection").textContent, "ws://127.0.0.1:9021/webmcp");
  assert.equal(control("settingsRuntime").textContent, "Active VT Code TUI");
  assert.equal(control("settingsWorkspace").textContent, "/workspace/paired");
  assert.equal(control("settingsLimits").textContent, "1 MiB · 8 in flight");
  assert.equal(control("settingsListener").textContent, "127.0.0.1:auto · loopback");
  assert.equal(control("settingsPairingTtl").textContent, "300 seconds");
  assert.match(control("settingsSyncNote").textContent, /terminal owns workspace roots, policy, pairing, and writes/);
  current = { ...current, connected: false, statusPayload: null };
  ui.renderSettings();
  assert.equal(control("settingsSyncState").textContent, "Pairing required");
  assert.equal(control("settingsConnection").textContent, "Bridge disconnected");
  assert.equal(control("settingsWorkspace").textContent, "Not reported");
});

test("settings quote setup paths, preserve modal guards, and choose connection focus", async (t) => {
  const control = installDom(t);
  const effects = feedback();
  const ui = createSettingsController({
    getBackend: () => ({ kind: "fallback", statusPayload: null }),
    getStorage: () => null, appInstance: "test", ...effects,
  });
  control("workspacePath").value = "/tmp/a'b; $(printf injected)";
  await ui.copySetupCommand("activeSetupCommand", "Active");
  assert.equal(effects.copied[0], "vtcode --workspace '/tmp/a'\\''b; $(printf injected)' chat\n\nThen in the TUI:\n/webmcp pair https://example.test");
  control("confirmDialog").open = true;
  ui.openSettingsDialog();
  assert.equal(control("settingsDialog").open, false);
  control("confirmDialog").open = false;
  control("evidenceDialog").open = true;
  ui.openConnectionPanel();
  assert.equal(control("evidenceDialog").open, false);
  assert.equal(control("settingsDialog").open, true);
  assert.equal(control("connectionPanel").open, true);
  assert.equal(control("workspaceSetupPanel").open, false);
  assert.equal(control("bridgeUrl").focused, true);
  control("bridgeUrl").value = "ws://127.0.0.1:9021/webmcp";
  ui.openConnectionPanel();
  assert.equal(control("pairingCode").focused, true);
  ui.openWorkspaceSetup();
  assert.equal(control("connectionPanel").open, false);
  assert.equal(control("workspaceSetupPanel").open, true);
  assert.equal(control("workspacePath").focused, true);
});

test("settings persistence saves only setup values and resets warnings after recovery", (t) => {
  const control = installDom(t);
  const effects = feedback();
  const values = new Map<string, string>();
  let storage: StorageLike | null = null;
  const ui = createSettingsController({
    getBackend: () => ({ kind: "fallback", statusPayload: null }),
    getStorage: () => storage, appInstance: "test", ...effects,
  });
  control("workspacePath").value = "  /workspace/setup  ";
  control("bridgeUrl").value = "  ws://127.0.0.1:9021/webmcp  ";
  control("pairingCode").value = "do-not-store-this-code";
  assert.equal(ui.persistBrowserSettings(), false);
  assert.equal(ui.persistBrowserSettings(), false);
  assert.deepEqual(effects.toasts, ["Settings could not be saved"]);
  storage = {
    getItem: (key) => values.get(key) ?? null,
    setItem: (key, value) => { values.set(key, value); },
    removeItem: (key) => { values.delete(key); },
  };
  assert.equal(ui.persistBrowserSettings(), true);
  assert.deepEqual(loadBrowserSettings(storage, "test"), {
    version: 1, app_instance: "test", workspace_path: "/workspace/setup", bridge_url: "ws://127.0.0.1:9021/webmcp",
  });
  assert.ok([...values.values()].every((value) => !value.includes("do-not-store-this-code")));
  storage = null;
  assert.equal(ui.persistBrowserSettings(), false);
  assert.equal(effects.toasts.length, 2);
});
