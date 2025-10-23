import * as vscode from 'vscode';

export function activate(context: vscode.ExtensionContext) {
    const helloWorld = vscode.commands.registerCommand('vtcode-hello-world.helloWorld', async () => {
        const workspaceName = vscode.workspace.name ?? 'world';
        const message = `Hello, ${workspaceName}! Welcome to VTCode.`;
        await vscode.window.showInformationMessage(message);
    });

    context.subscriptions.push(helloWorld);
}

export function deactivate() {
    // This extension does not allocate external resources that need cleanup.
}
