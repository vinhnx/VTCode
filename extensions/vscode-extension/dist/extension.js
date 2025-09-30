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
const constants_1 = require("./constants");
const readEnvironmentValue = (key, fallback) => {
    const envValue = process.env[key];
    if (envValue && envValue.trim().length > 0) {
        return envValue;
    }
    return fallback;
};
const getActivationMessage = () => {
    return readEnvironmentValue(constants_1.ExtensionConfigKey.ActivationMessage, constants_1.ExtensionDefaultValue.ActivationMessage);
};
const getGreetingMessage = () => {
    return readEnvironmentValue(constants_1.ExtensionConfigKey.GreetingMessage, constants_1.ExtensionDefaultValue.GreetingMessage);
};
const activate = (context) => {
    const activationMessage = getActivationMessage();
    void vscode.window.showInformationMessage(activationMessage);
    const showGreetingCommand = vscode.commands.registerCommand(constants_1.ExtensionCommand.ShowGreeting, () => {
        const greetingMessage = getGreetingMessage();
        void vscode.window.showInformationMessage(greetingMessage);
    });
    context.subscriptions.push(showGreetingCommand);
};
exports.activate = activate;
const deactivate = () => {
    // No resources to dispose
};
exports.deactivate = deactivate;
//# sourceMappingURL=extension.js.map