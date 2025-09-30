import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';
import { log } from '@repo/shared/lib/logger';
import {
    ExtensionCliArgument,
    ExtensionCommand,
    ExtensionConfigKey,
    ExtensionDefaultValue,
    ExtensionEnvVar,
    ExtensionLogMessage,
    ExtensionNotificationMessage,
    ExtensionUiKindLabel
} from './constants';

type EnvironmentValue = ExtensionDefaultValue | string;

type WorkspaceMetadata = {
    path: string;
    name: string;
    folderCount: number;
};

type ActiveDocumentMetadata = {
    path?: string;
    languageId?: string;
};

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

const isFileWithinWorkspace = (workspacePath: string, filePath: string): boolean => {
    const relativePath = path.relative(workspacePath, filePath);
    if (relativePath.length === 0) {
        return true;
    }

    return !relativePath.startsWith('..') && !path.isAbsolute(relativePath);
};

const getUriFromActiveTab = (): vscode.Uri | undefined => {
    const activeGroup = vscode.window.tabGroups.activeTabGroup;
    const activeTab = activeGroup?.activeTab;
    if (!activeTab || !activeTab.input) {
        return undefined;
    }

    const input = activeTab.input as Partial<vscode.TabInputText & vscode.TabInputTextDiff>;
    if (input.uri instanceof vscode.Uri) {
        return input.uri;
    }

    if (input.modified instanceof vscode.Uri) {
        return input.modified;
    }

    return undefined;
};

const resolveActiveDocumentMetadata = (workspacePath: string): ActiveDocumentMetadata => {
    const editor = vscode.window.activeTextEditor;
    const editorUri = editor?.document.uri;
    const tabUri = getUriFromActiveTab();
    const candidateUri = editorUri ?? tabUri;

    if (!candidateUri || candidateUri.scheme !== 'file') {
        return {};
    }

    const candidatePath = candidateUri.fsPath;
    if (!isFileWithinWorkspace(workspacePath, candidatePath)) {
        log.warn(
            { candidatePath, workspacePath },
            ExtensionLogMessage.ActiveDocumentOutsideWorkspace
        );
        return {};
    }

    return {
        path: candidatePath,
        languageId: editor?.document.languageId
    };
};

const getWorkspaceMetadata = (workspaceFolder: vscode.WorkspaceFolder): WorkspaceMetadata => {
    const folders = vscode.workspace.workspaceFolders;
    const folderCount = folders ? folders.length : 0;

    return {
        path: workspaceFolder.uri.fsPath,
        name: workspaceFolder.name,
        folderCount
    };
};

const getUiKindLabel = (uiKind: vscode.UIKind): ExtensionUiKindLabel => {
    if (uiKind === vscode.UIKind.Web) {
        return ExtensionUiKindLabel.Web;
    }

    return ExtensionUiKindLabel.Desktop;
};

const attachIfPresent = (
    environment: Record<string, string>,
    key: ExtensionEnvVar,
    value: string | undefined
): void => {
    if (value && value.trim().length > 0) {
        environment[key] = value;
    }
};

const collectEnvironmentContext = (
    workspace: WorkspaceMetadata,
    activeDocument: ActiveDocumentMetadata
): Record<string, string> => {
    const environment: Record<string, string> = {
        [ExtensionEnvVar.WorkspaceDir]: workspace.path,
        [ExtensionEnvVar.WorkspaceName]: workspace.name,
        [ExtensionEnvVar.WorkspaceFolderCount]: workspace.folderCount.toString(),
        [ExtensionEnvVar.AppName]: vscode.env.appName,
        [ExtensionEnvVar.AppHost]: vscode.env.appHost,
        [ExtensionEnvVar.UiKind]: getUiKindLabel(vscode.env.uiKind),
        [ExtensionEnvVar.Version]: vscode.version,
        [ExtensionEnvVar.Platform]: os.platform()
    };

    attachIfPresent(environment, ExtensionEnvVar.RemoteName, vscode.env.remoteName ?? undefined);

    if (activeDocument.path) {
        environment[ExtensionEnvVar.ActiveDocument] = activeDocument.path;
        attachIfPresent(
            environment,
            ExtensionEnvVar.ActiveDocumentLanguage,
            activeDocument.languageId
        );
    }

    log.info(
        {
            workspaceName: workspace.name,
            folderCount: workspace.folderCount,
            remoteName: vscode.env.remoteName,
            uiKind: environment[ExtensionEnvVar.UiKind],
            activeDocumentPath: activeDocument.path,
            activeDocumentLanguage: activeDocument.languageId
        },
        ExtensionLogMessage.EnrichedEnvironmentReady
    );

    return environment;
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

    const workspaceMetadata = getWorkspaceMetadata(workspaceFolder);
    const binaryPath: EnvironmentValue = readEnvironmentValue(
        ExtensionConfigKey.BinaryPath,
        ExtensionDefaultValue.BinaryPath
    );
    const terminalName: EnvironmentValue = readEnvironmentValue(
        ExtensionConfigKey.TerminalName,
        ExtensionDefaultValue.TerminalName
    );
    const activeDocumentMetadata = resolveActiveDocumentMetadata(workspaceMetadata.path);

    disposeExistingTerminal(terminalName);

    const environment = collectEnvironmentContext(workspaceMetadata, activeDocumentMetadata);
    if (activeDocumentMetadata.path) {
        log.info(
            {
                activeDocumentPath: activeDocumentMetadata.path,
                workspacePath: workspaceMetadata.path,
                terminalName
            },
            ExtensionLogMessage.ActiveDocumentAttached
        );
    }

    const terminalOptions: vscode.TerminalOptions = {
        name: terminalName,
        shellPath: binaryPath,
        shellArgs: [ExtensionCliArgument.Chat],
        cwd: workspaceMetadata.path,
        env: environment
    };

    log.info(
        {
            binaryPath,
            command: ExtensionCommand.StartChat,
            workspacePath: workspaceMetadata.path,
            terminalName,
            activeDocumentPath: activeDocumentMetadata.path
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
