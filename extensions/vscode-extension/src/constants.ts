export enum ExtensionCommand {
    StartChat = 'vtcode.startChat'
}

export enum ExtensionConfigKey {
    ActivationMessage = 'VT_EXTENSION_ACTIVATION_MESSAGE',
    BinaryPath = 'VT_EXTENSION_VTCODE_BINARY',
    TerminalName = 'VT_EXTENSION_TERMINAL_NAME'
}

export enum ExtensionDefaultValue {
    ActivationMessage = 'VT Code extension activated.',
    BinaryPath = 'vtcode',
    TerminalName = 'VT Code Chat'
}

export enum ExtensionCliArgument {
    Chat = 'chat'
}

export enum ExtensionEnvVar {
    WorkspaceDir = 'WORKSPACE_DIR',
    ActiveDocument = 'VT_EXTENSION_ACTIVE_DOCUMENT',
    WorkspaceName = 'VT_EXTENSION_WORKSPACE_NAME',
    WorkspaceFolderCount = 'VT_EXTENSION_WORKSPACE_FOLDER_COUNT',
    ActiveDocumentLanguage = 'VT_EXTENSION_ACTIVE_DOCUMENT_LANGUAGE',
    AppName = 'VT_EXTENSION_VSCODE_APP_NAME',
    AppHost = 'VT_EXTENSION_VSCODE_APP_HOST',
    UiKind = 'VT_EXTENSION_VSCODE_UI_KIND',
    RemoteName = 'VT_EXTENSION_VSCODE_REMOTE_NAME',
    Version = 'VT_EXTENSION_VSCODE_VERSION',
    Platform = 'VT_EXTENSION_VSCODE_PLATFORM'
}

export enum ExtensionNotificationMessage {
    ChatLaunched = 'VT Code chat session launched in the integrated terminal.',
    WorkspaceMissing = 'Open a workspace folder to start the VT Code chat loop.'
}

export enum ExtensionUiKindLabel {
    Desktop = 'desktop',
    Web = 'web'
}

export enum ExtensionLogMessage {
    ActivationComplete = 'VT Code extension activated.',
    ChatFailed = 'Failed to start VT Code chat terminal.',
    Deactivated = 'VT Code extension deactivated.',
    DisposingTerminal = 'Disposing existing VT Code chat terminal.',
    LaunchingTerminal = 'Creating VT Code chat terminal.',
    MissingWorkspace = 'Unable to locate a workspace folder for the VT Code chat loop.',
    ActiveDocumentAttached = 'Forwarding active document context to VT Code chat.',
    ActiveDocumentOutsideWorkspace = 'Active document is outside the workspace; skipping context.',
    EnrichedEnvironmentReady = 'Collected VS Code environment context for VT Code chat session.'
}
