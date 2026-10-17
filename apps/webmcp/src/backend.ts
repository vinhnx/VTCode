import {
  MAX_FRAME_BYTES as MAX_BRIDGE_FRAME_BYTES,
  parseBridgeFrame,
  PROTOCOL_VERSION,
  validateResponsePayload,
  type BridgeOperation,
  type BridgeRequest,
  type OperationPayloads,
  type PairPayload,
  type RequestPayloads,
} from "./protocol.ts";
import {
  BackendError,
  errorCode,
  isRecord,
  type AppliedChange,
  type BackendConnectionEvent,
  type BackendEvent,
  type BackendKind,
  type CheckResult,
  type FileChange,
  type FileChangeInput,
  type FileSnapshot,
  type PatchProposal,
  type StatusPayload,
  type TurnResult,
  type WorkspaceFileEntry,
  type WriteFileInput,
} from "./types.ts";

export const MAX_FILE_BYTES = 2 * 1024 * 1024;
import type { ExplanationScope, ExplanationPage, EvidenceRef, EvidencePage } from "./explanation.ts";
const MAX_CHANGES = 32;
const REQUEST_TIMEOUT_MS = 30_000;
const MIN_HEARTBEAT_INTERVAL_MS = 250;
const MAX_HEARTBEAT_INTERVAL_MS = 30_000;
const DEFAULT_SESSION_TTL_SECS = 300;
export const MAX_TURN_PROMPT_BYTES = 16 * 1024;
const TURN_DIFF_PREFIX = "\n\nReview this browser draft unified diff:\n\n```diff\n";
const TURN_DIFF_SUFFIX = "\n```";
const TURN_DIFF_TRUNCATION = "\n[diff truncated by the browser prompt limit]\n";
const DIFF_CONTEXT_LINES = 3;
const MAX_DIFF_TRACE_CELLS = 1_000_000;

const SEED_FILES: Readonly<Record<string, string>> = Object.freeze({
  "README.md": "# hello-world\n\nA tiny project for the VT Code WebMCP app.\n\nThe workflow is inspect → edit → review → approve → verify.",
  "src/greeting.js": "import { name } from './config.js';\n\nexport function greeting() {\n  return `Hello, ${name}!`;\n}\n",
  "src/config.js": "export const name = 'WebMCP';\n",
});

interface DiffLine {
  readonly text: string;
  readonly ending: string;
}

type DiffOperationType = "equal" | "delete" | "insert";

interface DiffOperation {
  readonly type: DiffOperationType;
  readonly line: DiffLine;
}

interface DiffHunk {
  start: number;
  end: number;
}

interface StoredProposal {
  readonly proposal: PatchProposal;
  readonly beforeByPath: Record<string, string>;
}

interface LastChange {
  readonly applied: AppliedChange;
  readonly before: FileSnapshot[];
  readonly after: FileSnapshot[];
}

type BackendListener = (event: BackendEvent) => void;
type ConnectionListener = (event: BackendConnectionEvent) => void;
type StatusListener = (payload: StatusPayload | null) => void;

interface PendingRequest {
  readonly operation: BridgeOperation;
  readonly resolve: (payload: unknown) => void;
  readonly reject: (error: unknown) => void;
  readonly timeout: ReturnType<typeof setTimeout>;
}

function cloneFiles(files: Readonly<Record<string, string>>): Record<string, string> {
  const clone: Record<string, string> = Object.create(null) as Record<string, string>;
  for (const [path, content] of Object.entries(files)) clone[path] = content;
  return clone;
}

function backendError(message: string, code = "backend_error"): BackendError {
  return new BackendError(message, code);
}

function validatePath(path: unknown): asserts path is string {
  if (typeof path !== "string" || path.length === 0 || path.length > 4096 || path.includes("\0")) {
    throw backendError("Workspace path is invalid", "path_rejected");
  }
  if (path.startsWith("/") || path.split("/").some((part) => part === "..")) {
    throw backendError("Workspace paths must remain relative to the workspace root", "path_rejected");
  }
}

function normalizeChange(change: unknown): FileChangeInput {
  if (!isRecord(change)) return {};
  return {
    path: typeof change.path === "string" ? change.path : undefined,
    base_digest: typeof change.base_digest === "string" ? change.base_digest : undefined,
    baseDigest: typeof change.baseDigest === "string" ? change.baseDigest : undefined,
    content: typeof change.content === "string" ? change.content : undefined,
  };
}

export async function digest(text: string): Promise<string> {
  const bytes = new TextEncoder().encode(text);
  if (globalThis.crypto?.subtle) {
    const hash = await globalThis.crypto.subtle.digest("SHA-256", bytes);
    return `sha256:${[...new Uint8Array(hash)].map((byte) => byte.toString(16).padStart(2, "0")).join("")}`;
  }
  let hash = 2166136261;
  for (const byte of bytes) hash = Math.imul(hash ^ byte, 16777619);
  return `sha256:fallback-${(hash >>> 0).toString(16)}`;
}

