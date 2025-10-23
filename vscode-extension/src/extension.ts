import * as vscode from 'vscode';
import { spawn } from 'node:child_process';
import { registerVtcodeLanguageFeatures } from './languageFeatures';

type QuickActionItem = vscode.QuickPickItem & { run: () => Thenable<unknown> | void };

interface QuickActionDescription {
    readonly label: string;
    readonly description: string;
    readonly command: string;
    readonly icon?: string;
    readonly args?: unknown[];
}

class QuickActionTreeItem extends vscode.TreeItem {
    constructor(public readonly action: QuickActionDescription) {
        super(action.label, vscode.TreeItemCollapsibleState.None);
        this.description = action.description;
        this.iconPath = new vscode.ThemeIcon(action.icon ?? 'rocket');
        this.command = {
            command: action.command,
            title: action.label,
            arguments: action.args
        };
        this.contextValue = 'vtcodeQuickAction';
    }
}

class QuickActionTreeDataProvider implements vscode.TreeDataProvider<QuickActionTreeItem> {
    private readonly onDidChangeTreeDataEmitter = new vscode.EventEmitter<void>();
    readonly onDidChangeTreeData = this.onDidChangeTreeDataEmitter.event;

    constructor(private readonly getActions: () => QuickActionDescription[]) {}

    getTreeItem(element: QuickActionTreeItem): vscode.TreeItem {
        return element;
    }

    getChildren(): vscode.ProviderResult<QuickActionTreeItem[]> {
        return this.getActions().map((action) => new QuickActionTreeItem(action));
    }

    refresh(): void {
        this.onDidChangeTreeDataEmitter.fire();
    }
}

let outputChannel: vscode.OutputChannel | undefined;
let statusBarItem: vscode.StatusBarItem | undefined;
let quickActionsProviderInstance: QuickActionTreeDataProvider | undefined;
let agentTerminal: vscode.Terminal | undefined;
let terminalCloseListener: vscode.Disposable | undefined;
let cliAvailable = false;
let missingCliWarningShown = false;
let cliAvailabilityCheck: Promise<void> | undefined;

const CLI_DETECTION_TIMEOUT_MS = 4000;

