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
const terminal_1 = require("./terminal");
const sanitizeValue = (value) => {
    if (!value) {
        return undefined;
    }
    const trimmedValue = value.trim();
    return trimmedValue.length > 0 ? trimmedValue : undefined;
};
const resolveExecutable = (configuration) => {
    const environmentValue = sanitizeValue(process.env[constants_1.ExtensionEnvironmentVariable.Executable]);
    if (environmentValue) {
        return environmentValue;
    }
    const configurationValue = sanitizeValue(configuration.get(constants_1.ExtensionConfigurationKey.Executable));
    if (configurationValue) {
        return configurationValue;
    }
    return constants_1.ExtensionDefaultValue.Executable;
};
const resolveArguments = (configuration) => {
    const environmentValue = sanitizeValue(process.env[constants_1.ExtensionEnvironmentVariable.Arguments]);
    if (environmentValue) {
        return environmentValue;
    }
    const configurationValue = sanitizeValue(configuration.get(constants_1.ExtensionConfigurationKey.Arguments));
    if (configurationValue) {
        return configurationValue;
    }
    return sanitizeValue(constants_1.ExtensionDefaultValue.Arguments);
};
const createCommandLine = (command, args) => {
    if (!args) {
        return command;
    }
    return [command, args].join(constants_1.ExtensionStringSymbol.Space);
};
const activate = (context) => {
    logger_1.log.info({}, constants_1.ExtensionLogMessage.Activation);
    const commandRegistration = vscode.commands.registerCommand(constants_1.ExtensionCommand.RunAgent, async () => {
        const configuration = vscode.workspace.getConfiguration(constants_1.ExtensionConfigurationSection.Root);
        const executable = resolveExecutable(configuration);
        const argumentString = resolveArguments(configuration);
        logger_1.log.info({
            [constants_1.ExtensionLogMetadataKey.Command]: executable,
            [constants_1.ExtensionLogMetadataKey.Arguments]: argumentString || constants_1.ExtensionLogMetadataValue.Empty
        }, constants_1.ExtensionLogMessage.CommandInvoked);
        const terminal = (0, terminal_1.ensureTerminal)();
        (0, terminal_1.showTerminal)(terminal);
        const commandLine = createCommandLine(executable, argumentString);
        (0, terminal_1.sendCommandToTerminal)(terminal, commandLine);
    });
    logger_1.log.info({}, constants_1.ExtensionLogMessage.CommandRegistered);
    context.subscriptions.push(commandRegistration);
};
exports.activate = activate;
const deactivate = () => {
    logger_1.log.info({}, constants_1.ExtensionLogMessage.Deactivation);
};
exports.deactivate = deactivate;
//# sourceMappingURL=extension.js.map