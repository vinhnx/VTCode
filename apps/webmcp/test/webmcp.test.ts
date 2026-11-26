import test from "node:test";
import assert from "node:assert/strict";
import { MAX_TOOL_OUTPUT_CHARS, createWebMcpTools, registerWebMcpTools, replaceExactText, type ModelContext, type ToolExecutionOptions, type WebMcpCallbacks, type WebMcpTool } from "../src/webmcp.ts";
import { isRecord, type Panel } from "../src/types.ts";

interface RegisteredTool {
  readonly tool: WebMcpTool;
  readonly options: WebMcpToolRegistrationOptions;
}

class FakeModelContext extends EventTarget implements ModelContext {
  readonly tools: RegisteredTool[] = [];
  registerToolImpl?: (tool: WebMcpTool, options: WebMcpToolRegistrationOptions) => Promise<void>;
  getToolsImpl?: () => Promise<WebMcpTool[]>;

  async registerTool(tool: WebMcpTool, options: WebMcpToolRegistrationOptions = {}): Promise<void> {
    if (this.registerToolImpl) {
      await this.registerToolImpl(tool, options);
      return;
    }
    this.tools.push({ tool, options });
  }

  async getTools(): Promise<WebMcpTool[]> {
    if (this.getToolsImpl) return this.getToolsImpl();
    return this.tools.map(({ tool }) => tool).sort((left, right) => left.name.localeCompare(right.name));
  }

  async executeTool(tool: WebMcpTool | undefined, serializedInput: string, options: ToolExecutionOptions = {}): Promise<unknown> {
    if (!tool) throw new Error("fake ModelContext could not find the requested tool");
    return tool.execute(JSON.parse(serializedInput) as unknown, options);
  }

  dispatch(type: string): void {
    this.dispatchEvent(new Event(type));
  }
}

function toolsForTest(overrides: Partial<WebMcpCallbacks> = {}): WebMcpTool[] {
  const defaults: WebMcpCallbacks = {
    listFiles: async () => ["README.md"],
    readFile: async (path: string) => ({ path, content: "draft\n" }),
    searchCode: async (query: string) => [{ path: "README.md", line: 1, text: query }],
    getEditorState: () => ({ selected: "README.md" }),
    openFile: async (path: string) => path,
    stageTextEdit: async ({ path }: { readonly path: string }) => ({ staged: true, path, requires_review: true }),
    reviewDraft: async () => ({ reviewed: true }),
    openPanel: (panel: Panel) => panel,
  };
  return createWebMcpTools({ ...defaults, ...overrides });
}

function registeredAt(context: FakeModelContext, index: number): RegisteredTool {
  const registered = context.tools[index];
  assert.ok(registered);
  return registered;
}

function toolNamed(context: FakeModelContext, name: string): WebMcpTool {
  const registered = context.tools.find(({ tool }) => tool.name === name);
  assert.ok(registered);
  return registered.tool;
}

function recordOutput(value: unknown): Record<string, unknown> {
  if (!isRecord(value)) throw new Error("expected a WebMCP object result");
  return value;
}

test("registers spec-shaped WebMCP tools with titles, schemas, annotations, and abort signals", async () => {
  const context = new FakeModelContext();
  const registration = await registerWebMcpTools(context, toolsForTest());
  assert.ok(registration);

  assert.equal(registration.names.length, 8);
  assert.deepEqual(registration.names, [
    "list_project_files",
    "read_file",
    "search_code",
    "get_editor_state",
    "open_file",
    "stage_text_edit",
    "review_draft",
    "open_panel",
  ]);
  assert.equal(registeredAt(context, 0).tool.title, "List project files");
  assert.equal(registeredAt(context, 0).tool.annotations.readOnlyHint, true);
  assert.equal(registeredAt(context, 0).tool.annotations.untrustedContentHint, true);
  assert.equal(registeredAt(context, 4).tool.annotations.readOnlyHint, false);
  assert.equal(registeredAt(context, 3).tool.annotations.untrustedContentHint, true);
  assert.equal(registeredAt(context, 6).tool.annotations.untrustedContentHint, true);
  assert.equal(registeredAt(context, 1).tool.inputSchema.required?.[0], "path");
  assert.equal(registeredAt(context, 7).tool.inputSchema.properties.panel?.enum?.length, 3);

  const state = await registeredAt(context, 3).tool.execute({}, { signal: new AbortController().signal });
  assert.deepEqual(state, { selected: "README.md" });
  const registrationSignal = registeredAt(context, 0).options.signal;
  assert.ok(registrationSignal);
  assert.equal(registrationSignal.aborted, false);

  registration.dispose();
  assert.equal(registrationSignal.aborted, true);
});