export function activate(context: vscode.ExtensionContext) {
    outputChannel = vscode.window.createOutputChannel('VTCode');
    context.subscriptions.push(outputChannel);

    ensureStableApi(context);
    logExtensionHostContext(context);
    registerVtcodeLanguageFeatures(context);

    statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    statusBarItem.name = 'VTCode Quick Actions';
    statusBarItem.accessibilityInformation = { role: 'button', label: 'Open VTCode quick actions or installation guide' };
    context.subscriptions.push(statusBarItem);

    quickActionsProviderInstance = new QuickActionTreeDataProvider(() => createQuickActions(cliAvailable));
    context.subscriptions.push(vscode.window.registerTreeDataProvider('vtcodeQuickActionsView', quickActionsProviderInstance));

    setStatusBarChecking(getConfiguredCommandPath());

    if (vscode.env.uiKind === vscode.UIKind.Web) {
        void vscode.window.showWarningMessage(
            'VTCode Companion is running in VS Code for the Web. Command execution features are disabled, but documentation and configuration helpers remain available.'
        );
    }

    const quickActionsCommand = vscode.commands.registerCommand('vtcode.openQuickActions', async () => {
        const actions = createQuickActions(cliAvailable);
        const pickItems: QuickActionItem[] = actions.map((action) => ({
            label: `${action.icon ? `$(${action.icon}) ` : ''}${action.label}`,
            description: action.description,
            run: () => vscode.commands.executeCommand(action.command, ...(action.args ?? []))
        }));

        const selection = await vscode.window.showQuickPick(pickItems, {
            placeHolder: 'Choose a VTCode action to run'
        });

        if (selection) {
            await selection.run();
        }
    });

    const askAgent = vscode.commands.registerCommand('vtcode.askAgent', async () => {
        if (!(await ensureCliAvailableForCommand())) {
            return;
        }

        const question = await vscode.window.showInputBox({
            prompt: 'What would you like the VTCode agent to help with?',
            placeHolder: 'Summarize src/main.rs',
            ignoreFocusOut: true
        });

        if (!question || !question.trim()) {
            return;
        }

        try {
            await runVtcodeCommand(['ask', question], { title: 'Asking VTCode…' });
            void vscode.window.showInformationMessage('VTCode finished processing your request. Check the VTCode output channel for details.');
        } catch (error) {
            handleCommandError('ask', error);
        }
    });

    const askSelection = vscode.commands.registerCommand('vtcode.askSelection', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            void vscode.window.showWarningMessage('Open a text editor to ask VTCode about the current selection.');
            return;
        }

        const selection = editor.selection;
        if (selection.isEmpty) {
            void vscode.window.showWarningMessage('Highlight text first, then run “Ask About Selection with VTCode.”');
            return;
        }

        const selectedText = editor.document.getText(selection);
        if (!selectedText.trim()) {
            void vscode.window.showWarningMessage('The selected text is empty. Select code or text for VTCode to inspect.');
            return;
        }

        if (!(await ensureCliAvailableForCommand())) {
            return;
        }

        const defaultQuestion = 'Explain the highlighted selection.';
        const question = await vscode.window.showInputBox({
            prompt: 'How should VTCode help with the highlighted selection?',
            value: defaultQuestion,
            valueSelection: [0, defaultQuestion.length],
            ignoreFocusOut: true
        });

        if (question === undefined) {
            return;
        }

        const trimmedQuestion = question.trim() || defaultQuestion;
        const languageId = editor.document.languageId || 'text';
        const rangeLabel = `${selection.start.line + 1}-${selection.end.line + 1}`;
        const workspaceFolder = vscode.workspace.getWorkspaceFolder(editor.document.uri);
        const relativePath = workspaceFolder
            ? vscode.workspace.asRelativePath(editor.document.uri, false)
            : editor.document.fileName;
        const normalizedSelection = selectedText.replace(/\r\n/g, '\n');
        const prompt = `${trimmedQuestion}\n\nFile: ${relativePath}\nLines: ${rangeLabel}\n\n\
\u0060\u0060\u0060${languageId}\n${normalizedSelection}\n\u0060\u0060\u0060`;

        try {
            await runVtcodeCommand(['ask', prompt], { title: 'Asking VTCode about the selection…' });
            void vscode.window.showInformationMessage('VTCode processed the highlighted selection. Review the output channel for the response.');
        } catch (error) {
            handleCommandError('ask about the selection', error);
        }
    });

    const openConfig = vscode.commands.registerCommand('vtcode.openConfig', async () => {
        try {
            const configUri = await findVtcodeConfig();
            if (!configUri) {
                void vscode.window.showWarningMessage('No vtcode.toml file was found in this workspace.');
                return;
            }

            const document = await vscode.workspace.openTextDocument(configUri);
            await vscode.window.showTextDocument(document, { preview: false });
        } catch (error) {
            handleCommandError('open configuration', error);
        }
    });

    const openDocumentation = vscode.commands.registerCommand('vtcode.openDocumentation', async () => {
        await vscode.env.openExternal(vscode.Uri.parse('https://github.com/vinhnx/vtcode#readme'));
    });

    const openDeepWiki = vscode.commands.registerCommand('vtcode.openDeepWiki', async () => {
        await vscode.env.openExternal(vscode.Uri.parse('https://deepwiki.com/vinhnx/vtcode'));
    });

    const openWalkthrough = vscode.commands.registerCommand('vtcode.openWalkthrough', async () => {
        await vscode.commands.executeCommand('workbench.action.openWalkthrough', 'vtcode.walkthrough');
    });

    const openInstallGuide = vscode.commands.registerCommand('vtcode.openInstallGuide', async () => {
        await vscode.env.openExternal(vscode.Uri.parse('https://github.com/vinhnx/vtcode#installation'));
    });

    const launchAgentTerminal = vscode.commands.registerCommand('vtcode.launchAgentTerminal', async () => {
        if (!(await ensureCliAvailableForCommand())) {
            return;
        }

        const cwd = getWorkspaceRoot();
        if (!cwd) {
            void vscode.window.showWarningMessage('Open a workspace folder before launching the VTCode agent terminal.');
            return;
        }

        const commandPath = getConfiguredCommandPath();
        const { terminal, created } = ensureAgentTerminal(commandPath, cwd);
        terminal.show(true);
        if (created) {
            const channel = getOutputChannel();
            channel.appendLine(`[info] Launching VTCode agent terminal with "${commandPath} chat" in ${cwd}.`);
        }
    });

    const runAnalyze = vscode.commands.registerCommand('vtcode.runAnalyze', async () => {
        if (!(await ensureCliAvailableForCommand())) {
            return;
        }

        try {
            await runVtcodeCommand(['analyze'], { title: 'Analyzing workspace with VTCode…' });
            void vscode.window.showInformationMessage('VTCode finished analyzing the workspace. Review the VTCode output channel for results.');
        } catch (error) {
            handleCommandError('analyze the workspace', error);
        }
    });

    const refreshQuickActions = vscode.commands.registerCommand('vtcode.refreshQuickActions', async () => {
        quickActionsProviderInstance?.refresh();
        await refreshCliAvailability('manual');
    });

    const configurationWatcher = vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration('vtcode.commandPath')) {
            void refreshCliAvailability('configuration');
        }
    });

    context.subscriptions.push(
        quickActionsCommand,
        askAgent,
        askSelection,
        openConfig,
        openDocumentation,
        openDeepWiki,
        openWalkthrough,
        openInstallGuide,
        launchAgentTerminal,
        runAnalyze,
        refreshQuickActions,
        configurationWatcher
    );

    void refreshCliAvailability('activation');
}

