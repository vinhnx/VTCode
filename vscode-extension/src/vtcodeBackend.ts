import * as pty from "node-pty";
import type { ChildProcess } from "node:child_process";
import { spawn } from "node:child_process";
import { createInterface } from "readline";
import * as vscode from "vscode";
import type { VtcodeConfigSummary } from "./vtcodeConfig";

const ANSI_CSI_PATTERN = new RegExp(String.raw`\u001B\[[0-9;]*[A-Za-z]`, "g");
const ANSI_OSC_PATTERN = new RegExp(String.raw`\u001B][^\u0007]*\u0007`, "g");

export interface VtcodeToolCall {
    readonly id: string;
    readonly name: string;
    readonly args: Record<string, unknown>;
}

export interface VtcodeToolExecutionResult {
    readonly text: string;
    readonly result?: unknown;
    readonly exitCode?: number;
}

export type VtcodeStreamChunk =
    | { kind: "text"; text: string }
    | { kind: "reasoning"; text: string }
    | { kind: "metadata"; metadata: Record<string, unknown> }
    | {
          kind: "toolCall";
          call: VtcodeToolCall;
          respond: (result: unknown) => void;
          reject: (error: string) => void;
      }
    | VtcodeToolResultChunk
    | { kind: "error"; message: string }
    | { kind: "done" };

export interface VtcodeToolResultChunk {
    readonly kind: "toolResult";
    readonly id: string;
    readonly toolType: "command" | "mcp";
    readonly name: string;
    readonly status: "started" | "in_progress" | "completed" | "failed";
    readonly output?: string;
    readonly exitCode?: number;
    readonly arguments?: unknown;
    readonly rawEvent?: Record<string, unknown>;
}

interface StreamPromptOptions {
    readonly prompt: string;
    readonly systemPrompt?: string;
    readonly context?: string;
}

type ExecThreadEvent =
    | ExecItemEvent
    | ExecThreadStartedEvent
    | ExecTurnStartedEvent
    | ExecTurnCompletedEvent
    | ExecTurnFailedEvent
    | ExecErrorEvent
    | Record<string, unknown>;

interface ExecItemEvent {
    readonly type: "item.started" | "item.updated" | "item.completed";
    readonly item: ExecThreadItem;
}

interface ExecThreadStartedEvent {
    readonly type: "thread.started";
    readonly thread_id: string;
}

interface ExecTurnStartedEvent {
    readonly type: "turn.started";
}

interface ExecTurnCompletedEvent {
    readonly type: "turn.completed";
    readonly usage: ExecUsage;
}

interface ExecTurnFailedEvent {
    readonly type: "turn.failed";
    readonly message: string;
    readonly usage?: ExecUsage;
}

interface ExecErrorEvent {
    readonly type: "error";
    readonly message: string;
}

interface ExecUsage {
    readonly input_tokens: number;
    readonly cached_input_tokens: number;
    readonly output_tokens: number;
}

interface ExecThreadItem {
    readonly id: string;
    readonly type: string;
    readonly text?: string;
    readonly command?: string;
    readonly aggregated_output?: string;
    readonly exit_code?: number;
    readonly status?: string;
    readonly tool_name?: string;
    readonly arguments?: unknown;
    readonly result?: string;
    readonly changes?: readonly ExecFileChange[];
}

interface ExecFileChange {
    readonly path: string;
    readonly kind: string;
}

interface ExecStreamState {
    readonly textByItemId: Map<string, string>;
    readonly reasoningByItemId: Map<string, string>;
    readonly commandState: Map<string, ExecCommandState>;
    readonly toolState: Map<string, ExecToolCallState>;
}

interface ExecCommandState {
    command?: string;
    aggregatedOutput?: string;
    exitCode?: number;
    status?: string;
    emitted?: boolean;
}

interface ExecToolCallState {
    toolName?: string;
    arguments?: unknown;
    result?: string;
    status?: string;
    emitted?: boolean;
}

