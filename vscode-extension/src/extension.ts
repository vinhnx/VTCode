import * as vscode from 'vscode';
import { spawn } from 'child_process';

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

export function activate(context: vscode.ExtensionContext) {
    outputChannel = vscode.window.createOutputChannel('VTCode');
    context.subscriptions.push(outputChannel);

    const statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    statusBarItem.text = '$(tools) VTCode';
    statusBarItem.tooltip = 'Open VTCode quick actions';
    statusBarItem.command = 'vtcode.openQuickActions';
    statusBarItem.show();
    context.subscriptions.push(statusBarItem);

    const getActions = (): QuickActionDescription[] => [
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
    ];

    const quickActionsProvider = new QuickActionTreeDataProvider(getActions);
    context.subscriptions.push(vscode.window.registerTreeDataProvider('vtcodeQuickActionsView', quickActionsProvider));

    const quickActionsCommand = vscode.commands.registerCommand('vtcode.openQuickActions', async () => {
        const pickItems: QuickActionItem[] = getActions().map((action) => ({
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
            vscode.window.showInformationMessage('VTCode finished processing your request. Check the VTCode output channel for details.');
        } catch (error) {
            handleCommandError('ask', error);
        }
    });

    const askSelection = vscode.commands.registerCommand('vtcode.askSelection', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            vscode.window.showWarningMessage('Open a text editor to ask VTCode about the current selection.');
            return;
        }

        const selection = editor.selection;
        if (selection.isEmpty) {
            vscode.window.showWarningMessage('Highlight text first, then run “Ask About Selection with VTCode.”');
            return;
        }

        const selectedText = editor.document.getText(selection).trim();
        if (!selectedText) {
            vscode.window.showWarningMessage('The selected text is empty. Select code or text for VTCode to inspect.');
            return;
        }

        try {
            await runVtcodeCommand(['ask', selectedText], { title: 'Asking VTCode about the selection…' });
            vscode.window.showInformationMessage('VTCode processed the highlighted selection. Review the output channel for the response.');
        } catch (error) {
            handleCommandError('ask about the selection', error);
        }
    });

    const openConfig = vscode.commands.registerCommand('vtcode.openConfig', async () => {
        try {
            const configUri = await findVtcodeConfig();
            if (!configUri) {
                vscode.window.showWarningMessage('No vtcode.toml file was found in this workspace.');
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

    const refreshQuickActions = vscode.commands.registerCommand('vtcode.refreshQuickActions', () => {
        quickActionsProvider.refresh();
    });

    context.subscriptions.push(
        quickActionsCommand,
        askAgent,
        askSelection,
        openConfig,
        openDocumentation,
        openDeepWiki,
        openWalkthrough,
        refreshQuickActions
    );
}

export function deactivate() {
    if (outputChannel) {
        outputChannel.dispose();
        outputChannel = undefined;
    }
}

async function runVtcodeCommand(args: string[], options?: { title?: string }): Promise<void> {
    const commandPath = vscode.workspace.getConfiguration('vtcode').get<string>('commandPath', 'vtcode').trim() || 'vtcode';
    const cwd = getWorkspaceRoot();
    if (!cwd) {
        throw new Error('Open a workspace folder before running VTCode commands.');
    }

    const channel = getOutputChannel();
    const displayArgs = args
        .map((arg) => (/(\s|"|')/.test(String(arg)) ? `"${String(arg).replace(/"/g, '\"')}"` : String(arg)))
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
                    shell: true,
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
    vscode.window.showErrorMessage(`Failed to ${contextLabel} with VTCode: ${message}`);
}

function getOutputChannel(): vscode.OutputChannel {
    if (!outputChannel) {
        outputChannel = vscode.window.createOutputChannel('VTCode');
    }

    return outputChannel;
}