export function deactivate() {
    if (outputChannel) {
        outputChannel.dispose();
        outputChannel = undefined;
    }

    if (statusBarItem) {
        statusBarItem.dispose();
        statusBarItem = undefined;
    }

    if (agentTerminal) {
        agentTerminal.dispose();
        agentTerminal = undefined;
    }

    if (terminalCloseListener) {
        terminalCloseListener.dispose();
        terminalCloseListener = undefined;
    }

    quickActionsProviderInstance = undefined;
    cliAvailabilityCheck = undefined;
}

async function ensureCliAvailableForCommand(): Promise<boolean> {
    await refreshCliAvailability('manual');

    if (cliAvailable) {
        return true;
    }

    const commandPath = getConfiguredCommandPath();
    const selection = await vscode.window.showWarningMessage(
        `The VTCode CLI ("${commandPath}") is not available. Install the CLI or update the "vtcode.commandPath" setting to run this command.`,
        'Open Installation Guide'
    );

    if (selection === 'Open Installation Guide') {
        await vscode.commands.executeCommand('vtcode.openInstallGuide');
    }

    return false;
}

function createQuickActions(cliAvailableState: boolean): QuickActionDescription[] {
    const actions: QuickActionDescription[] = [];

    if (cliAvailableState) {
        actions.push(
            {
                label: 'Ask the VTCode agent…',
                description: 'Send a one-off question and stream the answer in VS Code.',
                command: 'vtcode.askAgent',
                icon: 'comment-discussion'
            },
            {
                label: 'Ask about highlighted selection',
                description: 'Right-click or trigger VTCode to explain the selected text.',
                command: 'vtcode.askSelection',
                icon: 'comment'
            },
            {
                label: 'Launch interactive VTCode terminal',
                description: 'Open an integrated terminal session running vtcode chat.',
                command: 'vtcode.launchAgentTerminal',
                icon: 'terminal'
            },
            {
                label: 'Analyze workspace with VTCode',
                description: 'Run vtcode analyze and stream the report to the VTCode output channel.',
                command: 'vtcode.runAnalyze',
                icon: 'pulse'
            }
        );
    } else {
        actions.push({
            label: 'Review VTCode CLI installation',
            description: 'Open the VTCode CLI installation instructions required for ask commands.',
            command: 'vtcode.openInstallGuide',
            icon: 'tools'
        });
    }

    actions.push(
        {
            label: 'Open vtcode.toml',
            description: 'Jump directly to the workspace VTCode configuration file.',
            command: 'vtcode.openConfig',
            icon: 'gear'
        },
        {
            label: 'View VTCode documentation',
            description: 'Open the VTCode README in your browser.',
            command: 'vtcode.openDocumentation',
            icon: 'book'
        },
        {
            label: 'Review VTCode DeepWiki overview',
            description: 'Open the DeepWiki page for VTCode capabilities.',
            command: 'vtcode.openDeepWiki',
            icon: 'globe'
        },
        {
            label: 'Explore the VTCode walkthrough',
            description: 'Open the getting-started walkthrough to learn about VTCode features.',
            command: 'vtcode.openWalkthrough',
            icon: 'rocket'
        }
    );

    return actions;
}

async function refreshCliAvailability(trigger: 'activation' | 'configuration' | 'manual'): Promise<void> {
    if (cliAvailabilityCheck) {
        await cliAvailabilityCheck;
        return;
    }

    const commandPath = getConfiguredCommandPath();
    setStatusBarChecking(commandPath);

    cliAvailabilityCheck = (async () => {
        if (vscode.env.uiKind === vscode.UIKind.Web) {
            updateCliAvailabilityState(false, commandPath);
            return;
        }

        const available = await detectCliAvailability(commandPath);
        updateCliAvailabilityState(available, commandPath);

        if (!available && trigger === 'activation') {
            await maybeShowMissingCliWarning(commandPath);
        }
    })();

    try {
        await cliAvailabilityCheck;
    } finally {
        cliAvailabilityCheck = undefined;
    }
}

