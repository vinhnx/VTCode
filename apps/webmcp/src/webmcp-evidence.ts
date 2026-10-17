export type JsonValue = null | boolean | number | string | JsonValue[] | { readonly [key: string]: JsonValue };

export interface WebMcpEvidenceSession {
  readonly client_label: string;
  readonly origin: string;
  readonly user_agent?: string;
  readonly webmcp_context: JsonValue;
}

export interface WebMcpDiscoveryRecord {
  readonly kind: "discovery";
  readonly recorded_at_ms: number;
  readonly tool_names: readonly string[];
  readonly tool_count: number;
  readonly source?: string;
}

export interface WebMcpToolCallRecord {
  readonly kind: "tool_call";
  readonly recorded_at_ms: number;
  readonly tool_name: string;
  readonly input: JsonValue;
  readonly success: boolean;
  readonly error?: {
    readonly code: string;
    readonly message: string;
  };
  readonly result_metadata: JsonValue;
  readonly duration_ms: number;
  readonly editor_state: JsonValue;
}

export type WebMcpEvidenceRecord = WebMcpDiscoveryRecord | WebMcpToolCallRecord;

export interface WebMcpEvidenceExport {
  readonly schema_version: 1;
  readonly started_at_ms: number;
  readonly session: WebMcpEvidenceSession | null;
  readonly max_records: number;
  readonly dropped_records: number;
  readonly records: readonly WebMcpEvidenceRecord[];
}

export interface WebMcpToolCallEvidence {
  readonly tool_name: string;
  readonly input?: unknown;
  readonly success: boolean;
  readonly result?: unknown;
  readonly error?: unknown;
  readonly duration_ms: number;
  readonly editor_state?: unknown;
}

export interface WebMcpEvidenceRecorderOptions {
  readonly max_records?: number;
  readonly now?: () => number;
}

export interface WebMcpEvidenceRecorder {
  readonly begin: (session: WebMcpEvidenceSession) => void;
  readonly recordDiscovery: (tool_names: readonly string[], source?: string) => void;
  readonly recordToolCall: (evidence: WebMcpToolCallEvidence) => void;
  readonly snapshot: () => WebMcpEvidenceExport;
  readonly toJson: () => string;
}

export const DEFAULT_MAX_EVIDENCE_RECORDS = 100;
export const MAX_EVIDENCE_RECORDS = 1_000;

const MAX_STRING_LENGTH = 256;
const MAX_ERROR_MESSAGE_LENGTH = 512;
const MAX_ARRAY_ITEMS = 16;
const MAX_OBJECT_KEYS = 24;
const MAX_NESTING_DEPTH = 4;
const OMITTED_VALUE = "[omitted]";
const REDACTED_VALUE = "[redacted]";

const SENSITIVE_KEY = /(?:secret|token|password|passwd|credential|authorization|cookie|pairing|session|private[_-]?key|api[_-]?key|access[_-]?key)/i;
const CONTENT_KEY = /^(?:content|find|replace|unified[_-]?diff|diff|stdout|stderr|text|body|prompt|raw|source)$/i;
// GitHub App installation tokens (`ghs_...`) use the recommended `ghs_[A-Za-z0-9.\-_]{36,}`
// pattern so both legacy opaque (~40 chars) and stateless JWT (~520 chars, two dots)
// formats are redacted even without a Bearer prefix. See
// https://github.blog/changelog/2026-05-15-github-app-installation-tokens-per-request-override-header/
// and https://github.blog/changelog/2026-10-02-stateless-github-app-installation-tokens-rolled-out
const SECRET_VALUE = /(?:ghs_[A-Za-z0-9.\-_]{36,}|(?:bearer\s+|basic\s+|(?:sk|ghp|github_pat|xox[baprs])-|AIza[0-9A-Za-z_-]{20,}|eyJ[A-Za-z0-9_-]{20,}\.[A-Za-z0-9_-]{10,}\.)[A-Za-z0-9._~+\-/=]+)/gi;

function boundedString(value: string, maxLength = MAX_STRING_LENGTH): string {
  return value.length <= maxLength ? value : `${value.slice(0, maxLength)}…`;
}

function sanitizedString(value: string, maxLength = MAX_STRING_LENGTH): string {
  return boundedString(value.replace(SECRET_VALUE, REDACTED_VALUE), maxLength);
}

function isJsonValue(value: unknown, ancestors = new WeakSet<object>()): value is JsonValue {
  if (value === null || typeof value === "boolean" || typeof value === "string") return true;
  if (typeof value === "number") return Number.isFinite(value);
  if (typeof value !== "object") return false;
  if (ancestors.has(value)) return false;
  ancestors.add(value);
  const valid = Array.isArray(value)
    ? value.every((item) => isJsonValue(item, ancestors))
    : Object.values(value).every((item) => isJsonValue(item, ancestors));
  ancestors.delete(value);
  return valid;
}

function sanitizeValue(value: unknown, depth = 0, key?: string, ancestors = new WeakSet<object>()): JsonValue {
  if (key && SENSITIVE_KEY.test(key)) return REDACTED_VALUE;
  if (key && CONTENT_KEY.test(key)) return OMITTED_VALUE;
  if (value === null || typeof value === "boolean") return value;
  if (typeof value === "string") return sanitizedString(value);
  if (typeof value === "number") return Number.isFinite(value) ? value : null;
  if (depth >= MAX_NESTING_DEPTH) return OMITTED_VALUE;
  if (typeof value !== "object" || ancestors.has(value)) return OMITTED_VALUE;
  ancestors.add(value);
  if (Array.isArray(value)) {
    const result = value.slice(0, MAX_ARRAY_ITEMS).map((item) => sanitizeValue(item, depth + 1, undefined, ancestors));
    ancestors.delete(value);
    return result;
  }

  const result: { [key: string]: JsonValue } = {};
  for (const [childKey, childValue] of Object.entries(value).slice(0, MAX_OBJECT_KEYS)) {
    result[childKey] = sanitizeValue(childValue, depth + 1, childKey, ancestors);
  }
  ancestors.delete(value);
  return result;
}

