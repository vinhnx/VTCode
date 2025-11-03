import * as vscode from "vscode";
import * as nodePty from "node-pty";

export interface VtcodeTerminalLaunchOptions {
    readonly id: string;
    readonly title: string;
    readonly commandPath: string;
    readonly args: readonly string[];
    readonly cwd: string;
    readonly env: NodeJS.ProcessEnv;
    readonly icon?: vscode.ThemeIcon;
    readonly message?: string;
}

export interface VtcodeTerminalOutputEvent {
    readonly terminalId: string;
    readonly data: string;
}

export interface VtcodeTerminalExitEvent {
    readonly terminalId: string;
    readonly code?: number;
    readonly signal?: number;
    readonly errorMessage?: string;
}

export interface VtcodeTerminalHandle extends vscode.Disposable {
    readonly id: string;
    readonly terminal: vscode.Terminal;
    isDisposed(): boolean;
    isRunning(): boolean;
    show(preserveFocus?: boolean): void;
    sendText(text: string, options?: { addNewLine?: boolean }): void;
}

export class VtcodeTerminalManager implements vscode.Disposable {
    private readonly terminals = new Map<string, ManagedTerminal>();
    private readonly outputEmitter = new vscode.EventEmitter<VtcodeTerminalOutputEvent>();
    private readonly exitEmitter = new vscode.EventEmitter<VtcodeTerminalExitEvent>();

    readonly onDidReceiveOutput = this.outputEmitter.event;
    readonly onDidExit = this.exitEmitter.event;

    constructor(private readonly context: vscode.ExtensionContext) {}

    dispose(): void {
        for (const terminal of this.terminals.values()) {
            terminal.dispose();
        }
        this.terminals.clear();
        this.outputEmitter.dispose();
        this.exitEmitter.dispose();
    }

    createOrShowTerminal(
        options: VtcodeTerminalLaunchOptions
    ): { handle: VtcodeTerminalHandle; created: boolean } {
        const existing = this.terminals.get(options.id);
        if (existing && !existing.isDisposed()) {
            existing.show(true);
            return { handle: existing, created: false };
        }

        const managed = new ManagedTerminal(
            options,
            (event) => this.outputEmitter.fire(event),
            (event) => this.exitEmitter.fire(event),
            () => this.terminals.delete(options.id)
        );

        const terminal = vscode.window.createTerminal({
            name: options.title,
            iconPath: options.icon ?? new vscode.ThemeIcon("comment-discussion"),
            pty: managed,
            message: options.message,
        });

        managed.attach(terminal);
        this.context.subscriptions.push(managed);
        this.terminals.set(options.id, managed);

        terminal.show(true);
        return { handle: managed, created: true };
    }

    sendText(
        terminalId: string,
        text: string,
        options: { addNewLine?: boolean } = {}
    ): boolean {
        const terminal = this.terminals.get(terminalId);
        if (!terminal || terminal.isDisposed()) {
            return false;
        }

        terminal.sendText(text, options);
        return true;
    }

    getTerminalHandle(terminalId: string): VtcodeTerminalHandle | undefined {
        const terminal = this.terminals.get(terminalId);
        if (!terminal || terminal.isDisposed()) {
            return undefined;
        }

        return terminal;
    }
}

