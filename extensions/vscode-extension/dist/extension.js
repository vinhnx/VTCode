"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.deactivate = exports.activate = void 0;
const vscode = __importStar(require("vscode"));
const logger_1 = require("@repo/shared/lib/logger");
const constants_1 = require("./constants");
const readEnvironmentValue = (key, fallback) => {
    const envValue = process.env[key];
    if (envValue && envValue.trim().length > 0) {
        return envValue;
    }
    return fallback;
};
const getWorkspaceFolder = () => {
    const folders = vscode.workspace.workspaceFolders;
    if (!folders || folders.length === 0) {
        return undefined;
    }
    return folders[0];
};
const disposeExistingTerminal = (terminalName) => {
    const existingTerminal = vscode.window.terminals.find((terminal) => {
        return terminal.name === terminalName;
    });
    if (existingTerminal) {
        logger_1.log.info({ terminalName }, constants_1.ExtensionLogMessage.DisposingTerminal);
        existingTerminal.dispose();
    }
};
const launchChatTerminal = async () => {
    const workspaceFolder = getWorkspaceFolder();
    if (!workspaceFolder) {
        logger_1.log.error({}, constants_1.ExtensionLogMessage.MissingWorkspace);
        void vscode.window.showErrorMessage(constants_1.ExtensionNotificationMessage.WorkspaceMissing);
        return;
    }
    const workspacePath = workspaceFolder.uri.fsPath;
    const binaryPath = readEnvironmentValue(constants_1.ExtensionConfigKey.BinaryPath, constants_1.ExtensionDefaultValue.BinaryPath);
    const terminalName = readEnvironmentValue(constants_1.ExtensionConfigKey.TerminalName, constants_1.ExtensionDefaultValue.TerminalName);
    disposeExistingTerminal(terminalName);
    const terminalOptions = {
        name: terminalName,
        shellPath: binaryPath,
        shellArgs: [constants_1.ExtensionCliArgument.Chat],
        cwd: workspacePath,
        env: {
            [constants_1.ExtensionEnvVar.WorkspaceDir]: workspacePath
        }
    };
    logger_1.log.info({
        binaryPath,
        command: constants_1.ExtensionCommand.StartChat,
        workspacePath,
        terminalName
    }, constants_1.ExtensionLogMessage.LaunchingTerminal);
    const terminal = vscode.window.createTerminal(terminalOptions);
    terminal.show();
    void vscode.window.showInformationMessage(constants_1.ExtensionNotificationMessage.ChatLaunched);
};
const registerChatCommand = (context) => {
    const command = vscode.commands.registerCommand(constants_1.ExtensionCommand.StartChat, () => {
        launchChatTerminal().catch((error) => {
            const message = error instanceof Error ? error.message : String(error);
            logger_1.log.error({ error: message }, constants_1.ExtensionLogMessage.ChatFailed);
            void vscode.window.showErrorMessage(constants_1.ExtensionLogMessage.ChatFailed);
        });
    });
    context.subscriptions.push(command);
};
const activate = (context) => {
    const activationMessage = readEnvironmentValue(constants_1.ExtensionConfigKey.ActivationMessage, constants_1.ExtensionDefaultValue.ActivationMessage);
    logger_1.log.info({}, constants_1.ExtensionLogMessage.ActivationComplete);
    registerChatCommand(context);
    void vscode.window.showInformationMessage(activationMessage);
};
exports.activate = activate;
const deactivate = () => {
    logger_1.log.info({}, constants_1.ExtensionLogMessage.Deactivated);
};
exports.deactivate = deactivate;
//# sourceMappingURL=extension.js.map