function splitDiffLines(text: string): DiffLine[] {
  const lines: DiffLine[] = [];
  let lineStart = 0;
  for (let index = 0; index < text.length; index += 1) {
    const character = text[index];
    if (character !== "\n" && character !== "\r") continue;
    const ending = character === "\r" && text[index + 1] === "\n" ? "\r\n" : character;
    lines.push({ text: text.slice(lineStart, index), ending });
    if (ending === "\r\n") index += 1;
    lineStart = index + 1;
  }
  if (lineStart < text.length) lines.push({ text: text.slice(lineStart), ending: "" });
  return lines;
}

function sameDiffLine(left: DiffLine, right: DiffLine): boolean {
  return left.text === right.text && left.ending === right.ending;
}

function equalOperations(lines: readonly DiffLine[]): DiffOperation[] {
  return lines.map((line) => ({ type: "equal", line }));
}

function replaceOperations(before: readonly DiffLine[], after: readonly DiffLine[]): DiffOperation[] {
  return [
    ...before.map((line) => ({ type: "delete" as const, line })),
    ...after.map((line) => ({ type: "insert" as const, line })),
  ];
}

function backtrackDiff(
  trace: readonly Map<number, number>[],
  before: readonly DiffLine[],
  after: readonly DiffLine[],
): DiffOperation[] | null {
  let beforeIndex = before.length;
  let afterIndex = after.length;
  const reversed: DiffOperation[] = [];

  for (let distance = trace.length - 1; distance > 0; distance -= 1) {
    const previous = trace[distance - 1];
    if (!previous) return null;
    const diagonal = beforeIndex - afterIndex;
    const shouldInsert = diagonal === -distance
      || (diagonal !== distance
        && (previous.get(diagonal - 1) ?? -Infinity) < (previous.get(diagonal + 1) ?? -Infinity));
    const previousDiagonal = shouldInsert ? diagonal + 1 : diagonal - 1;
    const previousBeforeIndex = previous.get(previousDiagonal);
    if (previousBeforeIndex === undefined) return null;
    const previousAfterIndex = previousBeforeIndex - previousDiagonal;

    while (beforeIndex > previousBeforeIndex && afterIndex > previousAfterIndex) {
      const line = before[beforeIndex - 1];
      if (!line) return null;
      reversed.push({ type: "equal", line });
      beforeIndex -= 1;
      afterIndex -= 1;
    }

    if (beforeIndex === previousBeforeIndex) {
      const line = after[afterIndex - 1];
      if (!line) return null;
      reversed.push({ type: "insert", line });
      afterIndex -= 1;
    } else {
      const line = before[beforeIndex - 1];
      if (!line) return null;
      reversed.push({ type: "delete", line });
      beforeIndex -= 1;
    }
  }

  while (beforeIndex > 0 && afterIndex > 0) {
    const line = before[beforeIndex - 1];
    if (!line) return null;
    reversed.push({ type: "equal", line });
    beforeIndex -= 1;
    afterIndex -= 1;
  }
  while (beforeIndex > 0) {
    const line = before[beforeIndex - 1];
    if (!line) return null;
    reversed.push({ type: "delete", line });
    beforeIndex -= 1;
  }
  while (afterIndex > 0) {
    const line = after[afterIndex - 1];
    if (!line) return null;
    reversed.push({ type: "insert", line });
    afterIndex -= 1;
  }
  return reversed.reverse();
}

function myersDiff(before: readonly DiffLine[], after: readonly DiffLine[]): DiffOperation[] | null {
  if (!before.length) return after.map((line) => ({ type: "insert", line }));
  if (!after.length) return before.map((line) => ({ type: "delete", line }));

  const maxDistance = before.length + after.length;
  const trace: Map<number, number>[] = [];
  let traceCells = 0;
  let frontier = new Map<number, number>([[0, 0]]);

  for (let distance = 0; distance <= maxDistance; distance += 1) {
    const next = new Map<number, number>();
    for (let diagonal = -distance; diagonal <= distance; diagonal += 2) {
      const shouldInsert = diagonal === -distance
        || (diagonal !== distance
          && (frontier.get(diagonal - 1) ?? -Infinity) < (frontier.get(diagonal + 1) ?? -Infinity));
      let beforeIndex = shouldInsert
        ? frontier.get(diagonal + 1) ?? 0
        : (frontier.get(diagonal - 1) ?? 0) + 1;
      let afterIndex = beforeIndex - diagonal;
      while (beforeIndex < before.length && afterIndex < after.length) {
        const beforeLine = before[beforeIndex];
        const afterLine = after[afterIndex];
        if (!beforeLine || !afterLine || !sameDiffLine(beforeLine, afterLine)) break;
        beforeIndex += 1;
        afterIndex += 1;
      }
      next.set(diagonal, beforeIndex);
    }

    traceCells += next.size;
    if (traceCells > MAX_DIFF_TRACE_CELLS) return null;
    trace.push(next);
    const completed = next.get(before.length - after.length);
    if (completed === before.length) return backtrackDiff(trace, before, after);
    frontier = next;
  }
  return null;
}

