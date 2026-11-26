import { BackendError, isRecord, type Panel } from "./types.ts";

export interface ToolExecutionOptions {
  readonly signal?: AbortSignal;
}

export interface JsonSchemaProperty {
  readonly type: "string";
  readonly description?: string;
  readonly minLength?: number;
  readonly maxLength?: number;
  readonly enum?: readonly string[];
}

export interface ObjectInputSchema {
  readonly [key: string]: unknown;
  readonly type: "object";
  readonly properties: Readonly<Record<string, JsonSchemaProperty>>;
  readonly required?: readonly string[];
  readonly additionalProperties?: boolean;
}

export interface WebMcpToolAnnotations {
  readonly readOnlyHint?: boolean;
  readonly untrustedContentHint?: boolean;
}

export type ToolExecute = (input?: unknown, options?: ToolExecutionOptions) => Promise<unknown>;

export interface WebMcpTool {
  readonly name: string;
  readonly title: string;
  readonly description: string;
  readonly inputSchema: ObjectInputSchema;
  readonly annotations: WebMcpToolAnnotations;
  readonly execute: ToolExecute;
}

export interface ModelContext {
  readonly getTools?: () => Promise<readonly WebMcpTool[]>;
  readonly registerTool?: (
    tool: WebMcpTool,
    options?: WebMcpToolRegistrationOptions,
  ) => Promise<void> | void;
  readonly addEventListener?: (type: string, listener: EventListenerOrEventListenerObject) => void;
  readonly removeEventListener?: (type: string, listener: EventListenerOrEventListenerObject) => void;
}

export interface StageTextEditInput {
  readonly path: string;
  readonly find: string;
  readonly replace: string;
  readonly expected_digest: string;
}

export interface WebMcpCallbacks {
  readonly listFiles: (options?: ToolExecutionOptions) => Promise<readonly string[]>;
  readonly readFile: (path: string, options?: ToolExecutionOptions) => Promise<unknown>;
  readonly searchCode: (query: string, options?: ToolExecutionOptions) => Promise<unknown>;
  readonly getEditorState: () => unknown;
  readonly openFile: (path: string) => Promise<unknown>;
  readonly stageTextEdit: (input: StageTextEditInput, options?: ToolExecutionOptions) => Promise<unknown>;
  readonly reviewDraft: () => Promise<unknown>;
  readonly openPanel: (panel: Panel) => void;
}

export interface WebMcpRegistration {
  readonly names: string[];
  readonly dispose: () => void;
}

const EMPTY_INPUT_SCHEMA: ObjectInputSchema = Object.freeze({
  type: "object",
  properties: {},
  additionalProperties: false,
});
const MAX_TOOL_OUTPUT_CHARS = 1500;
const OUTPUT_TRUNCATION_MARKER = "\n[output truncated by WebMCP limit]";

function serializedLength(value: unknown): number {
  try {
    const serialized = JSON.stringify(value);
    return typeof serialized === "string" ? serialized.length : Number.POSITIVE_INFINITY;
  } catch {
    return Number.POSITIVE_INFINITY;
  }
}

function toolInputError(message: string): BackendError {
  return new BackendError(message, "invalid_input");
}

function truncateString(value: string, maxChars: number): string {
  if (value.length <= maxChars) return value;
  const markerLength = OUTPUT_TRUNCATION_MARKER.length;
  return markerLength >= maxChars
    ? value.slice(0, maxChars)
    : `${value.slice(0, maxChars - markerLength)}${OUTPUT_TRUNCATION_MARKER}`;
}

