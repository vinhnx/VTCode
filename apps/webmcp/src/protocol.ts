import { BackendError, isRecord, type AppliedChange, type BridgeEvent, type BridgeSettings, type CanonicalRuntimeEvent, type CheckResult, type FileChange, type FileSnapshot, type PatchProposal, type RuntimeStatus, type StatusPayload, type TurnResult, type VersionedThreadEvent, type WorkspaceFile } from "./types.ts";

export const PROTOCOL_VERSION = "1";
import { isExplanationPage, isEvidencePage, type ExplanationPage, type EvidencePage, type EvidenceRef, type ExplanationScope } from "./explanation.ts";

interface ExplanationGetRequest { readonly type: "explanation.get"; readonly request_id: string; readonly token: string; readonly scope: ExplanationScope; readonly offset?: number }
interface ExplanationEvidenceRequest { readonly type: "explanation.evidence"; readonly request_id: string; readonly token: string; readonly reference: EvidenceRef; readonly offset?: number }
interface ExplanationNavigateRequest { readonly type: "explanation.navigate"; readonly request_id: string; readonly token: string; readonly reference: EvidenceRef }
export const MAX_REQUEST_ID_BYTES = 256;
export const MAX_FRAME_BYTES = 1024 * 1024;

export interface PairRequest {
  readonly type: "pair";
  readonly request_id: string;
  readonly code?: string;
  readonly resume_token?: string;
  readonly origin?: string;
  readonly after_sequence?: number;
}

export interface StatusRequest {
  readonly type: "status";
  readonly request_id: string;
  readonly token: string;
}

export interface ListFilesRequest {
  readonly type: "workspace.list_files";
  readonly request_id: string;
  readonly token: string;
}

export interface ReadFileRequest {
  readonly type: "workspace.read_file";
  readonly request_id: string;
  readonly token: string;
  readonly path: string;
}

export interface ProposeChangesRequest {
  readonly type: "patch.propose";
  readonly request_id: string;
  readonly token: string;
  readonly changes: FileChange[];
}

export interface ApplyProposalRequest {
  readonly type: "patch.apply";
  readonly request_id: string;
  readonly token: string;
  readonly proposal_id: string;
}

export interface RunChecksRequest {
  readonly type: "checks.run";
  readonly request_id: string;
  readonly token: string;
  readonly command: string;
}

export interface RevertLastChangeRequest {
  readonly type: "patch.revert";
  readonly request_id: string;
  readonly token: string;
  readonly change_id: string;
}

export interface RequestTurnRequest {
  readonly type: "turn.request";
  readonly request_id: string;
  readonly token: string;
  readonly proposal_id?: string;
  readonly prompt: string;
}

export interface CancelRequest {
  readonly type: "cancel";
  readonly request_id: string;
  readonly token: string;
  readonly target_id: string;
}

export type BridgeRequest =
  | ExplanationGetRequest | ExplanationEvidenceRequest | ExplanationNavigateRequest
  | PairRequest
  | StatusRequest
  | ListFilesRequest
  | ReadFileRequest
  | ProposeChangesRequest
  | ApplyProposalRequest
  | RunChecksRequest
  | RevertLastChangeRequest
  | RequestTurnRequest
  | CancelRequest;

export type BridgeOperation =
  | "explanation.get" | "explanation.evidence" | "explanation.navigate"
  | "pair"
  | "status"
  | "workspace.list_files"
  | "workspace.read_file"
  | "patch.propose"
  | "patch.apply"
  | "checks.run"
  | "patch.revert"
  | "turn.request"
  | "cancel";

export interface RequestPayloads {
  readonly "explanation.get": {scope: ExplanationScope; offset?: number};
  readonly "explanation.evidence": {reference: EvidenceRef; offset?: number};
  readonly "explanation.navigate": {reference: EvidenceRef};
  readonly pair: Omit<PairRequest, "type" | "request_id">;
  readonly status: Record<string, never>;
  readonly "workspace.list_files": Record<string, never>;
  readonly "workspace.read_file": Pick<ReadFileRequest, "path">;
  readonly "patch.propose": Pick<ProposeChangesRequest, "changes">;
  readonly "patch.apply": Pick<ApplyProposalRequest, "proposal_id">;
  readonly "checks.run": Pick<RunChecksRequest, "command">;
  readonly "patch.revert": Pick<RevertLastChangeRequest, "change_id">;
  readonly "turn.request": Pick<RequestTurnRequest, "prompt"> & Partial<Pick<RequestTurnRequest, "proposal_id">>;
  readonly cancel: Pick<CancelRequest, "target_id">;
}