function diffOperations(before: readonly DiffLine[], after: readonly DiffLine[]): DiffOperation[] {
  let prefix = 0;
  while (prefix < before.length && prefix < after.length) {
    const beforeLine = before[prefix];
    const afterLine = after[prefix];
    if (!beforeLine || !afterLine || !sameDiffLine(beforeLine, afterLine)) break;
    prefix += 1;
  }

  let suffix = 0;
  while (before.length - suffix > prefix && after.length - suffix > prefix) {
    const beforeLine = before[before.length - suffix - 1];
    const afterLine = after[after.length - suffix - 1];
    if (!beforeLine || !afterLine || !sameDiffLine(beforeLine, afterLine)) break;
    suffix += 1;
  }

  const beforeMiddle = before.slice(prefix, before.length - suffix);
  const afterMiddle = after.slice(prefix, after.length - suffix);
  const middle = myersDiff(beforeMiddle, afterMiddle) || replaceOperations(beforeMiddle, afterMiddle);
  return [
    ...equalOperations(before.slice(0, prefix)),
    ...middle,
    ...equalOperations(before.slice(before.length - suffix)),
  ];
}

function formatDiffRange(start: number, count: number): string {
  return count === 1 ? `${start}` : `${start},${count}`;
}

function renderDiffHunks(operations: readonly DiffOperation[]): string[] {
  const changed = operations
    .map((operation, index) => operation.type === "equal" ? -1 : index)
    .filter((index) => index >= 0);
  if (!changed.length) return [];

  const hunks: DiffHunk[] = [];
  for (const index of changed) {
    const start = Math.max(0, index - DIFF_CONTEXT_LINES);
    const end = Math.min(operations.length, index + DIFF_CONTEXT_LINES + 1);
    const previous = hunks.at(-1);
    if (previous && start <= previous.end) previous.end = Math.max(previous.end, end);
    else hunks.push({ start, end });
  }

  const beforeOffsets = [0];
  const afterOffsets = [0];
  for (const operation of operations) {
    beforeOffsets.push((beforeOffsets.at(-1) ?? 0) + (operation.type === "insert" ? 0 : 1));
    afterOffsets.push((afterOffsets.at(-1) ?? 0) + (operation.type === "delete" ? 0 : 1));
  }

  const lines: string[] = [];
  for (const hunk of hunks) {
    const beforeStartCount = beforeOffsets[hunk.start] ?? 0;
    const afterStartCount = afterOffsets[hunk.start] ?? 0;
    const beforeCount = (beforeOffsets[hunk.end] ?? 0) - beforeStartCount;
    const afterCount = (afterOffsets[hunk.end] ?? 0) - afterStartCount;
    const beforeStart = beforeCount === 0 ? beforeStartCount : beforeStartCount + 1;
    const afterStart = afterCount === 0 ? afterStartCount : afterStartCount + 1;
    lines.push(`@@ -${formatDiffRange(beforeStart, beforeCount)} +${formatDiffRange(afterStart, afterCount)} @@`);

    for (const operation of operations.slice(hunk.start, hunk.end)) {
      const prefix = operation.type === "equal" ? " " : operation.type === "delete" ? "-" : "+";
      lines.push(`${prefix}${operation.line.text}`);
      if (!operation.line.ending) lines.push("\\ No newline at end of file");
    }
  }
  return lines;
}

export function createUnifiedDiff(
  changes: readonly Pick<FileChange, "path" | "content">[],
  beforeByPath: Readonly<Record<string, string>> = {},
): string {
  const lines: string[] = [];
  for (const change of changes) {
    const beforeValue = beforeByPath[change.path];
    const beforeContent = typeof beforeValue === "string" ? beforeValue : "";
    const operations = diffOperations(splitDiffLines(beforeContent), splitDiffLines(change.content));
    const hunks = renderDiffHunks(operations);
    if (!hunks.length) continue;
    lines.push(`--- a/${change.path}`, `+++ b/${change.path}`, ...hunks);
  }
  return lines.join("\n");
}