class ManagedTerminal
    implements vscode.Pseudoterminal, VtcodeTerminalHandle
{
    private readonly writeEmitter = new vscode.EventEmitter<string>();
    private readonly closeEmitter = new vscode.EventEmitter<number | void>();
    private readonly disposables: vscode.Disposable[] = [];
    private readonly pendingInput: string[] = [];

    private attachedTerminal: vscode.Terminal | undefined;
    private ptyProcess: nodePty.IPty | undefined;
    private disposed = false;
    private dimensions: vscode.TerminalDimensions | undefined;

    readonly onDidWrite = this.writeEmitter.event;
    readonly onDidClose = this.closeEmitter.event;

    constructor(
        private readonly options: VtcodeTerminalLaunchOptions,
        private readonly emitOutput: (event: VtcodeTerminalOutputEvent) => void,
        private readonly emitExit: (event: VtcodeTerminalExitEvent) => void,
        private readonly onDisposed: () => void
    ) {}

    get id(): string {
        return this.options.id;
    }

    get terminal(): vscode.Terminal {
        if (!this.attachedTerminal) {
            throw new Error("VTCode terminal has not been attached yet.");
        }

        return this.attachedTerminal;
    }

    attach(terminal: vscode.Terminal): void {
        this.attachedTerminal = terminal;
    }

    show(preserveFocus?: boolean): void {
        this.attachedTerminal?.show(preserveFocus);
    }

    isDisposed(): boolean {
        return this.disposed;
    }

    isRunning(): boolean {
        return Boolean(this.ptyProcess);
    }

    open(initialDimensions?: vscode.TerminalDimensions): void {
        this.dimensions = initialDimensions;
        this.launchProcess();
    }

    close(): void {
        this.dispose();
    }

    setDimensions(dimensions: vscode.TerminalDimensions): void {
        this.dimensions = dimensions;
        if (this.ptyProcess) {
            try {
                this.ptyProcess.resize(dimensions.columns, dimensions.rows);
            } catch (error) {
                // Ignore resize errors from terminals that do not support it.
            }
        }
    }

    handleInput(data: string): void {
        if (this.disposed) {
            return;
        }

        if (this.ptyProcess) {
            this.ptyProcess.write(data);
        } else {
            this.pendingInput.push(data);
        }
    }

    sendText(text: string, options: { addNewLine?: boolean } = {}): void {
        if (this.disposed) {
            return;
        }

        const addNewLine = options.addNewLine ?? true;
        const payload = addNewLine ? `${text}\r` : text;
        if (this.ptyProcess) {
            this.ptyProcess.write(payload);
        } else {
            this.pendingInput.push(payload);
        }
    }

    dispose(): void {
        if (this.disposed) {
            return;
        }

        this.disposed = true;
        this.onDisposed();

        try {
            this.ptyProcess?.kill();
        } catch {
            // Ignore errors when killing the PTY, e.g. if it already exited.
        }

        for (const disposable of this.disposables) {
            disposable.dispose();
        }

        this.writeEmitter.dispose();
        this.closeEmitter.dispose();
    }

    private launchProcess(): void {
        if (this.ptyProcess || this.disposed) {
            return;
        }

        const columns = this.dimensions?.columns ?? 80;
        const rows = this.dimensions?.rows ?? 30;

        this.writeEmitter.fire(
            `\u001b[90m[vtcode] Launching ${this.options.commandPath} ${
                this.options.args.join(" ")
            }\u001b[0m\r\n`
        );

        let process: nodePty.IPty;
        try {
            process = nodePty.spawn(this.options.commandPath, [...this.options.args], {
                name: "xterm-color",
                cwd: this.options.cwd,
                env: this.options.env,
                cols: columns,
                rows: rows,
            });
        } catch (error) {
            const message =
                error instanceof Error ? error.message : String(error);
            this.writeEmitter.fire(
                `\u001b[31m[vtcode] Failed to launch terminal: ${message}\u001b[0m\r\n`
            );
            this.emitExit({
                terminalId: this.options.id,
                errorMessage: message,
            });
            this.closeEmitter.fire();
            this.dispose();
            return;
        }

        this.ptyProcess = process;
        const dataDisposable = process.onData((data) => {
            const text = normalizeLineEndings(data);
            this.writeEmitter.fire(text);
            this.emitOutput({ terminalId: this.options.id, data });
        });
        const exitDisposable = process.onExit((event) => {
            this.emitExit({
                terminalId: this.options.id,
                code: event.exitCode,
                signal: event.signal,
            });
            this.closeEmitter.fire(event.exitCode);
            this.dispose();
        });

        this.disposables.push(dataDisposable);
        this.disposables.push(exitDisposable);

        while (this.pendingInput.length > 0) {
            process.write(this.pendingInput.shift() ?? "");
        }
    }
}

function normalizeLineEndings(value: string): string {
    return value.replace(/\r?\n/g, "\r\n");
}