test("aborted WebMCP calls reject before application callbacks run", async () => {
  let called = false;
  const context = new FakeModelContext();
  await registerWebMcpTools(context, toolsForTest({
    listFiles: async () => {
      called = true;
      return [];
    },
  }));
  const controller = new AbortController();
  controller.abort();

  await assert.rejects(
    toolNamed(context, "list_project_files").execute({}, { signal: controller.signal }),
    { name: "AbortError" },
  );
  assert.equal(called, false);
});

test("tools are discoverable and executable through the browser ModelContext shape", async () => {
  const calls: unknown[][] = [];
  const context = new FakeModelContext();
  const registration = await registerWebMcpTools(context, toolsForTest({
    openFile: async (path: string) => { calls.push(["open_file", path]); },
    stageTextEdit: async ({ path }: { readonly path: string }) => { calls.push(["stage_text_edit", path]); return { staged: true, path, requires_review: true }; },
    reviewDraft: async () => { calls.push(["review_draft"]); return { reviewed: true }; },
    openPanel: (panel: Panel) => { calls.push(["open_panel", panel]); },
  }));
  assert.ok(registration);
  const tools = await context.getTools();
  const byName = new Map(tools.map((tool) => [tool.name, tool]));

  assert.deepEqual(await context.executeTool(byName.get("list_project_files"), "{}"), { files: ["README.md"] });
  assert.deepEqual(
    await context.executeTool(byName.get("read_file"), JSON.stringify({ path: "README.md" })),
    { path: "README.md", content: "draft\n" },
  );
  assert.deepEqual(await context.executeTool(byName.get("open_file"), JSON.stringify({ path: "README.md" })), { opened: "README.md" });
  assert.deepEqual(
    await context.executeTool(byName.get("stage_text_edit"), JSON.stringify({
      path: "README.md",
      find: "draft",
      replace: "updated",
      expected_digest: "sha256:test",
    })),
    { staged: true, path: "README.md", requires_review: true },
  );
  assert.deepEqual(await context.executeTool(byName.get("review_draft"), "{}"), { reviewed: true });
  assert.deepEqual(await context.executeTool(byName.get("open_panel"), JSON.stringify({ panel: "changes" })), { opened: "changes" });
  assert.deepEqual(calls, [["open_file", "README.md"], ["stage_text_edit", "README.md"], ["review_draft"], ["open_panel", "changes"]]);

  registration.dispose();
});

test("tool inputs fail clearly before application callbacks run", async () => {
  let called = false;
  const context = new FakeModelContext();
  await registerWebMcpTools(context, toolsForTest({
    readFile: async () => { called = true; return {}; },
  }));

  await assert.rejects(toolNamed(context, "read_file").execute({}, {}), /read_file requires path/);
  await assert.rejects(toolNamed(context, "search_code").execute({ query: "" }, {}), /search_code.query must not be empty/);
  await assert.rejects(
    toolNamed(context, "open_panel").execute({ panel: "unknown" }, {}),
    /open_panel\.panel has an unsupported value; choose one of: activity, changes, turn/,
  );
  await assert.rejects(toolNamed(context, "read_file").execute({ path: "README.md", extra: true }, {}), /read_file does not accept extra/);
  assert.equal(called, false);
});

test("browser draft edits require exactly one matching text span", () => {
  assert.equal(replaceExactText("Hello WebMCP", "Hello", "Hi"), "Hi WebMCP");
  assert.equal(replaceExactText("remove me", "remove me", ""), "");
  assert.throws(() => replaceExactText("Hello", "missing", "new"), {
    code: "text_not_found",
    message: /read the current file/,
  });
  assert.throws(() => replaceExactText("same same", "same", "new"), {
    code: "ambiguous_edit",
    message: /narrow find/,
  });
});

