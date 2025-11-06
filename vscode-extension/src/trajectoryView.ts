import * as vscode from "vscode";
import type { VtcodeTerminalOutputEvent } from "./agentTerminal";

interface RouteRecord {
    readonly kind: "route";
    readonly turn: number;
    readonly ts?: number;
    readonly selected_model: string;
    readonly class: string;
    readonly input_preview: string;
}

interface ToolRecord {
    readonly kind: "tool";
    readonly turn: number;
    readonly ts?: number;
    readonly name: string;
    readonly ok?: boolean;
    readonly args?: unknown;
}

type TrajectoryRecord = RouteRecord | ToolRecord;

interface TrajectoryTurn {
    readonly turn: number;
    readonly route?: RouteRecord;
    readonly tools: ToolRecord[];
    readonly timestamp?: number;
}

const MAX_LOG_LINES = 2000;
const MAX_TURNS = 50;
const decoder = new TextDecoder("utf-8");

class TrajectoryTreeDataProvider
    implements vscode.TreeDataProvider<TrajectoryTreeItem>
{
    private readonly onDidChangeEmitter = new vscode.EventEmitter<void>();
    private turns: TrajectoryTurn[] = [];
    private placeholderMessage: string | undefined;

    readonly onDidChangeTreeData = this.onDidChangeEmitter.event;

    update(turns: TrajectoryTurn[], placeholder?: string): void {
        this.turns = turns;
        this.placeholderMessage = placeholder;
        this.onDidChangeEmitter.fire();
    }

    getTreeItem(element: TrajectoryTreeItem): vscode.TreeItem {
        return element;
    }

    getChildren(
        element?: TrajectoryTreeItem
    ): vscode.ProviderResult<TrajectoryTreeItem[]> {
        if (!element) {
            if (this.turns.length === 0) {
                return [
                    new PlaceholderTreeItem(
                        this.placeholderMessage ??
                            "Run the VTCode agent to capture trajectory logs."
                    ),
                ];
            }

            return this.turns.map((turn) => new TurnTreeItem(turn));
        }

        if (element instanceof TurnTreeItem) {
            const items: TrajectoryTreeItem[] = [];
            if (element.turn.route) {
                items.push(new RouteTreeItem(element.turn.route));
            }

            for (const tool of element.turn.tools) {
                items.push(new ToolTreeItem(tool));
            }

            if (items.length === 0) {
                items.push(
                    new PlaceholderTreeItem(
                        "No tool activity captured for this turn yet."
                    )
                );
            }

            return items;
        }

        return [];
    }
}

abstract class TrajectoryTreeItem extends vscode.TreeItem {}

class PlaceholderTreeItem extends TrajectoryTreeItem {
    constructor(message: string) {
        super(message, vscode.TreeItemCollapsibleState.None);
        this.iconPath = new vscode.ThemeIcon("info");
        this.contextValue = "vtcodeTrajectoryPlaceholder";
    }
}

class TurnTreeItem extends TrajectoryTreeItem {
    constructor(public readonly turn: TrajectoryTurn) {
        super(
            `Turn ${turn.turn}`,
            vscode.TreeItemCollapsibleState.Collapsed
        );

        const descriptionParts: string[] = [];
        if (turn.route) {
            descriptionParts.push(turn.route.selected_model);
            descriptionParts.push(formatClassLabel(turn.route.class));
        }

        if (turn.timestamp) {
            const formattedTime = formatTimestamp(turn.timestamp);
            if (formattedTime) {
                descriptionParts.push(formattedTime);
            }
        }

        this.description = descriptionParts.join(" • ") || undefined;
        this.iconPath = new vscode.ThemeIcon("history");
        this.tooltip = buildTurnTooltip(turn);
        this.contextValue = "vtcodeTrajectoryTurn";
    }
}

class RouteTreeItem extends TrajectoryTreeItem {
    constructor(route: RouteRecord) {
        super(
            `Route → ${route.selected_model}`,
            vscode.TreeItemCollapsibleState.None
        );

        const classLabel = formatClassLabel(route.class);
        const descriptionParts = [classLabel];
        const timestamp = formatTimestamp(route.ts);
        if (timestamp) {
            descriptionParts.push(timestamp);
        }

        this.description = descriptionParts.join(" • ") || undefined;
        this.iconPath = new vscode.ThemeIcon("debug-step-over");
        this.tooltip = buildRouteTooltip(route);
        this.contextValue = "vtcodeTrajectoryRoute";
    }
}

