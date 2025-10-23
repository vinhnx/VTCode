import * as vscode from 'vscode';
import { spawn } from 'child_process';

type QuickActionItem = vscode.QuickPickItem & { run: () => Thenable<unknown> | void };

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

    const quickActions = vscode.commands.registerCommand('vtcode.openQuickActions', async () => {
        const actions: QuickActionItem[] = [
            {
                label: '$(comment-discussion) Ask the VTCode agent…',
                description: 'Send a one-off question to the VTCode CLI and stream the response here.',
                run: () => vscode.commands.executeCommand('vtcode.askAgent')
            },
            {
                label: '$(gear) Open vtcode.toml',
                description: 'Open the workspace configuration for VTCode.',
                run: () => vscode.commands.executeCommand('vtcode.openConfig')
            },
            {
                label: '$(book) Open documentation',
                description: 'Read the VTCode README in your browser.',
                run: () => vscode.commands.executeCommand('vtcode.openDocumentation')
            },
            {
                label: '$(globe) DeepWiki overview',
                description: 'Review the VTCode capability notes on DeepWiki.',
                run: () => vscode.commands.executeCommand('vtcode.openDeepWiki')
            }
        ];

        const selection = await vscode.window.showQuickPick(actions, {
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
            await runVtcodeCommand(['ask', question]);
            vscode.window.showInformationMessage('VTCode finished processing your request. Check the VTCode output channel for details.');
        } catch (error) {
            handleCommandError('ask', error);
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

    context.subscriptions.push(quickActions, askAgent, openConfig, openDocumentation, openDeepWiki);
}

export function deactivate() {
    if (outputChannel) {
        outputChannel.dispose();
        outputChannel = undefined;
    }
}

async function runVtcodeCommand(args: string[]): Promise<void> {
    const commandPath = vscode.workspace.getConfiguration('vtcode').get<string>('commandPath', 'vtcode').trim() || 'vtcode';
    const cwd = getWorkspaceRoot();
    const channel = getOutputChannel();
    const displayArgs = args
        .map((arg) => (/(\s|"|')/.test(arg) ? `"${arg.replace(/"/g, '\"')}"` : arg))
        .join(' ');

    channel.show(true);
    channel.appendLine(`$ ${commandPath} ${displayArgs}`);

    await new Promise<void>((resolve, reject) => {
        const child = spawn(commandPath, args, {
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
    });
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