export function buildTurnPrompt(prompt: string, unifiedDiff = ""): string {
  const encoder = new TextEncoder();
  const decoder = new TextDecoder();
  const requestedBase = prompt.trim() ? prompt.trim() : "Review the staged WebMCP patch";
  const baseBytes = encoder.encode(requestedBase);
  const base = baseBytes.length <= MAX_TURN_PROMPT_BYTES
    ? requestedBase
    : decoder.decode(baseBytes.slice(0, MAX_TURN_PROMPT_BYTES));
  if (!unifiedDiff.length) return base;

  const baseSizeBytes = encoder.encode(base).length;
  const framingBytes = encoder.encode(`${TURN_DIFF_PREFIX}${TURN_DIFF_SUFFIX}`).length;
  const availableDiffBytes = MAX_TURN_PROMPT_BYTES - baseSizeBytes - framingBytes;
  if (availableDiffBytes <= 0) return base;

  const diffBytes = encoder.encode(unifiedDiff);
  if (diffBytes.length <= availableDiffBytes) return `${base}${TURN_DIFF_PREFIX}${unifiedDiff}${TURN_DIFF_SUFFIX}`;

  const truncationBytes = encoder.encode(TURN_DIFF_TRUNCATION).length;
  const contentBytes = Math.max(0, availableDiffBytes - truncationBytes);
  let clippedEnd = contentBytes;
  while (clippedEnd > 0 && ((diffBytes[clippedEnd] ?? 0) & 0xc0) === 0x80) clippedEnd -= 1;
  const clipped = decoder.decode(diffBytes.slice(0, clippedEnd));
  return `${base}${TURN_DIFF_PREFIX}${clipped}${TURN_DIFF_TRUNCATION}${TURN_DIFF_SUFFIX}`;
}

function validateChanges(changes: readonly FileChangeInput[]): FileChange[] {
  if (changes.length === 0 || changes.length > MAX_CHANGES) {
    throw backendError("A proposal must contain between one and 32 file changes", "limit_exceeded");
  }
  const seen = new Set<string>();
  const validated: FileChange[] = [];
  for (const change of changes) {
    validatePath(change.path);
    const path = change.path;
    if (seen.has(path)) throw backendError(`Duplicate change path: ${path}`, "invalid_request");
    if (typeof change.content !== "string" || new TextEncoder().encode(change.content).length > MAX_FILE_BYTES) {
      throw backendError("Proposed file content exceeds the size limit", "limit_exceeded");
    }
    const baseDigest = change.base_digest ?? change.baseDigest;
    if (typeof baseDigest !== "string" || baseDigest.length === 0 || baseDigest.length > 200) {
      throw backendError(`Missing base digest for ${path}`, "invalid_request");
    }
    validated.push({ path, base_digest: baseDigest, content: change.content });
    seen.add(path);
  }
  return validated;
}

export class WorkspaceBackend {
  explanation(_scope: ExplanationScope, _offset = 0): Promise<ExplanationPage> { return Promise.reject(new BackendError("Execution explanations require an active paired VT Code session", "unsupported")); }
  explanationEvidence(_reference: EvidenceRef, _offset = 0): Promise<EvidencePage> { return Promise.reject(new BackendError("Execution evidence is unavailable", "unsupported")); }
  explanationNavigate(_reference: EvidenceRef): Promise<{focused:boolean}> { return Promise.reject(new BackendError("Terminal navigation is unavailable", "unsupported")); }
  readonly kind: BackendKind = "fallback";
  connected = false;
  statusPayload: StatusPayload | null = null;

  listFiles(): Promise<WorkspaceFileEntry[]> {
    throw new Error("WorkspaceBackend.listFiles is not implemented");
  }

  readFile(_path: string): Promise<FileSnapshot> {
    throw new Error("WorkspaceBackend.readFile is not implemented");
  }

  proposeChanges(_rawChanges: readonly unknown[]): Promise<PatchProposal> {
    throw new Error("WorkspaceBackend.proposeChanges is not implemented");
  }

  applyProposal(_proposalId: string): Promise<AppliedChange> {
    throw new Error("WorkspaceBackend.applyProposal is not implemented");
  }

  runChecks(_command?: string): Promise<CheckResult> {
    throw new Error("WorkspaceBackend.runChecks is not implemented");
  }

  revertLastChange(_changeId: string): Promise<AppliedChange> {
    throw new Error("WorkspaceBackend.revertLastChange is not implemented");
  }

  requestTurn(_prompt: string, _proposalId?: string): Promise<TurnResult> {
    throw new Error("WorkspaceBackend.requestTurn is not implemented");
  }

  writeFile(_input: WriteFileInput): Promise<FileSnapshot | AppliedChange> {
    throw new Error("WorkspaceBackend.writeFile is not implemented");
  }

  check(): Promise<CheckResult> {
    throw new Error("WorkspaceBackend.check is not implemented");
  }

  exportFiles(): Record<string, string> {
    throw new Error("WorkspaceBackend.exportFiles is not implemented");
  }

  subscribeToEvents(_listener: BackendListener): () => void { return () => {}; }
  subscribeToConnection(_listener: ConnectionListener): () => void { return () => {}; }
  subscribeToStatus(_listener: StatusListener): () => void { return () => {}; }
  close(): void {}
}

export class InMemoryBackend extends WorkspaceBackend {
  override readonly kind = "fallback" as const;
  override connected = false;
  private readonly files: Record<string, string>;
  private readonly proposals = new Map<string, StoredProposal>();
  private lastChange: LastChange | null = null;
  private readonly listeners = new Set<BackendListener>();
  private mutationTail: Promise<void> = Promise.resolve();

  constructor(initialFiles: Readonly<Record<string, string>> = SEED_FILES) {
    super();
    this.files = cloneFiles(initialFiles);
  }