class ToolTreeItem extends TrajectoryTreeItem {
    constructor(tool: ToolRecord) {
        super(`Tool: ${tool.name}`, vscode.TreeItemCollapsibleState.None);

        const statusLabel = tool.ok === false
            ? "Failed"
            : tool.ok === true
            ? "Completed"
            : "Unknown";
        const argsPreview = formatArgsPreview(tool.args);
        const descriptionParts = [statusLabel];
        if (argsPreview) {
            descriptionParts.push(argsPreview);
        }

        const timestamp = formatTimestamp(tool.ts);
        if (timestamp) {
            descriptionParts.push(timestamp);
        }

        this.description = descriptionParts.join(" • ") || undefined;
        this.iconPath = new vscode.ThemeIcon(
            tool.ok === false ? "error" : "check"
        );
        this.tooltip = buildToolTooltip(tool);
        this.contextValue = "vtcodeTrajectoryTool";
    }
}

interface TrajectoryViewOptions {
    readonly onAgentOutput?: vscode.Event<VtcodeTerminalOutputEvent>;
}

export class TrajectoryViewController implements vscode.Disposable {
    private readonly provider = new TrajectoryTreeDataProvider();
    private watcher: vscode.FileSystemWatcher | undefined;
    private watcherDisposables: vscode.Disposable[] = [];
    private refreshHandle: NodeJS.Timeout | undefined;
    private refreshDelayMs: number;

    constructor(
        private readonly context: vscode.ExtensionContext,
        options: TrajectoryViewOptions = {}
    ) {
        this.refreshDelayMs = this.resolveRefreshDelay();
        context.subscriptions.push(
            vscode.window.registerTreeDataProvider(
                "vtcodeAgentLoopView",
                this.provider
            )
        );
        context.subscriptions.push(
            vscode.commands.registerCommand(
                "vtcode.refreshAgentTimeline",
                () => this.refreshNow()
            )
        );
        context.subscriptions.push(
            vscode.commands.registerCommand(
                "vtcode.openAgentTrajectoryLog",
                () => this.openTrajectoryLog()
            )
        );
        context.subscriptions.push(this);

        context.subscriptions.push(
            vscode.workspace.onDidChangeConfiguration((event) => {
                if (event.affectsConfiguration("vtcode.agentTimeline.refreshDebounceMs")) {
                    this.refreshDelayMs = this.resolveRefreshDelay();
                }
            })
        );

        if (options.onAgentOutput) {
            context.subscriptions.push(
                options.onAgentOutput(() => this.scheduleRefresh())
            );
        }

        void this.initialize();
    }

    dispose(): void {
        this.disposeWatcher();
        if (this.refreshHandle) {
            clearTimeout(this.refreshHandle);
            this.refreshHandle = undefined;
        }
    }

    handleWorkspaceFoldersChanged(): void {
        this.registerWatcher();
        void this.refreshNow();
    }

    async refreshNow(): Promise<void> {
        await this.refresh();
    }

    private async initialize(): Promise<void> {
        this.registerWatcher();
        await this.refresh();
    }

    private registerWatcher(): void {
        this.disposeWatcher();

        const folder = this.getPrimaryWorkspaceFolder();
        if (!folder) {
            this.provider.update(
                [],
                "Open a workspace folder to inspect VTCode agent telemetry."
            );
            return;
        }

        const pattern = new vscode.RelativePattern(
            folder,
            ".vtcode/logs/trajectory.jsonl"
        );
        const watcher = vscode.workspace.createFileSystemWatcher(pattern);
        this.watcher = watcher;
        this.watcherDisposables.push(
            watcher.onDidCreate(() => this.scheduleRefresh()),
            watcher.onDidChange(() => this.scheduleRefresh()),
            watcher.onDidDelete(() => this.scheduleRefresh())
        );
    }

    private disposeWatcher(): void {
        for (const disposable of this.watcherDisposables) {
            disposable.dispose();
        }
        this.watcherDisposables = [];
        this.watcher?.dispose();
        this.watcher = undefined;
    }

    private scheduleRefresh(): void {
        if (this.refreshHandle) {
            clearTimeout(this.refreshHandle);
        }
        const delay = this.refreshDelayMs;
        this.refreshHandle = setTimeout(() => {
            this.refreshHandle = undefined;
            void this.refresh();
        }, delay);
    }

    private async refresh(): Promise<void> {
        const logUri = this.getTrajectoryLogUri();
        if (!logUri) {
            this.provider.update(
                [],
                "Open a workspace folder to inspect VTCode agent telemetry."
            );
            return;
        }

        try {
            await vscode.workspace.fs.stat(logUri);
        } catch (error) {
            if (error instanceof vscode.FileSystemError && error.code === "FileNotFound") {
                this.provider.update(
                    [],
                    "No trajectory logs detected. Run a VTCode chat session to populate this view."
                );
                return;
            }

            const message = error instanceof Error ? error.message : String(error);
            this.provider.update(
                [],
                `Failed to read trajectory logs: ${message}`
            );
            return;
        }

        try {
            const contents = await vscode.workspace.fs.readFile(logUri);
            const text = decoder.decode(contents);
            const turns = parseTrajectoryLog(text);
            const placeholder =
                turns.length === 0
                    ? "Trajectory log is empty. Run the VTCode agent to capture activity."
                    : undefined;
            this.provider.update(turns, placeholder);
        } catch (error) {
            const message = error instanceof Error ? error.message : String(error);
            this.provider.update(
                [],
                `Failed to parse trajectory logs: ${message}`
            );
        }
    }