export interface OperationPayloads {
  readonly "explanation.get": ExplanationPage;
  readonly "explanation.evidence": EvidencePage;
  readonly "explanation.navigate": {focused:boolean};
  readonly pair: PairPayload;
  readonly status: StatusPayload;
  readonly "workspace.list_files": WorkspaceFile[];
  readonly "workspace.read_file": FileSnapshot;
  readonly "patch.propose": PatchProposal;
  readonly "patch.apply": AppliedChange;
  readonly "checks.run": CheckResult;
  readonly "patch.revert": AppliedChange;
  readonly "turn.request": TurnResult;
  readonly cancel: CancelPayload;
}

export interface PairPayload {
  readonly token: string;
  readonly protocol_version: string;
  readonly expires_in_secs: number;
}

export interface CancelPayload {
  readonly cancelled: string;
  readonly accepted: boolean;
}

export interface BridgeErrorPayload {
  readonly code: string;
  readonly message: string;
}

export interface BridgeResponseEnvelope {
  readonly type: "response";
  readonly request_id: string;
  readonly ok: boolean;
  readonly payload?: unknown;
  readonly error?: BridgeErrorPayload;
}

export interface BridgeEventMessage extends BridgeEvent {}

export type IncomingBridgeMessage = BridgeResponseEnvelope | BridgeEventMessage;

function hasOwn(value: Record<string, unknown>, key: string): boolean {
  return Object.prototype.hasOwnProperty.call(value, key);
}

function isNonEmptyString(value: unknown, maxLength = Number.POSITIVE_INFINITY): value is string {
  return typeof value === "string" && value.length > 0 && value.length <= maxLength;
}

function isSafeNonNegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isSafePositiveInteger(value: unknown): value is number {
  return isSafeNonNegativeInteger(value) && value > 0;
}

function isOptionalString(value: Record<string, unknown>, key: string): boolean {
  return !hasOwn(value, key) || value[key] === null || isNonEmptyString(value[key]);
}

function isWorkspacePath(value: unknown): value is string {
  return isNonEmptyString(value, 4096)
    && !value.includes("\0")
    && !value.startsWith("/")
    && !value.split("/").some((part) => part === "..");
}

export function isValidRequestId(value: unknown): value is string {
  return isNonEmptyString(value) && new TextEncoder().encode(value).length <= MAX_REQUEST_ID_BYTES;
}

export function isBridgeResponseEnvelope(value: unknown): value is BridgeResponseEnvelope {
  if (!isRecord(value)
    || value.type !== "response"
    || !isValidRequestId(value.request_id)
    || typeof value.ok !== "boolean") {
    return false;
  }
  if (value.ok) return hasOwn(value, "payload") && !hasOwn(value, "error");
  return !hasOwn(value, "payload")
    && isRecord(value.error)
    && isNonEmptyString(value.error.code, 128)
    && isNonEmptyString(value.error.message, 16 * 1024);
}

