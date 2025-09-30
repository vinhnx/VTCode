import * as vscode from 'vscode';
import { log } from '@repo/shared/lib/logger';
import {
    ExtensionCliArgument,
    ExtensionCommand,
    ExtensionConfigKey,
    ExtensionDefaultValue,
    ExtensionEnvVar,
    ExtensionLogMessage,
    ExtensionNotificationMessage
} from './constants';

type EnvironmentValue = ExtensionDefaultValue | string;

const readEnvironmentValue = (
    key: ExtensionConfigKey,
    fallback: ExtensionDefaultValue
): string => {
    const envValue = process.env[key];
    if (envValue && envValue.trim().length > 0) {
        return envValue;
    }

    return fallback;
};

const getWorkspaceFolder = (): vscode.WorkspaceFolder | undefined => {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders || folders.length === 0) {
        return undefined;
    }

    return folders[0];
};

const disposeExistingTerminal = (terminalName: string): void => {
    const existingTerminal = vscode.window.terminals.find((terminal) => {
        return terminal.name === terminalName;
    });

    if (existingTerminal) {
        log.info({ terminalName }, ExtensionLogMessage.DisposingTerminal);
        existingTerminal.dispose();
    }
};

const launchChatTerminal = async (): Promise<void> => {
    const workspaceFolder = getWorkspaceFolder();
    if (!workspaceFolder) {
        log.error({}, ExtensionLogMessage.MissingWorkspace);
        void vscode.window.showErrorMessage(ExtensionNotificationMessage.WorkspaceMissing);
        return;
    }

    const workspacePath = workspaceFolder.uri.fsPath;
    const binaryPath: EnvironmentValue = readEnvironmentValue(
        ExtensionConfigKey.BinaryPath,
        ExtensionDefaultValue.BinaryPath
    );
    const terminalName: EnvironmentValue = readEnvironmentValue(
        ExtensionConfigKey.TerminalName,
        ExtensionDefaultValue.TerminalName
    );

    disposeExistingTerminal(terminalName);

    const terminalOptions: vscode.TerminalOptions = {
        name: terminalName,
        shellPath: binaryPath,
        shellArgs: [ExtensionCliArgument.Chat],
        cwd: workspacePath,
        env: {
            [ExtensionEnvVar.WorkspaceDir]: workspacePath
        }
    };

    log.info(
        {
            binaryPath,
            command: ExtensionCommand.StartChat,
            workspacePath,
            terminalName
        },
        ExtensionLogMessage.LaunchingTerminal
    );

    const terminal = vscode.window.createTerminal(terminalOptions);
    terminal.show();
    void vscode.window.showInformationMessage(ExtensionNotificationMessage.ChatLaunched);
};

const registerChatCommand = (context: vscode.ExtensionContext): void => {
    const command = vscode.commands.registerCommand(ExtensionCommand.StartChat, () => {
        launchChatTerminal().catch((error: unknown) => {
            const message = error instanceof Error ? error.message : String(error);
            log.error({ error: message }, ExtensionLogMessage.ChatFailed);
            void vscode.window.showErrorMessage(ExtensionLogMessage.ChatFailed);
        });
    });

    context.subscriptions.push(command);
};

export const activate = (context: vscode.ExtensionContext): void => {
    const activationMessage = readEnvironmentValue(
        ExtensionConfigKey.ActivationMessage,
        ExtensionDefaultValue.ActivationMessage
    );

    log.info({}, ExtensionLogMessage.ActivationComplete);
    registerChatCommand(context);
    void vscode.window.showInformationMessage(activationMessage);
};

export const deactivate = (): void => {
    log.info({}, ExtensionLogMessage.Deactivated);
};
