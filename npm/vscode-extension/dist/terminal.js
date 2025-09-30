"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.sendCommandToTerminal = exports.showTerminal = exports.ensureTerminal = void 0;
const vscode_1 = require("vscode");
const logger_1 = require("@repo/shared/lib/logger");
const constants_1 = require("./constants");
const ensureTerminal = () => {
    const existingTerminal = vscode_1.window.terminals.find((terminal) => terminal.name === constants_1.ExtensionTerminal.Name);
    if (existingTerminal) {
        logger_1.log.info({ [constants_1.ExtensionLogMetadataKey.TerminalName]: constants_1.ExtensionTerminal.Name }, constants_1.ExtensionLogMessage.TerminalReused);
        return existingTerminal;
    }
    const createdTerminal = vscode_1.window.createTerminal({ name: constants_1.ExtensionTerminal.Name });
    logger_1.log.info({ [constants_1.ExtensionLogMetadataKey.TerminalName]: constants_1.ExtensionTerminal.Name }, constants_1.ExtensionLogMessage.TerminalCreated);
    return createdTerminal;
};
exports.ensureTerminal = ensureTerminal;
const showTerminal = (terminal) => {
    terminal.show();
};
exports.showTerminal = showTerminal;
const sendCommandToTerminal = (terminal, commandLine) => {
    terminal.sendText(commandLine, true);
    logger_1.log.info({ [constants_1.ExtensionLogMetadataKey.TerminalName]: constants_1.ExtensionTerminal.Name }, constants_1.ExtensionLogMessage.CommandExecuted);
};
exports.sendCommandToTerminal = sendCommandToTerminal;
//# sourceMappingURL=terminal.js.map