export function isCanonicalRuntimeEvent(value: unknown): value is CanonicalRuntimeEvent {
  if (!isRecord(value) || !isNonEmptyString(value.type, 128)) return false;
  switch (value.type) {
    case "thread.started":
      return isNonEmptyString(value.thread_id);
    case "thread.completed":
      return isNonEmptyString(value.thread_id)
        && isNonEmptyString(value.session_id)
        && isNonEmptyString(value.subtype, 64)
        && isNonEmptyString(value.outcome_code, 256)
        && isUsage(value.usage)
        && isSafeNonNegativeInteger(value.num_turns)
        && isOptionalString(value, "result")
        && isOptionalString(value, "stop_reason")
        && (!hasOwn(value, "total_cost_usd") || typeof value.total_cost_usd === "number");
    case "thread.compact_boundary":
      return isNonEmptyString(value.thread_id)
        && isNonEmptyString(value.trigger, 64)
        && isNonEmptyString(value.mode, 64)
        && isSafeNonNegativeInteger(value.original_message_count)
        && isSafeNonNegativeInteger(value.compacted_message_count)
        && isOptionalString(value, "history_artifact_path")
        && isOptionalString(value, "previous_segment_id")
        && isOptionalString(value, "new_segment_id")
        && isOptionalString(value, "previous_prefix_hash")
        && isOptionalString(value, "new_prefix_hash")
        && isOptionalString(value, "previous_catalog_hash")
        && isOptionalString(value, "new_catalog_hash");
    case "context.reset":
      return isNonEmptyString(value.thread_id)
        && isNonEmptyString(value.turn_id)
        && isNonEmptyString(value.trigger, 64)
        && typeof value.plan_preserved === "boolean"
        && typeof value.previous_context_usage_percent === "number"
        && Number.isInteger(value.previous_context_usage_percent)
        && value.previous_context_usage_percent >= 0
        && value.previous_context_usage_percent <= 255
        && typeof value.tool_budget_reset === "boolean";
    case "turn.started":
      return !hasOwn(value, "token_breakdown") || isTokenBreakdown(value.token_breakdown);
    case "turn.completed":
      return isUsage(value.usage);
    case "turn.failed":
      return isNonEmptyString(value.message, 16 * 1024)
        && (!hasOwn(value, "usage") || value.usage === null || isUsage(value.usage));
    case "item.started":
    case "item.updated":
    case "item.completed":
      return isThreadItem(value.item);
    case "permission.requested":
      return isNonEmptyString(value.tool_name, 4096);
    case "permission.resolved":
      return isNonEmptyString(value.tool_name, 4096)
        && isNonEmptyString(value.decision, 64)
        && isSafeNonNegativeInteger(value.wait_ms);
    case "interjected":
      return isNonEmptyString(value.source, 64)
        && isSafeNonNegativeInteger(value.image_count)
        && isNonEmptyString(value.redirect_kind, 64);
    case "plan.delta":
      return isNonEmptyString(value.thread_id)
        && isNonEmptyString(value.turn_id)
        && isNonEmptyString(value.item_id)
        && typeof value.delta === "string";
    case "plan.approval.requested":
      return isNonEmptyString(value.thread_id)
        && isNonEmptyString(value.turn_id)
        && isOptionalString(value, "plan_file");
    case "plan.approval.resolved":
      return isNonEmptyString(value.thread_id)
        && isNonEmptyString(value.turn_id)
        && isNonEmptyString(value.decision, 64)
        && typeof value.automatic === "boolean";
    case "error":
      return isNonEmptyString(value.message, 16 * 1024);
    default:
      // ThreadEvent has an explicit serde(other) variant. Keep accepting new
      // discriminators so an older browser can observe, but not interpret,
      // events added by a newer VT Code runtime.
      return true;
  }
}

export function isVersionedThreadEvent(value: unknown): value is VersionedThreadEvent {
  return isRecord(value)
    && isNonEmptyString(value.schema_version, 64)
    && isCanonicalRuntimeEvent(value.event);
}

export function isBridgeEventMessage(value: unknown): value is BridgeEventMessage {
  return isRecord(value)
    && value.type === "event"
    && isSafePositiveInteger(value.sequence)
    && isVersionedThreadEvent(value.event);
}

export function parseBridgeFrame(raw: unknown, maxFrameBytes = MAX_FRAME_BYTES): IncomingBridgeMessage {
  if (typeof raw !== "string") throw protocolError("frame is missing or is not a text message");
  if (new TextEncoder().encode(raw).length > maxFrameBytes) throw protocolError("frame exceeds the configured limit");
  let parsed: unknown;
  try {
    parsed = JSON.parse(raw) as unknown;
  } catch {
    throw protocolError("frame is not valid JSON");
  }
  if (isBridgeResponseEnvelope(parsed) || isBridgeEventMessage(parsed)) return parsed;
  throw protocolError("frame does not match a response or event envelope");
}

export function validateResponsePayload(operation: BridgeOperation, payload: unknown): unknown {
  const valid = (() => {
    switch (operation) {
      case "explanation.get": return isExplanationPage(payload);
      case "explanation.evidence": return isEvidencePage(payload);
      case "explanation.navigate": return isRecord(payload) && typeof payload.focused === "boolean";
      case "pair": return isPairPayload(payload);
      case "status": return isStatusPayload(payload);
      case "workspace.list_files": return isWorkspaceFileList(payload);
      case "workspace.read_file": return isFileSnapshot(payload);
      case "patch.propose": return isPatchProposal(payload);
      case "patch.apply":
      case "patch.revert": return isAppliedChange(payload);
      case "checks.run": return isCheckResult(payload);
      case "turn.request": return isTurnResult(payload);
      case "cancel": return isCancelPayload(payload);
    }
  })();
  if (!valid) throw protocolError(`response payload for ${operation} is invalid`);
  return payload;
}