async function detectCliAvailability(commandPath: string): Promise<boolean> {
    if (!commandPath) {
        return false;
    }

    return new Promise((resolve) => {
        let resolved = false;

        const complete = (value: boolean) => {
            if (!resolved) {
                resolved = true;
                resolve(value);
            }
        };

        try {
            const child = spawn(commandPath, ['--version'], {
                shell: false,
                env: process.env
            });

            const timer = setTimeout(() => {
                child.kill();
                complete(false);
            }, CLI_DETECTION_TIMEOUT_MS);

            child.on('error', () => {
                clearTimeout(timer);
                complete(false);
            });

            child.on('close', (code) => {
                clearTimeout(timer);
                complete(code === 0);
            });
        } catch (error) {
            complete(false);
        }
    });
}

function updateCliAvailabilityState(available: boolean, commandPath: string) {
    const previous = cliAvailable;
    cliAvailable = available;

    void vscode.commands.executeCommand('setContext', 'vtcode.cliAvailable', available);
    updateStatusBarItem(commandPath, available);

    if (available) {
        missingCliWarningShown = false;
    }

    if (previous !== available) {
        const channel = getOutputChannel();
        if (available) {
            channel.appendLine(`[info] Detected VTCode CLI using "${commandPath}".`);
        } else {
            channel.appendLine(`[warn] VTCode CLI not found using "${commandPath}".`);
        }
    }

    quickActionsProviderInstance?.refresh();
}

function setStatusBarChecking(commandPath: string) {
    if (!statusBarItem) {
        return;
    }

    statusBarItem.text = '$(sync~spin) VTCode';
    statusBarItem.tooltip = `Checking availability of "${commandPath}"`;
    statusBarItem.command = undefined;
    statusBarItem.backgroundColor = undefined;
    statusBarItem.color = undefined;
    statusBarItem.show();
}

function updateStatusBarItem(commandPath: string, available: boolean) {
    if (!statusBarItem) {
        return;
    }

    if (available) {
        statusBarItem.text = '$(tools) VTCode Ready';
        statusBarItem.tooltip = `VTCode CLI "${commandPath}" is available.`;
        statusBarItem.command = 'vtcode.openQuickActions';
        statusBarItem.backgroundColor = new vscode.ThemeColor('vtcode.statusBarBackground');
        statusBarItem.color = new vscode.ThemeColor('vtcode.statusBarForeground');
    } else {
        statusBarItem.text = '$(warning) VTCode CLI Missing';
        statusBarItem.tooltip = `VTCode CLI "${commandPath}" is not available. Click to view installation instructions.`;
        statusBarItem.command = 'vtcode.openInstallGuide';
        statusBarItem.backgroundColor = new vscode.ThemeColor('statusBarItem.warningBackground');
        statusBarItem.color = new vscode.ThemeColor('statusBarItem.warningForeground');
    }

    statusBarItem.show();
}

async function maybeShowMissingCliWarning(commandPath: string): Promise<void> {
    if (missingCliWarningShown || vscode.env.uiKind === vscode.UIKind.Web) {
        return;
    }

    missingCliWarningShown = true;
    const selection = await vscode.window.showWarningMessage(
        `VTCode CLI was not found on PATH as "${commandPath}".`,
        'Open Installation Guide'
    );

    if (selection === 'Open Installation Guide') {
        await vscode.commands.executeCommand('vtcode.openInstallGuide');
    }
}

function getConfiguredCommandPath(): string {
    return vscode.workspace.getConfiguration('vtcode').get<string>('commandPath', 'vtcode').trim() || 'vtcode';
}

