export type BackendKind = "fallback" | "websocket";

export type Panel = "activity" | "changes" | "turn";

export type WorkflowState = "workspace_ready" | "file_selected" | "draft_needs_review";

export interface WorkspaceFile {
  readonly path: string;
  readonly size_bytes?: number;
  readonly digest?: string;
}

export type WorkspaceFileEntry = string | WorkspaceFile;

export interface FileSnapshot {
  readonly path: string;
  readonly content: string;
  readonly digest: string;
  readonly size_bytes?: number;
  readonly base_digest?: string | null;
  readonly draft?: boolean;
}

export interface FileChange {
  readonly path: string;
  readonly base_digest: string;
  readonly content: string;
}

export interface FileChangeInput {
  readonly path?: string;
  readonly base_digest?: string;
  readonly baseDigest?: string;
  readonly content?: string;
}

export interface ClientProposal {
  readonly changes: FileChange[];
  readonly unified_diff: string;
}

export interface PatchProposal extends ClientProposal {
  readonly proposal_id: string;
}

export interface AppliedChange {
  readonly change_id: string;
  readonly paths: string[];
}

export interface CheckResult {
  readonly command: string;
  readonly passed?: boolean;
  readonly checks?: number;
  readonly failures?: string[];
  readonly stdout: string;
  readonly stderr: string;
  readonly exit_code: number | null;
}

export interface TurnResult {
  readonly accepted: boolean;
  readonly mode?: string;
  readonly reason?: string;
  readonly prompt?: string;
  readonly turn_id?: string;
  readonly output?: string;
}

export interface RuntimeStatus {
  readonly explanations_available?: boolean;
  readonly workspace_root: string;
  readonly connected: boolean;
  readonly turns_available: boolean;
  readonly mutations_allowed: boolean;
  readonly checks_allowed: boolean;
  readonly approval_authority: string;
}

export interface BridgeSettings {
  readonly host: string;
  readonly port: number;
  readonly pairing_ttl_secs: number;
  readonly max_frame_bytes: number;
  readonly max_in_flight_requests: number;
  readonly remote_enabled: boolean;
}

export interface StatusPayload {
  readonly protocol_version: string;
  readonly connected: boolean;
  readonly runtime: RuntimeStatus;
  readonly authenticated_origin: string;
  readonly settings: BridgeSettings;
  readonly latest_sequence: number;
}

export interface CanonicalRuntimeEvent {
  readonly type: string;
  readonly [key: string]: unknown;
}

export interface VersionedThreadEvent {
  readonly schema_version: string;
  readonly event: CanonicalRuntimeEvent;
}

export interface BridgeEvent {
  readonly type: "event";
  readonly sequence: number;
  readonly event: VersionedThreadEvent;
}

export interface FallbackWorkspaceEvent {
  readonly type: "workspace.updated" | "workspace.reverted";
  readonly paths: string[];
}

export type BackendEvent = BridgeEvent | FallbackWorkspaceEvent;

export type ConnectionState = "connected" | "reconnecting" | "reauthorize" | "disconnected" | "closed";

export interface BackendConnectionEvent {
  readonly state: ConnectionState;
  readonly error: BackendError | null;
}

export class BackendError extends Error {
  readonly code: string;
  proposalRecovery?: string;

  constructor(message: string, code = "backend_error") {
    super(message);
    this.name = "BackendError";
    this.code = code;
  }
}

export interface WriteFileInput {
  readonly path: string;
  readonly content: string;
  readonly baseDigest?: string;
  readonly base_digest?: string;
}

export interface TreeNode {
  readonly directories: Map<string, TreeNode>;
  readonly files: string[];
}

export interface PersistedBrowserState {
  readonly version: 1;
  readonly app_instance: string;
  readonly fallback_files: Record<string, string>;
  readonly drafts: Record<string, string>;
  readonly open_tabs: string[];
  readonly selected: string | null;
  readonly expanded_dirs: string[];
  readonly filter: string;
  readonly workspace_path: string;
}

export interface BrowserStateToSave {
  readonly fallback_files: Record<string, string>;
  readonly drafts: Record<string, string>;
  readonly open_tabs: string[];
  readonly selected: string | null;
  readonly expanded_dirs: string[];
  readonly filter: string;
  readonly workspace_path: string;
}

export interface PersistedBrowserSettings {
  readonly version: 1;
  readonly app_instance: string;
  readonly workspace_path: string;
  readonly bridge_url: string;
}

export interface BrowserSettingsToSave {
  readonly [key: string]: unknown;
  readonly workspace_path: string;
  readonly bridge_url: string;
}

export interface SearchMatch {
  readonly path: string;
  readonly line: number;
  readonly text: string;
}

export interface SearchResult {
  readonly matches: SearchMatch[];
  readonly truncated: boolean;
  readonly scanned_files: number;
  readonly scanned_bytes: number;
  readonly hint?: string;
}

export interface WebMcpEnvironmentState {
  readonly browsing_context_required: true;
  readonly origin_agent_cluster: boolean | null;
  readonly tools_permission_allowed: boolean | null;
}

export interface EditorStateForWebMcp {
  readonly backend: BackendKind;
  readonly connected: boolean;
  readonly workspace_root: string | null;
  readonly bridge_settings: BridgeSettings | null;
  readonly authenticated_origin: string | null;
  readonly selected: string | null;
  readonly open_tabs: string[];
  readonly dirty_files: string[];
  readonly has_client_proposal: boolean;
  readonly has_server_proposal: boolean;
  readonly active_panel: Panel;
  readonly workflow_state: WorkflowState;
  readonly recommended_next_tools: string[];
  readonly webmcp_context: WebMcpEnvironmentState;
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

export function errorCode(error: unknown): string | undefined {
  return isRecord(error) && typeof error.code === "string" ? error.code : undefined;
}

export function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