export function responseRequestId(value: unknown): string {
  return isValidRequestId(value) ? value : "unknown";
}

function isPairPayload(value: unknown): value is PairPayload {
  return isRecord(value)
    && isNonEmptyString(value.token, 4096)
    && value.protocol_version === PROTOCOL_VERSION
    && isSafeNonNegativeInteger(value.expires_in_secs);
}

function isRuntimeStatus(value: unknown): value is RuntimeStatus {
  return isRecord(value)
    && typeof value.workspace_root === "string"
    && typeof value.connected === "boolean"
    && (!hasOwn(value, "explanations_available") || typeof value.explanations_available === "boolean")
    && typeof value.turns_available === "boolean"
    && typeof value.mutations_allowed === "boolean"
    && typeof value.checks_allowed === "boolean"
    && typeof value.approval_authority === "string";
}

function isBridgeSettings(value: unknown): value is BridgeSettings {
  return isRecord(value)
    && typeof value.host === "string"
    && isSafeNonNegativeInteger(value.port)
    && isSafePositiveInteger(value.pairing_ttl_secs)
    && isSafePositiveInteger(value.max_frame_bytes)
    && isSafePositiveInteger(value.max_in_flight_requests)
    && typeof value.remote_enabled === "boolean";
}

function isStatusPayload(value: unknown): value is StatusPayload {
  return isRecord(value)
    && value.protocol_version === PROTOCOL_VERSION
    && typeof value.connected === "boolean"
    && isRuntimeStatus(value.runtime)
    && typeof value.authenticated_origin === "string"
    && isBridgeSettings(value.settings)
    && isSafeNonNegativeInteger(value.latest_sequence);
}

function isWorkspaceFile(value: unknown): value is WorkspaceFile {
  return isRecord(value)
    && isWorkspacePath(value.path)
    && isSafeNonNegativeInteger(value.size_bytes)
    && isNonEmptyString(value.digest, 256);
}

function isWorkspaceFileList(value: unknown): value is WorkspaceFile[] {
  return Array.isArray(value) && value.length <= 4096 && value.every(isWorkspaceFile);
}

function isFileSnapshot(value: unknown): value is FileSnapshot {
  return isRecord(value)
    && isWorkspacePath(value.path)
    && typeof value.content === "string"
    && isNonEmptyString(value.digest, 256)
    && (!hasOwn(value, "size_bytes") || isSafeNonNegativeInteger(value.size_bytes))
    && (!hasOwn(value, "base_digest") || value.base_digest === null || isNonEmptyString(value.base_digest, 256))
    && (!hasOwn(value, "draft") || typeof value.draft === "boolean");
}

function isFileChange(value: unknown): value is FileChange {
  return isRecord(value)
    && isWorkspacePath(value.path)
    && isNonEmptyString(value.base_digest, 256)
    && typeof value.content === "string";
}

function isPatchProposal(value: unknown): value is PatchProposal {
  return isRecord(value)
    && isNonEmptyString(value.proposal_id, 4096)
    && Array.isArray(value.changes)
    && value.changes.length > 0
    && value.changes.length <= 32
    && value.changes.every(isFileChange)
    && typeof value.unified_diff === "string";
}

function isAppliedChange(value: unknown): value is AppliedChange {
  return isRecord(value)
    && isNonEmptyString(value.change_id, 4096)
    && Array.isArray(value.paths)
    && value.paths.length > 0
    && value.paths.length <= 32
    && value.paths.every(isWorkspacePath);
}

function isCheckResult(value: unknown): value is CheckResult {
  return isRecord(value)
    && typeof value.command === "string"
    && (value.exit_code === null || (typeof value.exit_code === "number" && Number.isSafeInteger(value.exit_code)))
    && typeof value.stdout === "string"
    && typeof value.stderr === "string"
    && (!hasOwn(value, "passed") || typeof value.passed === "boolean")
    && (!hasOwn(value, "checks") || isSafeNonNegativeInteger(value.checks))
    && (!hasOwn(value, "failures") || (Array.isArray(value.failures) && value.failures.every((failure) => typeof failure === "string")));
}