function isExecItemEvent(event: ExecThreadEvent): event is ExecItemEvent {
    return (
        typeof event === "object" &&
        event !== null &&
        "type" in event &&
        (event as { type?: unknown }).type !== undefined &&
        ((event as { type?: unknown }).type === "item.started" ||
            (event as { type?: unknown }).type === "item.updated" ||
            (event as { type?: unknown }).type === "item.completed") &&
        "item" in event &&
        typeof (event as { item?: unknown }).item === "object" &&
        (event as { item?: unknown }).item !== null
    );
}

function isExecThreadStartedEvent(
    event: ExecThreadEvent
): event is ExecThreadStartedEvent {
    return (
        typeof event === "object" &&
        event !== null &&
        (event as { type?: unknown }).type === "thread.started" &&
        typeof (event as { thread_id?: unknown }).thread_id === "string"
    );
}

function isExecTurnCompletedEvent(
    event: ExecThreadEvent
): event is ExecTurnCompletedEvent {
    if (
        typeof event !== "object" ||
        event === null ||
        (event as { type?: unknown }).type !== "turn.completed"
    ) {
        return false;
    }

    const usage = (event as { usage?: ExecUsage }).usage;
    return (
        usage !== undefined &&
        usage !== null &&
        typeof usage.input_tokens === "number" &&
        typeof usage.cached_input_tokens === "number" &&
        typeof usage.output_tokens === "number"
    );
}

function isExecTurnFailedEvent(
    event: ExecThreadEvent
): event is ExecTurnFailedEvent {
    return (
        typeof event === "object" &&
        event !== null &&
        (event as { type?: unknown }).type === "turn.failed" &&
        typeof (event as { message?: unknown }).message === "string"
    );
}

function isExecErrorEvent(event: ExecThreadEvent): event is ExecErrorEvent {
    return (
        typeof event === "object" &&
        event !== null &&
        (event as { type?: unknown }).type === "error" &&
        typeof (event as { message?: unknown }).message === "string"
    );
}

interface PtyCommandOptions {
    readonly cwd?: string;
    readonly shell?: string;
    readonly onData?: (chunk: string) => void;
}

/**
 * Backend wrapper for VTCode CLI streaming and tool execution.
 * Manages streaming responses, tool calls, and PTY sessions.
 */
export class VtcodeBackend implements vscode.Disposable {
    private abortController: AbortController | undefined;
    private currentProcess: ChildProcess | undefined;
    private configSummary: VtcodeConfigSummary | undefined;
    private environmentProvider: (() => Record<string, string>) | undefined;

    constructor(
        private commandPath: string,
        private workspaceRoot: string | undefined,
        private readonly output: vscode.OutputChannel
    ) {}

    /**
     * Cleanup resources on disposal.
     */
    dispose(): void {
        this.cancelStream();
    }

    /**
     * Update configuration when settings or workspace changes.
     */
    updateConfiguration(
        commandPath: string,
        workspaceRoot: string | undefined,
        summary?: VtcodeConfigSummary
    ): void {
        this.commandPath = commandPath;
        this.workspaceRoot = workspaceRoot;
        if (summary) {
            this.configSummary = summary;
        }
    }

    setEnvironmentProvider(
        provider: (() => Record<string, string>) | undefined
    ): void {
        this.environmentProvider = provider;
    }

    /**
     * Stream a prompt to the VTCode CLI and parse the response chunks.
     * Supports cancellation via abort controller.
     */
    async *streamPrompt(
        options: StreamPromptOptions
    ): AsyncGenerator<VtcodeStreamChunk> {
        if (this.shouldUseExec()) {
            for await (const chunk of this.streamPromptViaExec(options)) {
                yield chunk;
            }
            return;
        }

        for await (const chunk of this.streamPromptViaAsk(options)) {
            yield chunk;
        }
    }

    private shouldUseExec(): boolean {
        return false;
    }