    private async openTrajectoryLog(): Promise<void> {
        const logUri = this.getTrajectoryLogUri();
        if (!logUri) {
            void vscode.window.showInformationMessage(
                "Open a workspace folder to view trajectory logs."
            );
            return;
        }

        try {
            await vscode.workspace.fs.stat(logUri);
        } catch (error) {
            if (error instanceof vscode.FileSystemError && error.code === "FileNotFound") {
                void vscode.window.showInformationMessage(
                    "VTCode has not written a trajectory log yet. Run a chat session to generate telemetry."
                );
                return;
            }

            const message = error instanceof Error ? error.message : String(error);
            void vscode.window.showErrorMessage(
                `Failed to open trajectory log: ${message}`
            );
            return;
        }

        const document = await vscode.workspace.openTextDocument(logUri);
        await vscode.window.showTextDocument(document, { preview: false });
    }

    private getTrajectoryLogUri(): vscode.Uri | undefined {
        const folder = this.getPrimaryWorkspaceFolder();
        if (!folder) {
            return undefined;
        }
        return vscode.Uri.joinPath(
            folder.uri,
            ".vtcode",
            "logs",
            "trajectory.jsonl"
        );
    }

    private resolveRefreshDelay(): number {
        const configured = vscode.workspace
            .getConfiguration("vtcode")
            .get<number>("agentTimeline.refreshDebounceMs", 250);
        if (!Number.isFinite(configured)) {
            return 250;
        }

        const clamped = Math.max(50, Math.min(5000, Math.trunc(configured)));
        return clamped;
    }

    private getPrimaryWorkspaceFolder(): vscode.WorkspaceFolder | undefined {
        const activeEditor = vscode.window.activeTextEditor;
        if (activeEditor) {
            const folder = vscode.workspace.getWorkspaceFolder(
                activeEditor.document.uri
            );
            if (folder) {
                return folder;
            }
        }

        const [first] = vscode.workspace.workspaceFolders ?? [];
        return first;
    }
}

export function registerTrajectoryView(
    context: vscode.ExtensionContext,
    options: TrajectoryViewOptions = {}
): TrajectoryViewController {
    return new TrajectoryViewController(context, options);
}

function parseTrajectoryLog(text: string): TrajectoryTurn[] {
    const lines = text
        .split(/\r?\n/)
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
    const recentLines = lines.slice(-MAX_LOG_LINES);

    const turnOrder: TrajectoryTurn[] = [];
    const turnMap = new Map<number, TrajectoryTurn>();

    for (const line of recentLines) {
        const record = parseTrajectoryLine(line);
        if (!record) {
            continue;
        }

        const turnNumber = record.turn;
        let turn = turnMap.get(turnNumber);
        if (!turn) {
            turn = { turn: turnNumber, tools: [] };
            turnMap.set(turnNumber, turn);
            turnOrder.push(turn);
        }

        if (record.ts) {
            turn.timestamp = Math.min(
                turn.timestamp ?? Number.POSITIVE_INFINITY,
                record.ts
            );
        }

        if (record.kind === "route") {
            turn.route = record;
        } else {
            turn.tools.push(record);
        }
    }

    for (const turn of turnOrder) {
        turn.tools.sort((a, b) => (a.ts ?? 0) - (b.ts ?? 0));
    }

    return turnOrder
        .sort((a, b) => b.turn - a.turn)
        .slice(0, MAX_TURNS);
}