function isTurnResult(value: unknown): value is TurnResult {
  return isRecord(value)
    && isNonEmptyString(value.turn_id, 4096)
    && typeof value.accepted === "boolean"
    && (!hasOwn(value, "mode") || typeof value.mode === "string")
    && (!hasOwn(value, "reason") || typeof value.reason === "string")
    && (!hasOwn(value, "prompt") || typeof value.prompt === "string")
    && (!hasOwn(value, "output") || typeof value.output === "string");
}

function isCancelPayload(value: unknown): value is CancelPayload {
  return isRecord(value) && isNonEmptyString(value.cancelled, 4096) && typeof value.accepted === "boolean";
}

function isUsage(value: unknown): boolean {
  return isRecord(value)
    && isSafeNonNegativeInteger(value.input_tokens)
    && isSafeNonNegativeInteger(value.cached_input_tokens)
    && isSafeNonNegativeInteger(value.cache_creation_tokens)
    && isSafeNonNegativeInteger(value.output_tokens);
}

function isTokenBreakdown(value: unknown): boolean {
  return isRecord(value)
    && isSafeNonNegativeInteger(value.system_prompt_tokens)
    && isSafeNonNegativeInteger(value.tool_schema_tokens)
    && isSafeNonNegativeInteger(value.instruction_file_tokens)
    && isSafeNonNegativeInteger(value.message_history_tokens)
    && isSafeNonNegativeInteger(value.cache_read_tokens)
    && isSafeNonNegativeInteger(value.cache_write_tokens)
    && isSafeNonNegativeInteger(value.cache_miss_tokens)
    && (!hasOwn(value, "subagent_bootstrap_tokens") || isSafeNonNegativeInteger(value.subagent_bootstrap_tokens));
}

function isThreadItem(value: unknown): boolean {
  if (!isRecord(value) || !isNonEmptyString(value.id, 4096) || !isNonEmptyString(value.type, 64)) return false;
  switch (value.type) {
    case "agent_message":
    case "plan":
      return typeof value.text === "string";
    case "reasoning":
      return typeof value.text === "string" && isOptionalString(value, "stage");
    case "command_execution":
      return typeof value.command === "string"
        && isNonEmptyString(value.status, 64)
        && typeof value.aggregated_output === "string"
        && (!hasOwn(value, "exit_code") || value.exit_code === null || typeof value.exit_code === "number");
    case "tool_invocation":
      return isNonEmptyString(value.tool_name, 4096)
        && isNonEmptyString(value.status, 64)
        && isOptionalString(value, "tool_call_id")
        && isOptionalString(value, "outcome");
    case "tool_output":
      return isNonEmptyString(value.call_id, 4096)
        && typeof value.output === "string"
        && isNonEmptyString(value.status, 64)
        && isOptionalString(value, "tool_call_id")
        && isOptionalString(value, "spool_path")
        && (!hasOwn(value, "exit_code") || value.exit_code === null || typeof value.exit_code === "number");
    case "file_change":
      return Array.isArray(value.changes)
        && value.changes.every((change) => isRecord(change)
          && isWorkspacePath(change.path)
          && isNonEmptyString(change.kind, 64))
        && isNonEmptyString(value.status, 64);
    case "mcp_tool_call":
      return isNonEmptyString(value.tool_name, 4096)
        && isOptionalString(value, "result")
        && isOptionalString(value, "status");
    case "web_search":
      return typeof value.query === "string"
        && isOptionalString(value, "provider")
        && (!hasOwn(value, "results") || (Array.isArray(value.results) && value.results.every((result) => typeof result === "string")));
    case "harness":
      return isNonEmptyString(value.event, 128)
        && isOptionalString(value, "message")
        && isOptionalString(value, "command")
        && isOptionalString(value, "path")
        && (!hasOwn(value, "exit_code") || value.exit_code === null || typeof value.exit_code === "number")
        && (!hasOwn(value, "attempt") || isSafeNonNegativeInteger(value.attempt))
        && isOptionalString(value, "error_category")
        && (!hasOwn(value, "duration_ms") || isSafeNonNegativeInteger(value.duration_ms));
    case "error":
      return isNonEmptyString(value.message, 16 * 1024);
    default:
      return false;
  }
}

function protocolError(detail: string): BackendError {
  return new BackendError(`VT Code sent an invalid WebMCP frame: ${detail}`, "protocol_error");
}