    private async *streamPromptViaExec(
        options: StreamPromptOptions
    ): AsyncGenerator<VtcodeStreamChunk> {
        this.abortController = new AbortController();
        const signal = this.abortController.signal;

        const formattedPrompt = this.buildPromptPayload(options);
        const args = ["exec", "--json", formattedPrompt];

        const spawnOptions = {
            cwd: this.workspaceRoot,
            env: this.buildSpawnEnv(),
        };

        this.output.appendLine(
            `[vtcode] Running: ${this.commandPath} ${args.join(" ")}`
        );

        this.currentProcess = spawn(this.commandPath, args, spawnOptions);
        const proc = this.currentProcess;

        if (!proc.stdout) {
            throw new Error("Failed to create process stdout stream");
        }

        const stderrChunks: string[] = [];
        let exitCode: number | null = null;
        const state: ExecStreamState = {
            textByItemId: new Map(),
            reasoningByItemId: new Map(),
            commandState: new Map(),
            toolState: new Map(),
        };
        const lineReader = createInterface({
            input: proc.stdout,
            crlfDelay: Infinity,
        });

        if (proc.stderr) {
            proc.stderr.on("data", (chunk: Buffer) => {
                const text = chunk.toString();
                stderrChunks.push(text);
                const trimmed = text.trim();
                if (trimmed) {
                    this.output.appendLine(`[vtcode][stderr] ${trimmed}`);
                }
            });
        }

        proc.on("close", (code: number | null) => {
            exitCode = code;
            if (code !== 0 && !signal.aborted) {
                const reason =
                    stderrChunks.join("").trim() ||
                    `VTCode CLI exited with code ${code ?? "unknown"}`;
                this.output.appendLine(`[vtcode] ${reason}`);
            }
        });

        proc.on("error", (error: Error) => {
            if (!signal.aborted) {
                this.output.appendLine(
                    `[vtcode] Process error: ${error.message}`
                );
            }
        });

        // Handle abort signal
        const abortHandler = () => {
            proc.kill("SIGTERM");
            this.output.appendLine("[vtcode] Stream cancelled by user");
        };
        signal.addEventListener("abort", abortHandler);

        try {
            for await (const line of lineReader) {
                if (signal.aborted) {
                    break;
                }
                const trimmed = line.trim();
                if (!trimmed) {
                    continue;
                }

                let event: ExecThreadEvent;
                try {
                    event = JSON.parse(trimmed) as ExecThreadEvent;
                } catch (parseError) {
                    const message =
                        parseError instanceof Error
                            ? parseError.message
                            : String(parseError);
                    this.output.appendLine(
                        `[vtcode] Failed to parse exec event: ${message}`
                    );
                    continue;
                }

                const chunks = this.transformExecEvent(event, state);
                for (const chunk of chunks) {
                    yield chunk;
                }
            }

            if (signal.aborted) {
                return;
            }

            if (exitCode !== null && exitCode !== 0) {
                const raw =
                    stderrChunks.join("").trim() ||
                    `VTCode CLI exited with code ${exitCode}`;
                const message = this.normalizeCliError(raw);
                yield { kind: "error", message };
                return;
            }

            yield { kind: "done" };
        } catch (error) {
            if (!signal.aborted) {
                const raw =
                    error instanceof Error ? error.message : String(error);
                const message = this.normalizeCliError(raw);
                this.output.appendLine(`[vtcode] Stream error: ${message}`);
                yield { kind: "error", message };
            }
        } finally {
            lineReader.close();
            signal.removeEventListener("abort", abortHandler);
            this.abortController = undefined;
            proc.kill();
            this.currentProcess = undefined;
        }
    }

