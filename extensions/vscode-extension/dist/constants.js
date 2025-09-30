"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ExtensionLogMessage = exports.ExtensionNotificationMessage = exports.ExtensionEnvVar = exports.ExtensionCliArgument = exports.ExtensionDefaultValue = exports.ExtensionConfigKey = exports.ExtensionCommand = void 0;
var ExtensionCommand;
(function (ExtensionCommand) {
    ExtensionCommand["StartChat"] = "vtcode.startChat";
})(ExtensionCommand || (exports.ExtensionCommand = ExtensionCommand = {}));
var ExtensionConfigKey;
(function (ExtensionConfigKey) {
    ExtensionConfigKey["ActivationMessage"] = "VT_EXTENSION_ACTIVATION_MESSAGE";
    ExtensionConfigKey["BinaryPath"] = "VT_EXTENSION_VTCODE_BINARY";
    ExtensionConfigKey["TerminalName"] = "VT_EXTENSION_TERMINAL_NAME";
})(ExtensionConfigKey || (exports.ExtensionConfigKey = ExtensionConfigKey = {}));
var ExtensionDefaultValue;
(function (ExtensionDefaultValue) {
    ExtensionDefaultValue["ActivationMessage"] = "VT Code extension activated.";
    ExtensionDefaultValue["BinaryPath"] = "vtcode";
    ExtensionDefaultValue["TerminalName"] = "VT Code Chat";
})(ExtensionDefaultValue || (exports.ExtensionDefaultValue = ExtensionDefaultValue = {}));
var ExtensionCliArgument;
(function (ExtensionCliArgument) {
    ExtensionCliArgument["Chat"] = "chat";
})(ExtensionCliArgument || (exports.ExtensionCliArgument = ExtensionCliArgument = {}));
var ExtensionEnvVar;
(function (ExtensionEnvVar) {
    ExtensionEnvVar["WorkspaceDir"] = "WORKSPACE_DIR";
    ExtensionEnvVar["ActiveDocument"] = "VT_EXTENSION_ACTIVE_DOCUMENT";
})(ExtensionEnvVar || (exports.ExtensionEnvVar = ExtensionEnvVar = {}));
var ExtensionNotificationMessage;
(function (ExtensionNotificationMessage) {
    ExtensionNotificationMessage["ChatLaunched"] = "VT Code chat session launched in the integrated terminal.";
    ExtensionNotificationMessage["WorkspaceMissing"] = "Open a workspace folder to start the VT Code chat loop.";
})(ExtensionNotificationMessage || (exports.ExtensionNotificationMessage = ExtensionNotificationMessage = {}));
var ExtensionLogMessage;
(function (ExtensionLogMessage) {
    ExtensionLogMessage["ActivationComplete"] = "VT Code extension activated.";
    ExtensionLogMessage["ChatFailed"] = "Failed to start VT Code chat terminal.";
    ExtensionLogMessage["Deactivated"] = "VT Code extension deactivated.";
    ExtensionLogMessage["DisposingTerminal"] = "Disposing existing VT Code chat terminal.";
    ExtensionLogMessage["LaunchingTerminal"] = "Creating VT Code chat terminal.";
    ExtensionLogMessage["MissingWorkspace"] = "Unable to locate a workspace folder for the VT Code chat loop.";
    ExtensionLogMessage["ActiveDocumentAttached"] = "Forwarding active document context to VT Code chat.";
    ExtensionLogMessage["ActiveDocumentOutsideWorkspace"] = "Active document is outside the workspace; skipping context.";
})(ExtensionLogMessage || (exports.ExtensionLogMessage = ExtensionLogMessage = {}));
//# sourceMappingURL=constants.js.map