function sanitizeInput(input: unknown): JsonValue {
  return sanitizeValue(input);
}

const RESULT_METADATA_KEYS = new Set([
  "active_panel",
  "content_truncated",
  "diff_truncated",
  "dirty_files",
  "files",
  "files_count",
  "hint",
  "line",
  "matches",
  "matches_count",
  "omitted_count",
  "opened",
  "open_tabs",
  "path",
  "requires_review",
  "reviewed",
  "scanned_bytes",
  "scanned_files",
  "size_bytes",
  "staged",
  "state",
  "status",
  "truncated",
  "workflow_state",
  "webmcp_context",
]);

function resultMetadata(result: unknown): JsonValue {
  if (!result || typeof result !== "object" || Array.isArray(result)) {
    return { result_type: result === null ? "null" : typeof result };
  }
  const metadata: { [key: string]: JsonValue } = {};
  for (const [key, value] of Object.entries(result)) {
    if (RESULT_METADATA_KEYS.has(key)) metadata[key] = sanitizeValue(value);
  }
  return metadata;
}

const EDITOR_STATE_KEYS = new Set([
  "active_panel",
  "backend",
  "connected",
  "dirty_files",
  "has_client_proposal",
  "has_server_proposal",
  "open_tabs",
  "recommended_next_tools",
  "selected",
  "webmcp_context",
  "workflow_state",
]);

function editorStateSnapshot(state: unknown): JsonValue {
  if (!state || typeof state !== "object" || Array.isArray(state)) return {};
  const snapshot: { [key: string]: JsonValue } = {};
  for (const [key, value] of Object.entries(state)) {
    if (EDITOR_STATE_KEYS.has(key)) snapshot[key] = sanitizeValue(value);
  }
  return snapshot;
}

function errorSnapshot(error: unknown): { readonly code: string; readonly message: string } {
  if (error instanceof Error) {
    const value = error as Error & { readonly code?: unknown };
    return {
      code: typeof value.code === "string" ? sanitizedString(value.code) : "error",
      message: sanitizedString(error.message, MAX_ERROR_MESSAGE_LENGTH),
    };
  }
  if (error && typeof error === "object") {
    const value = error as { readonly code?: unknown; readonly message?: unknown };
    return {
      code: typeof value.code === "string" ? sanitizedString(value.code) : "error",
      message: typeof value.message === "string" ? sanitizedString(value.message, MAX_ERROR_MESSAGE_LENGTH) : "Tool call failed",
    };
  }
  return { code: "error", message: "Tool call failed" };
}

function nonNegativeDuration(duration: number): number {
  return Number.isFinite(duration) && duration >= 0 ? Math.round(duration) : 0;
}

function boundedToolNames(tool_names: readonly string[]): string[] {
  return tool_names.slice(0, MAX_OBJECT_KEYS).map((name) => sanitizedString(name));
}

export function createWebMcpEvidenceRecorder(options: WebMcpEvidenceRecorderOptions = {}): WebMcpEvidenceRecorder {
  const max_records = options.max_records ?? DEFAULT_MAX_EVIDENCE_RECORDS;
  if (!Number.isInteger(max_records) || max_records < 1 || max_records > MAX_EVIDENCE_RECORDS) {
    throw new RangeError(`max_records must be an integer from 1 to ${MAX_EVIDENCE_RECORDS}`);
  }
  const now = options.now ?? Date.now;
  let started_at_ms = now();
  let session: WebMcpEvidenceSession | null = null;
  const records: WebMcpEvidenceRecord[] = [];
  let dropped_records = 0;

  function append(record: WebMcpEvidenceRecord): void {
    if (records.length >= max_records) {
      records.shift();
      dropped_records += 1;
    }
    records.push(record);
  }

  const snapshot = (): WebMcpEvidenceExport => ({
    schema_version: 1,
    started_at_ms,
    session,
    max_records,
    dropped_records,
    records: records.map((record) => ({ ...record })),
  });

  return {
    begin(nextSession) {
      started_at_ms = now();
      session = {
        client_label: sanitizedString(nextSession.client_label),
        origin: sanitizedString(nextSession.origin),
        ...(nextSession.user_agent ? { user_agent: sanitizedString(nextSession.user_agent) } : {}),
        webmcp_context: sanitizeValue(nextSession.webmcp_context),
      };
      records.length = 0;
      dropped_records = 0;
    },
    recordDiscovery(tool_names, source) {
      const names = boundedToolNames(tool_names);
      append({
        kind: "discovery",
        recorded_at_ms: now(),
        tool_names: names,
        tool_count: tool_names.length,
        ...(source ? { source: sanitizedString(source) } : {}),
      });
    },
    recordToolCall(evidence) {
      append({
        kind: "tool_call",
        recorded_at_ms: now(),
        tool_name: sanitizedString(evidence.tool_name),
        input: sanitizeInput(evidence.input ?? {}),
        success: evidence.success,
        ...(evidence.success ? {} : { error: errorSnapshot(evidence.error) }),
        result_metadata: resultMetadata(evidence.result),
        duration_ms: nonNegativeDuration(evidence.duration_ms),
        editor_state: editorStateSnapshot(evidence.editor_state),
      });
    },
    snapshot,
    toJson: () => JSON.stringify(snapshot()),
  };
}

export function isJsonSerializable(value: unknown): value is JsonValue {
  return isJsonValue(value);
}