    private async *streamPromptViaAsk(
        options: StreamPromptOptions
    ): AsyncGenerator<VtcodeStreamChunk> {
        this.abortController = new AbortController();
        const signal = this.abortController.signal;
        const formattedPrompt = this.buildPromptPayload(options);
        const args = ["ask", formattedPrompt];
        const spawnOptions = {
            cwd: this.workspaceRoot,
            env: this.buildSpawnEnv(),
        };

        this.output.appendLine(
            `[vtcode] Running: ${this.commandPath} ${args.join(" ")}`
        );

        this.currentProcess = spawn(this.commandPath, args, spawnOptions);
        const proc = this.currentProcess;

        if (!proc.stdout) {
            throw new Error("Failed to create process stdout stream");
        }

        const stderrChunks: string[] = [];
        let exitCode: number | null = null;

        if (proc.stderr) {
            proc.stderr.on("data", (chunk: Buffer) => {
                const text = chunk.toString();
                stderrChunks.push(text);
                const trimmed = text.trim();
                if (trimmed) {
                    this.output.appendLine(`[vtcode][stderr] ${trimmed}`);
                }
            });
        }

        proc.on("close", (code: number | null) => {
            exitCode = code;
            if (code !== 0 && !signal.aborted) {
                const reason =
                    stderrChunks.join("").trim() ||
                    `VTCode CLI exited with code ${code ?? "unknown"}`;
                this.output.appendLine(`[vtcode] ${reason}`);
            }
        });

        proc.on("error", (error: Error) => {
            if (!signal.aborted) {
                this.output.appendLine(
                    `[vtcode] Process error: ${error.message}`
                );
            }
        });

        const abortHandler = () => {
            proc.kill("SIGTERM");
            this.output.appendLine("[vtcode] Prompt request cancelled by user");
        };
        signal.addEventListener("abort", abortHandler);

        try {
            for await (const chunk of proc.stdout) {
                if (signal.aborted) {
                    break;
                }
                const text = chunk.toString();
                if (!text) {
                    continue;
                }
                yield { kind: "text", text };
            }

            if (signal.aborted) {
                return;
            }

            if (exitCode !== null && exitCode !== 0) {
                const raw =
                    stderrChunks.join("").trim() ||
                    `VTCode CLI exited with code ${exitCode}`;
                const message = this.normalizeCliError(raw);
                yield { kind: "error", message };
                return;
            }

            yield { kind: "done" };
        } catch (error) {
            if (!signal.aborted) {
                const raw =
                    error instanceof Error ? error.message : String(error);
                const message = this.normalizeCliError(raw);
                this.output.appendLine(`[vtcode] Prompt error: ${message}`);
                yield { kind: "error", message };
            }
        } finally {
            signal.removeEventListener("abort", abortHandler);
            this.abortController = undefined;
            proc.kill();
            this.currentProcess = undefined;
        }
    }

    private normalizeCliError(raw: string): string {
        const clean = raw
            .replace(ANSI_CSI_PATTERN, "")
            .replace(ANSI_OSC_PATTERN, "")
            .trim();
        if (!clean) {
            return "VTCode CLI reported an unknown error.";
        }

        const provider = this.configSummary?.agentProvider;
        const defaultModel = this.configSummary?.agentDefaultModel;
        const lower = clean.toLowerCase();

        if (clean.includes("API key not found for provider")) {
            const providerName = provider ?? "the configured provider";
            return `${clean}\nHint: Configure credentials for ${providerName} or update vtcode.toml to use a model available locally${
                defaultModel ? ` (current default_model: ${defaultModel})` : ""
            }.`;
        }

        if (lower.includes("unauthorized")) {
            const providerHint = provider ? ` (${provider})` : "";
            return `${clean}\nHint: Check API credentials${providerHint} or adjust vtcode.toml to a provider you can access${
                defaultModel ? ` (current default_model: ${defaultModel})` : ""
            }.`;
        }

        return clean;
    }

    private buildSpawnEnv(): NodeJS.ProcessEnv {
        const baseEnv = { ...process.env } as NodeJS.ProcessEnv;
        if (!this.environmentProvider) {
            return baseEnv;
        }

        try {
            const overlay = this.environmentProvider();
            for (const [key, value] of Object.entries(overlay)) {
                baseEnv[key] = value;
            }
        } catch (error) {
            const details =
                error instanceof Error ? error.message : String(error);
            this.output.appendLine(
                `[vtcode] Failed to resolve VTCode environment overlay: ${details}`
            );
        }

        return baseEnv;
    }

    private buildPromptPayload(options: StreamPromptOptions): string {
        if (options.context) {
            return `Conversation so far:\n${options.context}\n\nLatest user message:\n${options.prompt}`;
        }
        return options.prompt;
    }

