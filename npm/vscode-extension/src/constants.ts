export enum ExtensionCommand {
    RunAgent = 'vtcode.run'
}

export enum ExtensionConfigurationSection {
    Root = 'vtcode'
}

export enum ExtensionConfigurationKey {
    Executable = 'executable',
    Arguments = 'arguments'
}

export enum ExtensionEnvironmentVariable {
    Executable = 'VTCODE_EXECUTABLE',
    Arguments = 'VTCODE_EXECUTABLE_ARGS'
}

export enum ExtensionDefaultValue {
    Executable = 'vtcode',
    Arguments = ''
}

export enum ExtensionTerminal {
    Name = 'VT Code'
}

export enum ExtensionLogMessage {
    Activation = 'Activating VT Code extension',
    CommandRegistered = 'Registered VT Code command',
    CommandInvoked = 'Launching VT Code terminal command',
    TerminalCreated = 'Created VT Code terminal instance',
    TerminalReused = 'Reusing existing VT Code terminal instance',
    CommandExecuted = 'Sent command to VT Code terminal',
    TerminalMissing = 'No terminal instance available for VT Code',
    Deactivation = 'Deactivating VT Code extension'
}

export enum ExtensionLogMetadataKey {
    Command = 'command',
    Arguments = 'arguments',
    TerminalName = 'terminalName'
}

export enum ExtensionLogMetadataValue {
    Empty = 'empty'
}

export enum ExtensionStringSymbol {
    Space = ' '
}