  override async listFiles(): Promise<string[]> { return Object.keys(this.files).sort(); }

  override exportFiles(): Record<string, string> { return cloneFiles(this.files); }

  override async readFile(path: string): Promise<FileSnapshot> {
    validatePath(path);
    const content = this.files[path];
    if (typeof content !== "string") throw backendError(`Unknown project file: ${path}`, "not_found");
    return { path, content, digest: await digest(content), size_bytes: new TextEncoder().encode(content).length };
  }

  override async proposeChanges(rawChanges: readonly unknown[]): Promise<PatchProposal> {
    return this.withMutation(async () => {
      const changes = validateChanges(rawChanges.map(normalizeChange));
      const beforeByPath: Record<string, string> = Object.create(null) as Record<string, string>;
      for (const change of changes) {
        const snapshot = await this.readFile(change.path);
        if (snapshot.digest !== change.base_digest) throw backendError(`Stale patch: external change detected for ${change.path}`, "conflict");
        beforeByPath[change.path] = snapshot.content;
      }
      const proposal: PatchProposal = {
        proposal_id: `fallback-${cryptoRandomId()}`,
        changes,
        unified_diff: createUnifiedDiff(changes, beforeByPath),
      };
      this.proposals.set(proposal.proposal_id, { proposal, beforeByPath });
      return proposal;
    });
  }

  override async applyProposal(proposalId: string): Promise<AppliedChange> {
    return this.withMutation(async () => {
      const stored = this.proposals.get(proposalId);
      if (!stored) throw backendError("Proposal was not found", "proposal_not_found");
      const before: FileSnapshot[] = [];
      for (const change of stored.proposal.changes) {
        const current = await this.readFile(change.path);
        if (current.digest !== change.base_digest) throw backendError(`Stale patch: external change detected for ${change.path}`, "conflict");
        before.push(current);
      }
      for (const change of stored.proposal.changes) this.files[change.path] = change.content;
      const after: FileSnapshot[] = [];
      for (const change of stored.proposal.changes) after.push(await this.readFile(change.path));
      const applied: AppliedChange = {
        change_id: `fallback-${cryptoRandomId()}`,
        paths: stored.proposal.changes.map((change) => change.path),
      };
      this.lastChange = { applied, before, after };
      this.proposals.delete(proposalId);
      this.emit({ type: "workspace.updated", paths: applied.paths });
      return applied;
    });
  }

  override async runChecks(): Promise<CheckResult> {
    const greeting = await this.readFile("src/greeting.js");
    const config = await this.readFile("src/config.js");
    const checks = [greeting.content.includes("return `Hello, ${name}!"), config.content.includes("export const name =")];
    const passed = checks.every(Boolean);
    return {
      command: "in-memory deterministic checks",
      passed,
      checks: checks.length,
      failures: checks.flatMap((ok, index) => ok ? [] : [`assertion_${index + 1}`]),
      stdout: passed ? "2 checks passed" : "deterministic check failed",
      stderr: "",
      exit_code: passed ? 0 : 1,
    };
  }

  override async revertLastChange(changeId: string): Promise<AppliedChange> {
    return this.withMutation(async () => {
      if (!this.lastChange || this.lastChange.applied.change_id !== changeId) throw backendError("Last change was not found", "change_not_found");
      for (const snapshot of this.lastChange.after) {
        const current = await this.readFile(snapshot.path);
        if (current.digest !== snapshot.digest) throw backendError(`File changed after apply: ${snapshot.path}`, "conflict");
      }
      for (const snapshot of this.lastChange.before) this.files[snapshot.path] = snapshot.content;
      const reverted = this.lastChange.applied;
      this.lastChange = null;
      this.emit({ type: "workspace.reverted", paths: reverted.paths });
      return reverted;
    });
  }

  override async requestTurn(prompt: string, _proposalId?: string): Promise<TurnResult> {
    if (new TextEncoder().encode(prompt).length > MAX_TURN_PROMPT_BYTES) {
      throw backendError("Prompt is too long", "limit_exceeded");
    }
    return {
      accepted: false,
      mode: this.kind,
      reason: "Fallback mode is browser-only. Pair a WebMCP bridge for workspace access; agent turns require an active VT Code runtime.",
      prompt,
    };
  }

