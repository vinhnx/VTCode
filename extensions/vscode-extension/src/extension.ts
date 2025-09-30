import * as vscode from 'vscode';
import { ExtensionCommand, ExtensionConfigKey, ExtensionDefaultValue } from './constants';

const readEnvironmentValue = (key: ExtensionConfigKey, fallback: ExtensionDefaultValue): string => {
    const envValue = process.env[key];
    if (envValue && envValue.trim().length > 0) {
        return envValue;
    }
    return fallback;
};

const getActivationMessage = (): string => {
    return readEnvironmentValue(
        ExtensionConfigKey.ActivationMessage,
        ExtensionDefaultValue.ActivationMessage
    );
};

const getGreetingMessage = (): string => {
    return readEnvironmentValue(
        ExtensionConfigKey.GreetingMessage,
        ExtensionDefaultValue.GreetingMessage
    );
};

export const activate = (context: vscode.ExtensionContext): void => {
    const activationMessage = getActivationMessage();
    void vscode.window.showInformationMessage(activationMessage);

    const showGreetingCommand = vscode.commands.registerCommand(
        ExtensionCommand.ShowGreeting,
        () => {
            const greetingMessage = getGreetingMessage();
            void vscode.window.showInformationMessage(greetingMessage);
        }
    );

    context.subscriptions.push(showGreetingCommand);
};

export const deactivate = (): void => {
    // No resources to dispose
};