    private transformExecEvent(
        event: ExecThreadEvent,
        state: ExecStreamState
    ): VtcodeStreamChunk[] {
        if (typeof event !== "object" || event === null) {
            return [];
        }

        const type = (event as { type?: unknown }).type;
        if (typeof type !== "string") {
            return [];
        }

        switch (type) {
            case "item.started":
            case "item.updated":
            case "item.completed":
                return isExecItemEvent(event)
                    ? this.handleExecItemEvent(
                          event,
                          state,
                          type === "item.completed"
                      )
                    : [];
            case "thread.started":
                return isExecThreadStartedEvent(event)
                    ? [
                          {
                              kind: "metadata",
                              metadata: { threadId: event.thread_id },
                          },
                      ]
                    : [];
            case "turn.started":
                return [];
            case "turn.completed":
                return isExecTurnCompletedEvent(event)
                    ? [
                          {
                              kind: "metadata",
                              metadata: {
                                  usage: this.serializeUsage(event.usage),
                              },
                          },
                      ]
                    : [];
            case "turn.failed":
                return isExecTurnFailedEvent(event)
                    ? [
                          {
                              kind: "error",
                              message: event.message,
                          },
                      ]
                    : [];
            case "error":
                return isExecErrorEvent(event)
                    ? [
                          {
                              kind: "error",
                              message: event.message,
                          },
                      ]
                    : [];
            default:
                return [];
        }
    }

    private handleExecItemEvent(
        event: ExecItemEvent,
        state: ExecStreamState,
        terminal: boolean
    ): VtcodeStreamChunk[] {
        const item = event.item;
        if (!item || typeof item !== "object") {
            return [];
        }

        switch (item.type) {
            case "agent_message":
                return this.emitIncrementalText(
                    state.textByItemId,
                    item,
                    "text"
                );
            case "reasoning":
                return this.emitIncrementalText(
                    state.reasoningByItemId,
                    item,
                    "reasoning"
                );
            case "command_execution":
                return this.emitCommandResult(
                    state.commandState,
                    item,
                    terminal
                );
            case "mcp_tool_call":
                return this.emitToolCallResult(state.toolState, item, terminal);
            case "file_change":
                return this.emitFileChangeMetadata(item);
            case "error":
                return typeof item.result === "string" &&
                    item.result.trim().length > 0
                    ? [
                          {
                              kind: "error",
                              message: item.result,
                          },
                      ]
                    : [];
            default:
                return [];
        }
    }

    private emitIncrementalText(
        cache: Map<string, string>,
        item: ExecThreadItem,
        kind: "text" | "reasoning"
    ): VtcodeStreamChunk[] {
        if (typeof item.id !== "string" || typeof item.text !== "string") {
            return [];
        }

        const previous = cache.get(item.id) ?? "";
        let delta = item.text;
        if (item.text.startsWith(previous)) {
            delta = item.text.slice(previous.length);
        }
        cache.set(item.id, item.text);
        if (!delta) {
            return [];
        }

        return [
            kind === "text"
                ? { kind: "text", text: delta }
                : { kind: "reasoning", text: delta },
        ];
    }

    private emitCommandResult(
        cache: Map<string, ExecCommandState>,
        item: ExecThreadItem,
        terminal: boolean
    ): VtcodeStreamChunk[] {
        if (typeof item.id !== "string") {
            return [];
        }

        const existing = cache.get(item.id) ?? {};
        if (typeof item.command === "string") {
            existing.command = item.command;
        }
        if (typeof item.aggregated_output === "string") {
            existing.aggregatedOutput = item.aggregated_output;
        }
        if (typeof item.exit_code === "number") {
            existing.exitCode = item.exit_code;
        }
        if (typeof item.status === "string") {
            existing.status = item.status;
        }
        cache.set(item.id, existing);

        const normalized = this.normalizeToolStatus(existing.status, terminal);
        const shouldEmit =
            terminal || normalized === "completed" || normalized === "failed";
        if (!shouldEmit || existing.emitted) {
            return [];
        }

        existing.emitted = true;
        const chunk: VtcodeToolResultChunk = {
            kind: "toolResult",
            id: item.id,
            toolType: "command",
            name: existing.command ?? "(command)",
            status: normalized,
            output: existing.aggregatedOutput,
            exitCode: existing.exitCode,
            rawEvent: { ...item } as Record<string, unknown>,
        };
        return [chunk];
    }