test("tool results stay within the WebMCP character budget and report truncation", async () => {
  const context = new FakeModelContext();
  await registerWebMcpTools(context, toolsForTest({
    listFiles: async () => Array.from({ length: 200 }, (_, index) => `src/${"x".repeat(24)}-${index}.js`),
    readFile: async (path: string) => ({ path, content: "content ".repeat(3000), digest: "sha256:test", size_bytes: 24000 }),
    searchCode: async () => Array.from({ length: 200 }, (_, index) => ({ path: `src/${index}.js`, line: index + 1, text: "match ".repeat(30) })),
    reviewDraft: async () => ({ reviewed: true, unified_diff: "+change\n".repeat(3000), diff_truncated: false }),
  }));
  const outputs = await Promise.all([
    toolNamed(context, "list_project_files").execute({}, {}),
    toolNamed(context, "read_file").execute({ path: "README.md" }, {}),
    toolNamed(context, "search_code").execute({ query: "match" }, {}),
    toolNamed(context, "review_draft").execute({}, {}),
  ]);

  for (const output of outputs) assert.ok(JSON.stringify(output).length <= MAX_TOOL_OUTPUT_CHARS);
  const listOutput = recordOutput(outputs[0]);
  const readOutput = recordOutput(outputs[1]);
  const searchOutput = recordOutput(outputs[2]);
  const reviewOutput = recordOutput(outputs[3]);
  assert.equal(listOutput.truncated, true);
  assert.equal(readOutput.content_truncated, true);
  assert.equal(searchOutput.truncated, true);
  assert.equal(reviewOutput.diff_truncated, true);
  assert.ok(typeof listOutput.omitted_count === "number" && listOutput.omitted_count > 0);
  assert.ok(typeof searchOutput.omitted_count === "number" && searchOutput.omitted_count > 0);
});

test("toolchange notifications reach the registration owner", async () => {
  const context = new FakeModelContext();
  let changed: readonly string[] | null = null;
  let discovered: readonly string[] | null = null;
  const registration = await registerWebMcpTools(context, toolsForTest(), {
    onToolChange: (names) => { changed = names; },
    onToolsDiscovered: (names) => { discovered = names; },
  });
  assert.ok(registration);

  assert.deepEqual(discovered, registration.names.slice().sort());
  context.dispatch("toolchange");
  assert.deepEqual(changed, registration.names);
  assert.deepEqual(discovered, registration.names.slice().sort());
  registration.dispose();
  changed = null;
  context.dispatch("toolchange");
  assert.equal(changed, null);
});

async function flushMicrotasks(): Promise<void> {
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
}

test("a disposed registration ignores toolchange discovery that settles later", async () => {
  const context = new FakeModelContext();
  const deferred: { resolve?: (tools: WebMcpTool[]) => void } = {};
  let discoveryCalls = 0;
  context.getToolsImpl = () => {
    discoveryCalls += 1;
    // The registration's own discovery must settle; only the toolchange read hangs.
    return discoveryCalls === 1
      ? Promise.resolve(toolsForTest())
      : new Promise<WebMcpTool[]>((resolve) => { deferred.resolve = resolve; });
  };
  let discovered: readonly string[] | null = null;
  let changes = 0;
  const registration = await registerWebMcpTools(context, toolsForTest(), {
    onToolChange: () => { changes += 1; },
    onToolsDiscovered: (names) => { discovered = names; },
  });
  assert.ok(registration);
  assert.deepEqual(discovered, registration.names);

  context.dispatch("toolchange");
  assert.equal(changes, 1);

  registration.dispose();
  registration.dispose();
  deferred.resolve?.([]);
  await flushMicrotasks();

  assert.deepEqual(discovered, registration.names, "late discovery must not replace the settled names");
  assert.equal(changes, 1);
  context.dispatch("toolchange");
  assert.equal(changes, 1, "dispose removes the toolchange listener");
});

test("an aborted registration signal supersedes an in-flight registration", async () => {
  const context = new FakeModelContext();
  const release: { now?: () => void } = {};
  let registrationSignal: AbortSignal | undefined;
  context.registerToolImpl = async (_tool, options) => {
    registrationSignal = options.signal;
    await new Promise<void>((resolve) => { release.now = resolve; });
  };
  const external = new AbortController();
  let discovered: readonly string[] | null = null;
  const pending = registerWebMcpTools(context, toolsForTest(), {
    signal: external.signal,
    onToolsDiscovered: (names) => { discovered = names; },
  });

  await flushMicrotasks();
  assert.ok(registrationSignal);
  assert.equal(registrationSignal.aborted, false);

  external.abort();
  assert.equal(registrationSignal.aborted, true, "supersession unregisters tools synchronously");

  release.now?.();
  assert.equal(await pending, null, "a superseded registration is never published");
  assert.equal(discovered, null);
});
