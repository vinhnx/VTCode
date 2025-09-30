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
    WorkspaceDir = 'WORKSPACE_DIR'
}

export enum ExtensionNotificationMessage {
    ChatLaunched = 'VT Code chat session launched in the integrated terminal.',
    WorkspaceMissing = 'Open a workspace folder to start the VT Code chat loop.'
}

export enum ExtensionLogMessage {
    ActivationComplete = 'VT Code extension activated.',
    ChatFailed = 'Failed to start VT Code chat terminal.',
    Deactivated = 'VT Code extension deactivated.',
    DisposingTerminal = 'Disposing existing VT Code chat terminal.',
    LaunchingTerminal = 'Creating VT Code chat terminal.',
    MissingWorkspace = 'Unable to locate a workspace folder for the VT Code chat loop.'
}