    private emitToolCallResult(
        cache: Map<string, ExecToolCallState>,
        item: ExecThreadItem,
        terminal: boolean
    ): VtcodeStreamChunk[] {
        if (typeof item.id !== "string") {
            return [];
        }

        const existing = cache.get(item.id) ?? {};
        if (typeof item.tool_name === "string") {
            existing.toolName = item.tool_name;
        }
        if (Object.prototype.hasOwnProperty.call(item, "arguments")) {
            existing.arguments = item.arguments;
        }
        if (typeof item.result === "string") {
            existing.result = item.result;
        }
        if (typeof item.status === "string") {
            existing.status = item.status;
        }
        cache.set(item.id, existing);

        const normalized = this.normalizeToolStatus(existing.status, terminal);
        const shouldEmit =
            terminal || normalized === "completed" || normalized === "failed";
        if (!shouldEmit || existing.emitted) {
            return [];
        }

        existing.emitted = true;
        const chunk: VtcodeToolResultChunk = {
            kind: "toolResult",
            id: item.id,
            toolType: "mcp",
            name: existing.toolName ?? "MCP Tool",
            status: normalized,
            output: existing.result,
            arguments: existing.arguments,
            rawEvent: { ...item } as Record<string, unknown>,
        };
        return [chunk];
    }

    private emitFileChangeMetadata(item: ExecThreadItem): VtcodeStreamChunk[] {
        if (!Array.isArray(item.changes) || item.changes.length === 0) {
            return [];
        }

        const changes = item.changes.map((change) => ({
            path: change.path,
            kind: change.kind,
        }));
        return [
            {
                kind: "metadata",
                metadata: { fileChanges: changes },
            },
        ];
    }

    private normalizeToolStatus(
        status: string | undefined,
        terminal: boolean
    ): VtcodeToolResultChunk["status"] {
        if (!status) {
            return terminal ? "completed" : "in_progress";
        }

        switch (status.toLowerCase()) {
            case "started":
                return "started";
            case "inprogress":
            case "in_progress":
            case "in-progress":
                return "in_progress";
            case "completed":
            case "complete":
                return "completed";
            case "failed":
            case "error":
                return "failed";
            default:
                return terminal ? "completed" : "in_progress";
        }
    }

    private serializeUsage(usage: ExecUsage): Record<string, number> {
        return {
            inputTokens: usage.input_tokens,
            cachedInputTokens: usage.cached_input_tokens,
            outputTokens: usage.output_tokens,
        };
    }

    /**
     * Cancel the current streaming operation.
     */
    cancelStream(): void {
        if (this.abortController) {
            this.abortController.abort();
        }
        if (this.currentProcess) {
            this.currentProcess.kill("SIGTERM");
            this.currentProcess = undefined;
        }
    }

    /**
     * Parse a stream chunk from the CLI JSON output.
     */
    private parseStreamChunk(data: unknown): VtcodeStreamChunk | null {
        if (typeof data !== "object" || data === null) {
            return null;
        }

        const obj = data as Record<string, unknown>;
        const kind = obj.kind;

        if (kind === "text" && typeof obj.text === "string") {
            return { kind: "text", text: obj.text };
        }

        if (kind === "reasoning" && typeof obj.text === "string") {
            return { kind: "reasoning", text: obj.text };
        }

        if (kind === "metadata" && typeof obj.metadata === "object") {
            return {
                kind: "metadata",
                metadata: obj.metadata as Record<string, unknown>,
            };
        }

        if (kind === "toolCall" && typeof obj.call === "object") {
            const call = obj.call as VtcodeToolCall;
            return {
                kind: "toolCall",
                call,
                respond: (result: unknown) => {
                    // Send tool result back to CLI via stdin
                    this.sendToolResult(call.id, result);
                },
                reject: (error: string) => {
                    // Send tool error back to CLI
                    this.sendToolError(call.id, error);
                },
            };
        }

        if (kind === "error" && typeof obj.message === "string") {
            return { kind: "error", message: obj.message };
        }

        if (kind === "done") {
            return { kind: "done" };
        }

        return null;
    }

