import * as vscode from 'vscode';
import { log } from '@repo/shared/lib/logger';
import {
    ExtensionCommand,
    ExtensionConfigurationKey,
    ExtensionConfigurationSection,
    ExtensionDefaultValue,
    ExtensionEnvironmentVariable,
    ExtensionLogMessage,
    ExtensionLogMetadataKey,
    ExtensionLogMetadataValue,
    ExtensionStringSymbol
} from './constants';
import {
    ensureTerminal,
    sendCommandToTerminal,
    showTerminal
} from './terminal';

const sanitizeValue = (value: string | undefined): string | undefined => {
    if (!value) {
        return undefined;
    }

    const trimmedValue = value.trim();
    return trimmedValue.length > 0 ? trimmedValue : undefined;
};

const resolveExecutable = (
    configuration: vscode.WorkspaceConfiguration
): string => {
    const environmentValue = sanitizeValue(
        process.env[ExtensionEnvironmentVariable.Executable]
    );

    if (environmentValue) {
        return environmentValue;
    }

    const configurationValue = sanitizeValue(
        configuration.get<string>(ExtensionConfigurationKey.Executable)
    );

    if (configurationValue) {
        return configurationValue;
    }

    return ExtensionDefaultValue.Executable;
};

const resolveArguments = (
    configuration: vscode.WorkspaceConfiguration
): string | undefined => {
    const environmentValue = sanitizeValue(
        process.env[ExtensionEnvironmentVariable.Arguments]
    );

    if (environmentValue) {
        return environmentValue;
    }

    const configurationValue = sanitizeValue(
        configuration.get<string>(ExtensionConfigurationKey.Arguments)
    );

    if (configurationValue) {
        return configurationValue;
    }

    return sanitizeValue(ExtensionDefaultValue.Arguments);
};

const createCommandLine = (command: string, args: string | undefined): string => {
    if (!args) {
        return command;
    }

    return [command, args].join(ExtensionStringSymbol.Space);
};

export const activate = (context: vscode.ExtensionContext): void => {
    log.info({}, ExtensionLogMessage.Activation);

    const commandRegistration = vscode.commands.registerCommand(
        ExtensionCommand.RunAgent,
        async () => {
            const configuration = vscode.workspace.getConfiguration(
                ExtensionConfigurationSection.Root
            );

            const executable = resolveExecutable(configuration);
            const argumentString = resolveArguments(configuration);

            log.info(
                {
                    [ExtensionLogMetadataKey.Command]: executable,
                    [ExtensionLogMetadataKey.Arguments]: argumentString || ExtensionLogMetadataValue.Empty
                },
                ExtensionLogMessage.CommandInvoked
            );

            const terminal = ensureTerminal();
            showTerminal(terminal);

            const commandLine = createCommandLine(executable, argumentString);
            sendCommandToTerminal(terminal, commandLine);
        }
    );

    log.info({}, ExtensionLogMessage.CommandRegistered);
    context.subscriptions.push(commandRegistration);
};

export const deactivate = (): void => {
    log.info({}, ExtensionLogMessage.Deactivation);
};