function boundStringResult(value: string): string {
  if (serializedLength(value) <= MAX_TOOL_OUTPUT_CHARS) return value;
  let low = 0;
  let high = value.length;
  let best = "";
  while (low <= high) {
    const middle = Math.floor((low + high) / 2);
    const candidate = truncateString(value, middle);
    if (serializedLength(candidate) <= MAX_TOOL_OUTPUT_CHARS) {
      best = candidate;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return best;
}

function boundTextField(result: Record<string, unknown>, field: string): Record<string, unknown> {
  const truncatedField = field === "content"
    ? "content_truncated"
    : field === "unified_diff" ? "diff_truncated" : `${field}_truncated`;
  const base = { ...result, [field]: "", [truncatedField]: true };
  if (serializedLength(base) > MAX_TOOL_OUTPUT_CHARS) return { truncated: true };

  const value = result[field];
  if (typeof value !== "string") return { truncated: true };
  let low = 0;
  let high = value.length;
  let best = base;
  while (low <= high) {
    const middle = Math.floor((low + high) / 2);
    const candidate = { ...base, [field]: truncateString(value, middle) };
    if (serializedLength(candidate) <= MAX_TOOL_OUTPUT_CHARS) {
      best = candidate;
      low = middle + 1;
    } else {
      high = middle - 1;
    }
  }
  return best;
}

function boundCollectionField(result: Record<string, unknown>, field: string): Record<string, unknown> {
  const collection = result[field];
  if (!Array.isArray(collection)) return { truncated: true };
  const base = { ...result, [field]: [], truncated: true, omitted_count: collection.length };
  if (serializedLength(base) > MAX_TOOL_OUTPUT_CHARS) return { truncated: true };

  const included: unknown[] = [];
  for (const item of collection) {
    const candidate = {
      ...base,
      [field]: [...included, item],
      omitted_count: collection.length - included.length - 1,
    };
    if (serializedLength(candidate) > MAX_TOOL_OUTPUT_CHARS) break;
    included.push(item);
  }
  return {
    ...base,
    [field]: included,
    omitted_count: collection.length - included.length,
  };
}

export function boundToolOutput(result: unknown): unknown {
  if (serializedLength(result) <= MAX_TOOL_OUTPUT_CHARS) return result;
  if (typeof result === "string") return boundStringResult(result);
  if (Array.isArray(result)) return boundCollectionField({ items: result }, "items");
  if (isRecord(result)) {
    for (const field of ["content", "unified_diff", "stdout", "stderr", "text", "reason"]) {
      if (typeof result[field] === "string") return boundTextField(result, field);
    }
    for (const field of ["files", "matches", "paths", "open_tabs", "dirty_files", "items"]) {
      if (Array.isArray(result[field])) return boundCollectionField(result, field);
    }
  }
  return { truncated: true };
}

function validateToolInput(toolName: string, schema: ObjectInputSchema, input: unknown): Record<string, unknown> {
  const value = input === undefined ? {} : input;
  if (!isRecord(value)) throw toolInputError(`${toolName} expects an object input matching its input schema`);
  for (const required of schema.required ?? []) {
    if (!(required in value)) throw toolInputError(`${toolName} requires ${required}; provide it before retrying`);
  }
  if (schema.additionalProperties === false) {
    const properties = new Set(Object.keys(schema.properties));
    for (const key of Object.keys(value)) {
      if (!properties.has(key)) throw toolInputError(`${toolName} does not accept ${key}; use only fields listed in the schema`);
    }
  }
  for (const [name, property] of Object.entries(schema.properties)) {
    if (!(name in value)) continue;
    const current = value[name];
    if (property.type === "string" && typeof current !== "string") {
      throw toolInputError(`${toolName}.${name} must be a string; provide a string value`);
    }
    if (typeof current === "string" && property.minLength !== undefined && current.length < property.minLength) {
      throw toolInputError(`${toolName}.${name} must not be empty; provide a non-empty value`);
    }
    if (typeof current === "string" && property.maxLength !== undefined && current.length > property.maxLength) {
      throw toolInputError(`${toolName}.${name} exceeds the length limit; shorten the value and retry`);
    }
    if (property.enum && (typeof current !== "string" || !property.enum.includes(current))) {
      throw toolInputError(`${toolName}.${name} has an unsupported value; choose one of: ${property.enum.join(", ")}`);
    }
  }
  return value;
}

function applyToolContract(tool: WebMcpTool): WebMcpTool {
  const execute = tool.execute;
  return {
    ...tool,
    execute: async (input: unknown = {}, options: ToolExecutionOptions = {}) => {
      const validatedInput = validateToolInput(tool.name, tool.inputSchema, input);
      return boundToolOutput(await execute(validatedInput, options));
    },
  };
}

export function replaceExactText(content: string, find: string, replace: string): string {
  const first = content.indexOf(find);
  if (first < 0) throw new BackendError("Text was not found; read the current file and provide an exact non-empty match", "text_not_found");
  if (content.indexOf(find, first + 1) >= 0) {
    throw new BackendError("Text occurs more than once; narrow find to one exact match before retrying", "ambiguous_edit");
  }
  return `${content.slice(0, first)}${replace}${content.slice(first + find.length)}`;
}

function abortError(signal: AbortSignal): Error {
  if (signal.reason instanceof Error) return signal.reason;
  if (typeof DOMException === "function") return new DOMException("The WebMCP tool call was aborted", "AbortError");
  return new Error("The WebMCP tool call was aborted");
}

function throwIfAborted(signal: AbortSignal | undefined): void {
  if (signal?.aborted) throw abortError(signal);
}

function withAbortSignal(execute: ToolExecute): ToolExecute {
  return async (input: unknown = {}, options: ToolExecutionOptions = {}) => {
    const signal = options.signal;
    throwIfAborted(signal);
    const result = await execute(input, { signal });
    throwIfAborted(signal);
    return result;
  };
}

function stringInput(input: Record<string, unknown>, toolName: string, field: string): string {
  const value = input[field];
  if (typeof value !== "string") throw toolInputError(`${toolName}.${field} must be a string; provide a string value`);
  return value;
}

export function createWebMcpTools({
  listFiles,
  readFile,
  searchCode,
  getEditorState,
  openFile,
  stageTextEdit,
  reviewDraft,
  openPanel,
}: WebMcpCallbacks): WebMcpTool[] {
  const tools: WebMcpTool[] = [
    {
      name: "list_project_files",
      title: "List project files",
      description: "Start here to list bounded files visible to the current VT Code workspace without changing it.",
      inputSchema: EMPTY_INPUT_SCHEMA,
      annotations: { readOnlyHint: true, untrustedContentHint: true },
      execute: async (_input: unknown, { signal }: ToolExecutionOptions = {}) => ({ files: await listFiles({ signal }) }),
    },
    {
      name: "read_file",
      title: "Read a project file",
      description: "Read a bounded file preview and fresh digest after a path is known; use it before staging an edit.",
      inputSchema: {
        type: "object",
        properties: {
          path: { type: "string", description: "Workspace-relative file path.", minLength: 1, maxLength: 4096 },
        },
        required: ["path"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: true, untrustedContentHint: true },
      execute: async (input: unknown = {}, { signal }: ToolExecutionOptions = {}) => {
        const value = validateToolInput("read_file", {
          type: "object",
          properties: { path: { type: "string", minLength: 1, maxLength: 4096 } },
          required: ["path"],
          additionalProperties: false,
        }, input);
        return readFile(stringInput(value, "read_file", "path"), { signal });
      },
    },
    {
      name: "search_code",
      title: "Search project code",
      description: "Search visible workspace text for a term and return bounded path, line, and text matches without changing it.",
      inputSchema: {
        type: "object",
        properties: {
          query: { type: "string", description: "Case-insensitive text to find.", minLength: 1, maxLength: 120 },
        },
        required: ["query"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: true, untrustedContentHint: true },
      execute: async (input: unknown = {}, { signal }: ToolExecutionOptions = {}) => {
        const value = validateToolInput("search_code", {
          type: "object",
          properties: { query: { type: "string", minLength: 1, maxLength: 120 } },
          required: ["query"],
          additionalProperties: false,
        }, input);
        const result = await searchCode(stringInput(value, "search_code", "query"), { signal });
        return Array.isArray(result) ? { matches: result, truncated: false } : result;
      },
    },
    {
      name: "get_editor_state",
      title: "Inspect editor state",
      description: "Return selected file, open tabs, draft paths, and backend state without returning file contents.",
      inputSchema: EMPTY_INPUT_SCHEMA,
      annotations: { readOnlyHint: true, untrustedContentHint: true },
      execute: async (_input: unknown, { signal }: ToolExecutionOptions = {}) => {
        throwIfAborted(signal);
        return getEditorState();
      },
    },
    {
      name: "open_file",
      title: "Open a file in the editor",
      description: "Open a path returned by search or file listing; this changes only the page view and never writes a file.",
      inputSchema: {
        type: "object",
        properties: {
          path: { type: "string", description: "Workspace-relative file path.", minLength: 1, maxLength: 4096 },
        },
        required: ["path"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: false, untrustedContentHint: false },
      execute: async (input: unknown = {}, { signal }: ToolExecutionOptions = {}) => {
        throwIfAborted(signal);
        const value = validateToolInput("open_file", {
          type: "object",
          properties: { path: { type: "string", minLength: 1, maxLength: 4096 } },
          required: ["path"],
          additionalProperties: false,
        }, input);
        const path = stringInput(value, "open_file", "path");
        await openFile(path);
        return { opened: path };
      },
    },
    {
      name: "stage_text_edit",
      title: "Stage a text edit",
      description: "After read_file, stage one exact replacement in a clean browser draft using its fresh digest; review is required and disk is never written.",
      inputSchema: {
        type: "object",
        properties: {
          path: { type: "string", description: "Workspace-relative file path.", minLength: 1, maxLength: 4096 },
          find: { type: "string", description: "Exact non-empty text to replace once.", minLength: 1, maxLength: 65536 },
          replace: { type: "string", description: "Replacement text; empty text deletes the match.", maxLength: 65536 },
          expected_digest: { type: "string", description: "Fresh digest returned by read_file for this path.", minLength: 1, maxLength: 200 },
        },
        required: ["path", "find", "replace", "expected_digest"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: false, untrustedContentHint: true },
      execute: async (input: unknown = {}, { signal }: ToolExecutionOptions = {}) => {
        throwIfAborted(signal);
        const value = validateToolInput("stage_text_edit", {
          type: "object",
          properties: {
            path: { type: "string", minLength: 1, maxLength: 4096 },
            find: { type: "string", minLength: 1, maxLength: 65536 },
            replace: { type: "string", maxLength: 65536 },
            expected_digest: { type: "string", minLength: 1, maxLength: 200 },
          },
          required: ["path", "find", "replace", "expected_digest"],
          additionalProperties: false,
        }, input);
        const edit: StageTextEditInput = {
          path: stringInput(value, "stage_text_edit", "path"),
          find: stringInput(value, "stage_text_edit", "find"),
          replace: stringInput(value, "stage_text_edit", "replace"),
          expected_digest: stringInput(value, "stage_text_edit", "expected_digest"),
        };
        return stageTextEdit(edit, { signal });
      },
    },
    {
      name: "review_draft",
      title: "Review the current draft",
      description: "After a draft edit, create its browser unified diff; this never approves or applies a filesystem change.",
      inputSchema: EMPTY_INPUT_SCHEMA,
      annotations: { readOnlyHint: false, untrustedContentHint: true },
      execute: async (_input: unknown, { signal }: ToolExecutionOptions = {}) => {
        throwIfAborted(signal);
        return reviewDraft();
      },
    },
    {
      name: "open_panel",
      title: "Open an editor panel",
      description: "Show the activity, changes, or VT Code panel in the browser editor.",
      inputSchema: {
        type: "object",
        properties: {
          panel: { type: "string", enum: ["activity", "changes", "turn"], description: "Panel to show." },
        },
        required: ["panel"],
        additionalProperties: false,
      },
      annotations: { readOnlyHint: false, untrustedContentHint: false },
      execute: async (input: unknown = {}, { signal }: ToolExecutionOptions = {}) => {
        throwIfAborted(signal);
        const value = validateToolInput("open_panel", {
          type: "object",
          properties: { panel: { type: "string", enum: ["activity", "changes", "turn"] } },
          required: ["panel"],
          additionalProperties: false,
        }, input);
        const panel = stringInput(value, "open_panel", "panel");
        openPanel(panel as Panel);
        return { opened: panel };
      },
    },
  ];
  return tools.map(applyToolContract);
}

export interface WebMcpRegistrationOptions {
  readonly onToolChange?: (names: readonly string[]) => void;
  readonly onToolsDiscovered?: (names: readonly string[]) => void;
  /**
   * Registration-time signal, mirroring WebMCP's per-registration tool identity.
   * Aborting it unregisters the tools and invalidates any async work started by
   * this registration, so a superseded attempt cannot publish stale results.
   */
  readonly signal?: AbortSignal;
}

export async function registerWebMcpTools(
  modelContext: ModelContext | undefined,
  tools: readonly WebMcpTool[],
  {
    onToolChange = () => {},
    onToolsDiscovered = () => {},
    signal,
  }: WebMcpRegistrationOptions = {},
): Promise<WebMcpRegistration | null> {
  if (!modelContext?.registerTool || signal?.aborted) return null;
  const controller = new AbortController();
  const onExternalAbort = (): void => controller.abort(signal?.reason);
  signal?.addEventListener("abort", onExternalAbort, { once: true });
  const abortRegistration = (reason?: unknown): void => {
    if (!controller.signal.aborted) controller.abort(reason);
  };
  const names: string[] = [];
  let disposed = false;
  const stale = (): boolean => disposed || signal?.aborted === true;
  try {
    for (const tool of tools) {
      // Re-check identity at the point of use: an overlapping registration may
      // have aborted our signal while we were awaiting the previous tool.
      if (stale()) {
        signal?.removeEventListener("abort", onExternalAbort);
        abortRegistration(signal?.reason);
        return null;
      }
      await modelContext.registerTool({ ...tool, execute: withAbortSignal(tool.execute) }, { signal: controller.signal });
      names.push(tool.name);
    }
  } catch (error: unknown) {
    signal?.removeEventListener("abort", onExternalAbort);
    abortRegistration(error);
    throw error;
  }

  const notifyDiscoveredTools = async (): Promise<void> => {
    if (stale()) return;
    let discovered: readonly string[] = names;
    if (modelContext.getTools) {
      try {
        discovered = (await modelContext.getTools()).map((tool) => tool.name);
      } catch {
        // Locally registered names are the best available discovery result.
      }
    }
    // The discovery read races with disposal; ignore a result that settles after it.
    if (stale()) return;
    onToolsDiscovered(discovered);
  };
  await notifyDiscoveredTools();
  if (stale()) {
    signal?.removeEventListener("abort", onExternalAbort);
    abortRegistration(signal?.reason);
    return null;
  }
  const handleToolChange = (): void => {
    if (stale()) return;
    onToolChange(names);
    void notifyDiscoveredTools();
  };
  modelContext.addEventListener?.("toolchange", handleToolChange);
  return {
    names,
    dispose(): void {
      if (disposed) return;
      disposed = true;
      signal?.removeEventListener("abort", onExternalAbort);
      modelContext.removeEventListener?.("toolchange", handleToolChange);
      abortRegistration(signal?.reason);
    },
  };
}

export { MAX_TOOL_OUTPUT_CHARS };
