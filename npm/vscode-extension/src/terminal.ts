import { Terminal, window } from 'vscode';
import { log } from '@repo/shared/lib/logger';
import {
    ExtensionLogMetadataKey,
    ExtensionLogMessage,
    ExtensionTerminal
} from './constants';

export const ensureTerminal = (): Terminal => {
    const existingTerminal = window.terminals.find(
        (terminal) => terminal.name === ExtensionTerminal.Name
    );

    if (existingTerminal) {
        log.info(
            { [ExtensionLogMetadataKey.TerminalName]: ExtensionTerminal.Name },
            ExtensionLogMessage.TerminalReused
        );
        return existingTerminal;
    }

    const createdTerminal = window.createTerminal({ name: ExtensionTerminal.Name });
    log.info(
        { [ExtensionLogMetadataKey.TerminalName]: ExtensionTerminal.Name },
        ExtensionLogMessage.TerminalCreated
    );
    return createdTerminal;
};

export const showTerminal = (terminal: Terminal): void => {
    terminal.show();
};

export const sendCommandToTerminal = (terminal: Terminal, commandLine: string): void => {
    terminal.sendText(commandLine, true);
    log.info(
        { [ExtensionLogMetadataKey.TerminalName]: ExtensionTerminal.Name },
        ExtensionLogMessage.CommandExecuted
    );
};