async function runVtcodeCommand(args: string[], options?: { title?: string }): Promise<void> {
    if (vscode.env.uiKind === vscode.UIKind.Web) {
        throw new Error('VTCode commands that spawn the CLI are not available in the web extension host.');
    }

    const commandPath = getConfiguredCommandPath();
    const cwd = getWorkspaceRoot();
    if (!cwd) {
        throw new Error('Open a workspace folder before running VTCode commands.');
    }

    const channel = getOutputChannel();
    const displayArgs = args
        .map((arg) => {
            const value = String(arg);
            return /(\s|"|')/.test(value) ? JSON.stringify(value) : value;
        })
        .join(' ');

    channel.show(true);
    channel.appendLine(`$ ${commandPath} ${displayArgs}`);

    await vscode.window.withProgress(
        {
            location: vscode.ProgressLocation.Notification,
            title: options?.title ?? 'Running VTCode…'
        },
        async () =>
            new Promise<void>((resolve, reject) => {
                const child = spawn(commandPath, args.map((arg) => String(arg)), {
                    cwd,
                    shell: false,
                    env: process.env
                });

                child.stdout.on('data', (data: Buffer) => {
                    channel.append(data.toString());
                });

                child.stderr.on('data', (data: Buffer) => {
                    channel.append(data.toString());
                });

                child.on('error', (error: Error) => {
                    reject(error);
                });

                child.on('close', (code) => {
                    if (code === 0) {
                        resolve();
                    } else {
                        reject(new Error(`VTCode exited with code ${code ?? 'unknown'}`));
                    }
                });
            })
    );
}

async function findVtcodeConfig(): Promise<vscode.Uri | undefined> {
    const matches = await vscode.workspace.findFiles('**/vtcode.toml', '**/{node_modules,dist,out,.git,target}/**', 5);

    if (matches.length === 0) {
        return undefined;
    }

    if (matches.length === 1) {
        return matches[0];
    }

    const items = matches.map((uri) => ({
        label: vscode.workspace.asRelativePath(uri),
        uri
    }));

    const selection = await vscode.window.showQuickPick(items, {
        placeHolder: 'Select the vtcode.toml to open'
    });

    return selection?.uri;
}

function getWorkspaceRoot(): string | undefined {
    const activeEditor = vscode.window.activeTextEditor;
    if (activeEditor) {
        const workspaceFolder = vscode.workspace.getWorkspaceFolder(activeEditor.document.uri);
        if (workspaceFolder) {
            return workspaceFolder.uri.fsPath;
        }
    }

    const [firstWorkspace] = vscode.workspace.workspaceFolders ?? [];
    return firstWorkspace?.uri.fsPath;
}

function handleCommandError(contextLabel: string, error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    void vscode.window.showErrorMessage(`Failed to ${contextLabel} with VTCode: ${message}`);
}

function getOutputChannel(): vscode.OutputChannel {
    if (!outputChannel) {
        outputChannel = vscode.window.createOutputChannel('VTCode');
    }

    return outputChannel;
}

function ensureAgentTerminal(commandPath: string, cwd: string): { terminal: vscode.Terminal; created: boolean } {
    if (agentTerminal) {
        return { terminal: agentTerminal, created: false };
    }

    const terminal = vscode.window.createTerminal({
        name: 'VTCode Agent',
        cwd,
        env: process.env,
        iconPath: new vscode.ThemeIcon('comment-discussion')
    });
    const quotedCommandPath = /\s/.test(commandPath) ? `"${commandPath.replace(/"/g, '\\"')}"` : commandPath;
    terminal.sendText(`${quotedCommandPath} chat`, true);

    agentTerminal = terminal;

    if (!terminalCloseListener) {
        terminalCloseListener = vscode.window.onDidCloseTerminal((closed) => {
            if (closed === agentTerminal) {
                agentTerminal = undefined;
                terminalCloseListener?.dispose();
                terminalCloseListener = undefined;
            }
        });
    }

    return { terminal, created: true };
}

function ensureStableApi(context: vscode.ExtensionContext) {
    const manifest = context.extension.packageJSON as { enabledApiProposals?: string[] } | undefined;
    const proposals = manifest?.enabledApiProposals ?? [];

    if (proposals.length > 0) {
        const channel = getOutputChannel();
        channel.appendLine(`[warn] Proposed VS Code APIs enabled: ${proposals.join(', ')}.`);
    }
}

function logExtensionHostContext(context: vscode.ExtensionContext) {
    const channel = getOutputChannel();
    const remoteName = vscode.env.remoteName ? `remote (${vscode.env.remoteName})` : 'local';
    const hostKind = vscode.env.uiKind === vscode.UIKind.Web ? 'web' : 'desktop';
    const modeLabel = getExtensionModeLabel(context.extensionMode);
    channel.appendLine(`[info] VTCode Companion activated in ${remoteName} ${hostKind} host (${modeLabel} mode).`);
}

function getExtensionModeLabel(mode: vscode.ExtensionMode): string {
    switch (mode) {
        case vscode.ExtensionMode.Development:
            return 'development';
        case vscode.ExtensionMode.Test:
            return 'test';
        case vscode.ExtensionMode.Production:
        default:
            return 'production';
    }
}