    /**
     * Send tool result back to CLI (placeholder - needs implementation).
     */
    private sendToolResult(callId: string, result: unknown): void {
        this.output.appendLine(
            `[vtcode] Tool result for ${callId}: ${JSON.stringify(result)}`
        );
        // TODO: Implement sending result back to CLI stdin
    }

    /**
     * Send tool error back to CLI (placeholder - needs implementation).
     */
    private sendToolError(callId: string, error: string): void {
        this.output.appendLine(`[vtcode] Tool error for ${callId}: ${error}`);
        // TODO: Implement sending error back to CLI stdin
    }

    /**
     * Execute a tool call (non-terminal tools).
     */
    async executeTool(
        call: VtcodeToolCall
    ): Promise<VtcodeToolExecutionResult> {
        this.output.appendLine(`[vtcode] Executing tool: ${call.name}`);

        // For now, we'll use the CLI to execute tools
        const args = ["execute-tool", call.name, JSON.stringify(call.args)];
        const spawnOptions = {
            cwd: this.workspaceRoot,
            env: this.buildSpawnEnv(),
        };

        return new Promise((resolve, reject) => {
            let stdout = "";
            let stderr = "";

            const proc = require("child_process").spawn(
                this.commandPath,
                args,
                spawnOptions
            );

            proc.stdout.on("data", (chunk: Buffer) => {
                stdout += chunk.toString();
            });

            proc.stderr.on("data", (chunk: Buffer) => {
                stderr += chunk.toString();
            });

            proc.on("close", (code: number) => {
                if (code === 0) {
                    resolve({
                        text: stdout,
                        exitCode: code,
                    });
                } else {
                    reject(
                        new Error(
                            `Tool ${call.name} failed with exit code ${code}: ${stderr}`
                        )
                    );
                }
            });

            proc.on("error", (error: Error) => {
                reject(error);
            });
        });
    }

    /**
     * Run a command in a PTY session with streaming output.
     */
    async runPtyCommand(
        command: string,
        options: PtyCommandOptions = {}
    ): Promise<VtcodeToolExecutionResult> {
        const cwd = options.cwd || this.workspaceRoot || process.cwd();
        const shell = options.shell || this.getDefaultShell();

        this.output.appendLine(`[vtcode] Running PTY command: ${command}`);

        return new Promise((resolve, reject) => {
            let output = "";
            let commandStarted = false;
            const timeout = 30000; // 30 second timeout

            const ptyProcess = pty.spawn(shell, [], {
                name: "xterm-256color",
                cols: 120,
                rows: 30,
                cwd,
                env: process.env as Record<string, string>,
            });

            // Set a timeout to prevent hanging
            const timer = setTimeout(() => {
                ptyProcess.kill();
                reject(new Error(`Command timed out after ${timeout}ms`));
            }, timeout);

            ptyProcess.onData((data) => {
                output += data;
                if (options.onData) {
                    options.onData(data);
                }
            });

            ptyProcess.onExit(({ exitCode }) => {
                clearTimeout(timer);
                this.output.appendLine(
                    `[vtcode] PTY command exited with code ${exitCode}`
                );
                resolve({
                    text: output,
                    exitCode,
                });
            });

            // Write the command and press enter
            ptyProcess.write(`${command}\r`);
            commandStarted = true;

            // Wait a moment for command to start, then send exit
            // This gives the command time to complete before we exit the shell
            setTimeout(() => {
                if (commandStarted) {
                    ptyProcess.write("exit\r");
                }
            }, 200);
        });
    }

    /**
     * Get the default shell for the current platform.
     */
    private getDefaultShell(): string {
        if (process.platform === "win32") {
            return process.env.COMSPEC || "cmd.exe";
        }
        return process.env.SHELL || "/bin/bash";
    }
}