function parseTrajectoryLine(line: string): TrajectoryRecord | undefined {
    let payload: unknown;
    try {
        payload = JSON.parse(line);
    } catch (error) {
        console.warn("Failed to parse trajectory line", error);
        return undefined;
    }

    if (!payload || typeof payload !== "object") {
        return undefined;
    }

    const record = payload as Record<string, unknown>;
    const kind = typeof record.kind === "string" ? record.kind : undefined;
    const turnValue = record.turn;
    const turn = typeof turnValue === "number" ? turnValue : Number(turnValue);
    if (!Number.isFinite(turn)) {
        return undefined;
    }

    const tsValue = record.ts;
    const ts =
        typeof tsValue === "number"
            ? tsValue
            : typeof tsValue === "string"
            ? Number(tsValue)
            : undefined;

    if (kind === "route") {
        const selectedModel =
            typeof record.selected_model === "string"
                ? record.selected_model
                : undefined;
        const classValue =
            typeof record.class === "string" ? record.class : undefined;
        const inputPreview =
            typeof record.input_preview === "string"
                ? record.input_preview
                : "";

        if (!selectedModel || !classValue) {
            return undefined;
        }

        return {
            kind: "route",
            turn,
            ts,
            selected_model: selectedModel,
            class: classValue,
            input_preview: inputPreview,
        } satisfies RouteRecord;
    }

    if (kind === "tool") {
        const name = typeof record.name === "string" ? record.name : undefined;
        if (!name) {
            return undefined;
        }

        const ok =
            typeof record.ok === "boolean" ? record.ok : undefined;
        const args = Object.prototype.hasOwnProperty.call(record, "args")
            ? (record as { args?: unknown }).args
            : undefined;

        return {
            kind: "tool",
            turn,
            ts,
            name,
            ok,
            args,
        } satisfies ToolRecord;
    }

    return undefined;
}

function formatClassLabel(value: string): string {
    return value
        .split("_")
        .map((part) =>
            part.length > 0
                ? part.charAt(0).toUpperCase() + part.slice(1)
                : part
        )
        .join(" ");
}

function formatTimestamp(timestamp?: number): string | undefined {
    if (typeof timestamp !== "number" || !Number.isFinite(timestamp)) {
        return undefined;
    }

    const millis = timestamp > 1e12 ? timestamp : timestamp * 1000;
    const date = new Date(millis);
    if (Number.isNaN(date.getTime())) {
        return undefined;
    }

    return date.toLocaleString();
}

function formatArgsPreview(args: unknown): string | undefined {
    if (args === undefined) {
        return undefined;
    }

    if (typeof args === "string") {
        return truncateMiddle(args, 60);
    }

    try {
        const serialized = JSON.stringify(args);
        return serialized ? truncateMiddle(serialized, 60) : undefined;
    } catch (error) {
        console.warn("Failed to stringify trajectory args", error);
        return undefined;
    }
}

function truncateMiddle(value: string, maxLength: number): string {
    if (value.length <= maxLength) {
        return value;
    }

    const half = Math.floor((maxLength - 1) / 2);
    return `${value.slice(0, half)}…${value.slice(-half)}`;
}

function buildTurnTooltip(turn: TrajectoryTurn): vscode.MarkdownString {
    const tooltip = new vscode.MarkdownString(undefined, true);
    tooltip.appendMarkdown(`**Turn:** ${turn.turn}`);
    if (turn.route) {
        tooltip.appendMarkdown(
            `\n\n**Model:** ${turn.route.selected_model}`
        );
        tooltip.appendMarkdown(
            `\n\n**Class:** ${formatClassLabel(turn.route.class)}`
        );
    }

    const timestamp = formatTimestamp(turn.timestamp);
    if (timestamp) {
        tooltip.appendMarkdown(`\n\n**Timestamp:** ${timestamp}`);
    }

    return tooltip;
}

function buildRouteTooltip(route: RouteRecord): vscode.MarkdownString {
    const tooltip = new vscode.MarkdownString(undefined, true);
    tooltip.appendMarkdown(`**Model:** ${route.selected_model}`);
    tooltip.appendMarkdown(
        `\n\n**Class:** ${formatClassLabel(route.class)}`
    );

    const timestamp = formatTimestamp(route.ts);
    if (timestamp) {
        tooltip.appendMarkdown(`\n\n**Timestamp:** ${timestamp}`);
    }

    if (route.input_preview.trim().length > 0) {
        tooltip.appendMarkdown("\n\n**Input Preview**\n");
        tooltip.appendCodeblock(route.input_preview.trim());
    }

    return tooltip;
}

function buildToolTooltip(tool: ToolRecord): vscode.MarkdownString {
    const tooltip = new vscode.MarkdownString(undefined, true);
    tooltip.appendMarkdown(`**Tool:** ${tool.name}`);

    const statusLabel = tool.ok === false
        ? "Failed"
        : tool.ok === true
        ? "Completed"
        : "Unknown";
    tooltip.appendMarkdown(`\n\n**Status:** ${statusLabel}`);

    const timestamp = formatTimestamp(tool.ts);
    if (timestamp) {
        tooltip.appendMarkdown(`\n\n**Timestamp:** ${timestamp}`);
    }

    if (tool.args !== undefined) {
        tooltip.appendMarkdown("\n\n**Arguments**\n");
        const serialized = safeStringify(tool.args, 2);
        if (serialized) {
            tooltip.appendCodeblock(serialized);
        }
    }

    return tooltip;
}

function safeStringify(value: unknown, space?: number): string | undefined {
    try {
        return JSON.stringify(value, null, space ?? 0);
    } catch (error) {
        console.warn("Failed to stringify value", error);
        return undefined;
    }
}