  override subscribeToEvents(listener: BackendListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  override async writeFile({ path, content, baseDigest, base_digest }: WriteFileInput): Promise<FileSnapshot> {
    const current = await this.readFile(path);
    const proposal = await this.proposeChanges([{ path, content, base_digest: base_digest ?? baseDigest ?? current.digest }]);
    await this.applyProposal(proposal.proposal_id);
    return this.readFile(path);
  }

  override check(): Promise<CheckResult> { return this.runChecks(); }

  private async withMutation<T>(operation: () => Promise<T>): Promise<T> {
    const previous = this.mutationTail;
    let release!: () => void;
    this.mutationTail = new Promise<void>((resolve) => { release = resolve; });
    await previous;
    try { return await operation(); } finally { release(); }
  }

  private emit(event: BackendEvent): void {
    for (const listener of this.listeners) listener(event);
  }
}

export class VtCodeBackend extends WorkspaceBackend {
  override readonly kind = "websocket" as const;
  override connected = false;
  readonly url: string;
  private socket: WebSocket | null = null;
  token: string | null = null;
  private nextId = 1;
  private readonly pending = new Map<string, PendingRequest>();
  private readonly listeners = new Set<BackendListener>();
  private sequence = 0;
  private expectedEventSequence: number | null = null;
  private opening: Promise<void> | null = null;
  private openingReject: ((reason?: unknown) => void) | null = null;
  private resuming: Promise<PairPayload> | null = null;
  private sessionExpiresInSecs = DEFAULT_SESSION_TTL_SECS;
  private heartbeatTimer: ReturnType<typeof setInterval> | null = null;
  private heartbeatInFlight: Promise<StatusPayload> | null = null;
  private readonly connectionListeners = new Set<ConnectionListener>();
  private readonly statusListeners = new Set<StatusListener>();
  connectionState: BackendConnectionEvent["state"] = "disconnected";

