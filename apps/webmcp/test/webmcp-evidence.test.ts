import test from "node:test";
import assert from "node:assert/strict";
import { createWebMcpEvidenceRecorder, isJsonSerializable, type WebMcpEvidenceExport } from "../src/webmcp-evidence.ts";

test("records sanitized discovery and successful tool-call evidence", () => {
  let currentTime = 100;
  const recorder = createWebMcpEvidenceRecorder({ now: () => currentTime });
  recorder.begin({
    client_label: "Chrome WebMCP Tool Inspector",
    origin: "https://example.test",
    user_agent: "Chrome/149",
    webmcp_context: { browsing_context_required: true, origin_agent_cluster: true, tools_permission_allowed: true },
  });
  recorder.recordDiscovery(["list_project_files", "read_file"], "ChatGPT\nwindow");
  currentTime = 145;
  recorder.recordToolCall({
    tool_name: "read_file",
    input: { path: "src/main.ts", content: "secret file content", session_token: "session-token" },
    success: true,
    result: { path: "src/main.ts", size_bytes: 42, content: "raw file content", truncated: false },
    duration_ms: 45.4,
    editor_state: {
      selected: "src/main.ts",
      workflow_state: "file_selected",
      workspace_root: "/private/workspace",
      authenticated_origin: "https://example.test",
    },
  });

  const evidence = recorder.snapshot();
  assert.equal(evidence.records.length, 2);
  assert.deepEqual(evidence.session, {
    client_label: "Chrome WebMCP Tool Inspector",
    origin: "https://example.test",
    user_agent: "Chrome/149",
    webmcp_context: { browsing_context_required: true, origin_agent_cluster: true, tools_permission_allowed: true },
  });
  const discovery = evidence.records[0];
  assert.ok(discovery);
  assert.equal(discovery.kind, "discovery");
  if (discovery.kind !== "discovery") return;
  assert.deepEqual(discovery, {
    kind: "discovery",
    recorded_at_ms: 100,
    tool_names: ["list_project_files", "read_file"],
    tool_count: 2,
    source: "ChatGPT\nwindow",
  });
  const call = evidence.records[1];
  assert.ok(call);
  assert.equal(call.kind, "tool_call");
  if (call.kind !== "tool_call") return;
  assert.deepEqual(call.input, { path: "src/main.ts", content: "[omitted]", session_token: "[redacted]" });
  assert.deepEqual(call.result_metadata, { path: "src/main.ts", size_bytes: 42, truncated: false });
  assert.deepEqual(call.editor_state, { selected: "src/main.ts", workflow_state: "file_selected" });
  assert.equal(call.duration_ms, 45);
});

test("records bounded error evidence without leaking error secrets", () => {
  const recorder = createWebMcpEvidenceRecorder({ now: () => 1 });
  recorder.recordToolCall({
    tool_name: "stage_text_edit",
    input: { path: "src/main.ts", find: "old", replace: "new", expected_digest: "sha256:abc" },
    success: false,
    error: { code: "ambiguous_edit", message: `Bearer ${"a".repeat(80)}` },
    result: { unified_diff: "+secret diff", diff_truncated: true, requires_review: true },
    duration_ms: -5,
    editor_state: { workflow_state: "workspace_ready", recommended_next_tools: ["read_file"] },
  });

  const record = recorder.snapshot().records[0];
  assert.ok(record);
  assert.equal(record.kind, "tool_call");
  if (record.kind !== "tool_call") return;
  assert.deepEqual(record.input, { path: "src/main.ts", find: "[omitted]", replace: "[omitted]", expected_digest: "sha256:abc" });
  assert.deepEqual(record.result_metadata, { diff_truncated: true, requires_review: true });
  assert.deepEqual(record.error, { code: "ambiguous_edit", message: "[redacted]" });
  assert.equal(record.duration_ms, 0);
});

test("keeps the newest records within the configured bound and exports JSON", () => {
  const recorder = createWebMcpEvidenceRecorder({ max_records: 2, now: () => 10 });
  recorder.recordDiscovery(["first"]);
  recorder.recordDiscovery(["second"]);
  recorder.recordDiscovery(["third"]);

  const evidence: WebMcpEvidenceExport = recorder.snapshot();
  assert.equal(evidence.dropped_records, 1);
  const discoveries = evidence.records.filter((record) => record.kind === "discovery");
  assert.deepEqual(discoveries.map((record) => record.tool_names), [["second"], ["third"]]);
  const serialized = recorder.toJson();
  assert.deepEqual(JSON.parse(serialized), evidence);
  assert.equal(isJsonSerializable(JSON.parse(serialized)), true);
  const cyclic: { self?: unknown } = {};
  cyclic.self = cyclic;
  assert.equal(isJsonSerializable(cyclic), false);
});

test("rejects invalid record bounds", () => {
  assert.throws(() => createWebMcpEvidenceRecorder({ max_records: 0 }), RangeError);
  assert.throws(() => createWebMcpEvidenceRecorder({ max_records: 1.5 }), RangeError);
});

test("omits cyclic values instead of recursing while sanitizing", () => {
  const recorder = createWebMcpEvidenceRecorder();
  const cyclic: { self?: unknown } = {};
  cyclic.self = cyclic;
  recorder.recordToolCall({
    tool_name: "get_editor_state",
    input: cyclic,
    success: true,
    result: cyclic,
    duration_ms: 1,
    editor_state: cyclic,
  });
  const record = recorder.snapshot().records[0];
  assert.ok(record);
  assert.equal(record.kind, "tool_call");
  if (record.kind !== "tool_call") return;
  assert.deepEqual(record.input, { self: "[omitted]" });
  assert.deepEqual(record.editor_state, {});
});

test("redacts GitHub App installation tokens in both formats", () => {
  // Legacy opaque (~40 chars) and stateless JWT (~520 chars, two dots) per
  // May/Oct 2026 changelogs. Assembled at runtime so scanners do not flag them.
  // `note` is a non-sensitive, non-content key so value redaction is exercised.
  const legacy = `${"ghs_"}${"a".repeat(36)}`;
  const stateless = `${"ghs_"}${"b".repeat(20)}.${"c".repeat(250)}.${"d".repeat(250)}`;
  assert.equal(stateless.length >= 500, true);
  assert.equal(stateless.split(".").length, 3);

  const recorder = createWebMcpEvidenceRecorder();
  recorder.recordToolCall({
    tool_name: "read_file",
    input: { note: `saw ${legacy} and ${stateless} here` },
    success: false,
    error: { code: "x", message: `failed with ${stateless}` },
    duration_ms: 1,
    editor_state: {},
  });
  const record = recorder.snapshot().records[0];
  assert.ok(record);
  assert.equal(record.kind, "tool_call");
  if (record.kind !== "tool_call") return;
  assert.deepEqual(record.input, { note: "saw [redacted] and [redacted] here" });
  assert.deepEqual(record.error, { code: "x", message: "failed with [redacted]" });
});