  constructor(url: string) {
    super();
    if (!/^wss?:\/\//.test(url)) throw backendError("WebMCP URL must use ws:// or wss://", "invalid_request");
    this.url = url;
  }

  private setConnectionState(state: BackendConnectionEvent["state"], error: BackendError | null = null): void {
    if (this.connectionState === state && !error) return;
    this.connectionState = state;
    for (const listener of this.connectionListeners) listener({ state, error });
  }

  private setStatusPayload(payload: StatusPayload | null): void {
    this.statusPayload = payload;
    for (const listener of this.statusListeners) listener(payload);
  }

  private updateSessionLease(payload: PairPayload): void {
    if (Number.isSafeInteger(payload.expires_in_secs) && payload.expires_in_secs >= 0) {
      this.sessionExpiresInSecs = Math.max(1, payload.expires_in_secs);
    }
  }

  heartbeatIntervalMs(): number {
    return Math.max(MIN_HEARTBEAT_INTERVAL_MS, Math.min(MAX_HEARTBEAT_INTERVAL_MS, Math.floor(this.sessionExpiresInSecs * 1000 / 3)));
  }

  private startHeartbeat(): void {
    this.stopHeartbeat();
    this.heartbeatTimer = setInterval(() => { void this.keepAlive(); }, this.heartbeatIntervalMs());
  }

  private stopHeartbeat(): void {
    if (this.heartbeatTimer) clearInterval(this.heartbeatTimer);
    this.heartbeatTimer = null;
  }

  private async keepAlive(): Promise<void> {
    if (!this.token || this.heartbeatInFlight) return;
    const heartbeat = this.status();
    this.heartbeatInFlight = heartbeat;
    try {
      await heartbeat;
      if (this.connected) this.setConnectionState("connected");
    } catch (error: unknown) {
      const code = errorCode(error);
      if (code === "unauthorized" || code === "pairing_expired" || code === "request_in_progress") return;
      if (this.token && ["connection_closed", "connection_failed", "request_timeout"].includes(code ?? "")) {
        this.disconnectSocket(error instanceof BackendError ? error : backendError(String(error), code), "reconnecting");
      }
    } finally {
      if (this.heartbeatInFlight === heartbeat) this.heartbeatInFlight = null;
    }
  }

  private disconnectSocket(error: BackendError, state: BackendConnectionEvent["state"] = "disconnected"): void {
    const socket = this.socket;
    this.socket = null;
    this.connected = false;
    this.setStatusPayload(null);
    this.rejectPending(error);
    socket?.close();
    this.setConnectionState(state, error);
    if (this.sequence > 0 && this.expectedEventSequence === null) this.expectedEventSequence = this.sequence + 1;
  }

  private invalidateSession(error: BackendError): void {
    const socket = this.socket;
    this.socket = null;
    this.token = null;
    this.connected = false;
    this.setStatusPayload(null);
    this.stopHeartbeat();
    this.rejectPending(error);
    socket?.close();
    this.setConnectionState("reauthorize", error);
  }

  private protocolError(detail: string): BackendError {
    const error = backendError(`VT Code sent an invalid WebMCP frame: ${detail}`, "protocol_error");
    this.invalidateSession(error);
    return error;
  }

  private rejectPending(error: BackendError): void {
    for (const pending of this.pending.values()) {
      clearTimeout(pending.timeout);
      pending.reject(error);
    }
    this.pending.clear();
  }

  async pair(code: string): Promise<PairPayload> {
    if (!/^[A-Z0-9-]{4,64}$/.test(code)) throw backendError("Enter the pairing code shown by VT Code", "invalid_request");
    await this.open();
    const payload = await this.send("pair", {
      code,
      origin: globalThis.location?.origin,
      after_sequence: this.sequence || undefined,
    }, false);
    this.token = payload.token;
    this.updateSessionLease(payload);
    this.expectedEventSequence = this.sequence > 0 ? this.sequence + 1 : null;
    this.connected = true;
    this.startHeartbeat();
    this.setConnectionState("connected");
    return payload;
  }

  private async resume(): Promise<PairPayload> {
    if (!this.token) throw backendError("Pair this browser with VT Code first", "unauthorized");
    if (this.resuming) return this.resuming;
    const token = this.token;
    this.setConnectionState("reconnecting");
    const request = this.send("pair", {
      resume_token: token,
      origin: globalThis.location?.origin,
      after_sequence: this.sequence || undefined,
    }, false);
    const resuming = request.then((payload) => {
      if (this.token === token) {
        this.updateSessionLease(payload);
        this.expectedEventSequence = this.sequence > 0 ? this.sequence + 1 : null;
        this.connected = true;
        this.startHeartbeat();
        this.setConnectionState("connected");
      }
      return payload;
    });
    this.resuming = resuming;
    try { return await resuming; } finally { if (this.resuming === resuming) this.resuming = null; }
  }

  private async open(): Promise<void> {
    if (this.socket?.readyState === WebSocket.OPEN) return;
    if (this.opening) return this.opening;
    const opening = new Promise<void>((resolve, reject) => {
      const socket = new WebSocket(this.url);
      this.socket = socket;
      let settled = false;
      this.openingReject = reject;
      const fail = (error: BackendError): void => {
        if (!settled) {
          settled = true;
          if (this.openingReject === reject) this.openingReject = null;
          reject(error);
        }
      };
      socket.onopen = () => {
        settled = true;
        if (this.openingReject === reject) this.openingReject = null;
        resolve();
      };
      socket.onerror = () => {
        if (this.socket === socket) {
          fail(backendError(
            "VT Code WebSocket connection failed. Keep the current bridge running and use its newest WebSocket URL and pairing code.",
            "connection_failed",
          ));
        }
      };
      socket.onclose = () => {
        if (this.socket !== socket) return;
        const error = backendError("VT Code WebSocket disconnected", "connection_closed");
        this.disconnectSocket(error);
        fail(backendError("VT Code WebSocket closed before pairing", "connection_closed"));
      };
      socket.onmessage = (event) => this.receive(event.data as unknown);
    });
    this.opening = opening;
    try { await opening; } finally { if (this.opening === opening) this.opening = null; }
  }

  private receive(raw: unknown): void {
    let message: ReturnType<typeof parseBridgeFrame>;
    try {
      message = parseBridgeFrame(raw, MAX_BRIDGE_FRAME_BYTES);
    } catch (error: unknown) {
      const detail = error instanceof BackendError
        ? error.message.replace("VT Code sent an invalid WebMCP frame: ", "")
        : String(error);
      this.protocolError(detail);
      return;
    }

    if (message.type === "event") {
      if (this.expectedEventSequence !== null && message.sequence !== this.expectedEventSequence) {
        this.protocolError("runtime event sequence is invalid or has a gap");
        return;
      }
      if (message.sequence >= Number.MAX_SAFE_INTEGER) {
        this.protocolError("runtime event sequence is outside the browser-safe range");
        return;
      }
      this.sequence = message.sequence;
      this.expectedEventSequence = message.sequence + 1;
      for (const listener of this.listeners) listener(message);
      return;
    }

    const pending = this.pending.get(message.request_id);
    if (!pending) return;
    this.pending.delete(message.request_id);
    clearTimeout(pending.timeout);
    if (!message.ok) {
      const error = backendError(message.error?.message ?? "VT Code request failed", message.error?.code ?? "runtime_error");
      if (error.code === "unauthorized" || error.code === "pairing_expired") this.invalidateSession(error);
      pending.reject(error);
      return;
    }
    try {
      pending.resolve(validateResponsePayload(pending.operation, message.payload));
    } catch (error: unknown) {
      const detail = error instanceof BackendError
        ? error.message.replace("VT Code sent an invalid WebMCP frame: ", "")
        : String(error);
      const protocolError = this.protocolError(detail);
      pending.reject(protocolError);
    }
  }

  private async send<K extends BridgeOperation>(
    type: K,
    payload: RequestPayloads[K] = {} as RequestPayloads[K],
    authenticated = true,
  ): Promise<OperationPayloads[K]> {
    await this.open();
    if (authenticated && this.token && !this.connected) await this.resume();
    if (authenticated && !this.token) throw backendError("Pair this browser with VT Code first", "unauthorized");
    const requestId = `browser-${this.nextId++}`;
    const token = this.token;
    const request = {
      type,
      request_id: requestId,
      ...(authenticated && token ? { token } : {}),
      ...payload,
    } as BridgeRequest;
    const serialized = JSON.stringify(request);
    if (new TextEncoder().encode(serialized).length > MAX_BRIDGE_FRAME_BYTES) throw backendError("Request exceeds the frame limit", "limit_exceeded");

    return new Promise<OperationPayloads[K]>((resolve, reject) => {
      const timeout = setTimeout(() => {
        const pending = this.pending.get(requestId);
        if (!pending) return;
        this.pending.delete(requestId);
        pending.reject(backendError("VT Code request timed out", "request_timeout"));
      }, REQUEST_TIMEOUT_MS);
      const pending: PendingRequest = {
        operation: type,
        resolve: (responsePayload) => resolve(responsePayload as OperationPayloads[K]),
        reject,
        timeout,
      };
      this.pending.set(requestId, pending);
      const socket = this.socket;
      if (!socket || socket.readyState !== WebSocket.OPEN) {
        clearTimeout(timeout);
        this.pending.delete(requestId);
        const connectionError = backendError("VT Code WebSocket could not send the request", "connection_closed");
        this.disconnectSocket(connectionError, "reconnecting");
        reject(connectionError);
        return;
      }
      try {
        socket.send(serialized);
      } catch {
        clearTimeout(timeout);
        this.pending.delete(requestId);
        const connectionError = backendError("VT Code WebSocket could not send the request", "connection_closed");
        this.disconnectSocket(connectionError, "reconnecting");
        reject(connectionError);
      }
    });
  }

  override listFiles(): Promise<WorkspaceFileEntry[]> { return this.send("workspace.list_files"); }
  override readFile(path: string): Promise<FileSnapshot> { return this.send("workspace.read_file", { path }); }

  override proposeChanges(rawChanges: readonly unknown[]): Promise<PatchProposal> {
    const changes = validateChanges(rawChanges.map(normalizeChange));
    return this.send("patch.propose", { changes });
  }

  override applyProposal(proposalId: string): Promise<AppliedChange> { return this.send("patch.apply", { proposal_id: proposalId }); }
  override runChecks(command = "cargo check --locked"): Promise<CheckResult> { return this.send("checks.run", { command }); }
  override revertLastChange(changeId: string): Promise<AppliedChange> { return this.send("patch.revert", { change_id: changeId }); }

  async status(): Promise<StatusPayload> {
    const payload = await this.send("status");
    this.setStatusPayload(payload);
    return payload;
  }

  override explanation(scope: ExplanationScope, offset = 0): Promise<ExplanationPage> { return this.send("explanation.get", {scope,offset}); }
  override explanationEvidence(reference: EvidenceRef, offset = 0): Promise<EvidencePage> { return this.send("explanation.evidence", {reference,offset}); }
  override explanationNavigate(reference: EvidenceRef): Promise<{focused:boolean}> { return this.send("explanation.navigate", {reference}); }

  override requestTurn(prompt: string, proposalId?: string): Promise<TurnResult> {
    const payload: RequestPayloads["turn.request"] = proposalId ? { prompt, proposal_id: proposalId } : { prompt };
    return this.send("turn.request", payload);
  }

  override subscribeToEvents(listener: BackendListener): () => void {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  }

  override subscribeToConnection(listener: ConnectionListener): () => void {
    this.connectionListeners.add(listener);
    return () => this.connectionListeners.delete(listener);
  }

  override subscribeToStatus(listener: StatusListener): () => void {
    this.statusListeners.add(listener);
    return () => this.statusListeners.delete(listener);
  }

  override close(): void {
    const socket = this.socket;
    this.socket = null;
    this.stopHeartbeat();
    this.heartbeatInFlight = null;
    this.token = null;
    this.connected = false;
    this.connectionState = "closed";
    this.expectedEventSequence = null;
    this.setStatusPayload(null);
    this.openingReject?.(backendError("VT Code WebSocket disconnected", "connection_closed"));
    this.openingReject = null;
    this.rejectPending(backendError("VT Code WebSocket disconnected", "connection_closed"));
    socket?.close();
  }

  override async writeFile({ path, content, baseDigest, base_digest }: WriteFileInput): Promise<AppliedChange> {
    const current = await this.readFile(path);
    const proposal = await this.proposeChanges([{ path, content, base_digest: base_digest ?? baseDigest ?? current.digest }]);
    return this.applyProposal(proposal.proposal_id);
  }

  override check(): Promise<CheckResult> { return this.runChecks(); }
}

export const WebSocketVtCodeBackend = VtCodeBackend;

export function createBackend(initialFiles: Readonly<Record<string, string>> = SEED_FILES): InMemoryBackend {
  return new InMemoryBackend(initialFiles);
}

export async function connectVtCode(url: string, code: string): Promise<VtCodeBackend> {
  const backend = new VtCodeBackend(url);
  try {
    await backend.pair(code);
    await backend.status();
    return backend;
  } catch (error: unknown) {
    backend.close();
    throw error;
  }
}

function cryptoRandomId(): string {
  if (globalThis.crypto?.randomUUID) return globalThis.crypto.randomUUID();
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export { MAX_BRIDGE_FRAME_BYTES as MAX_FRAME_BYTES, PROTOCOL_VERSION, SEED_FILES };
