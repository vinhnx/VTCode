"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ExtensionStringSymbol = exports.ExtensionLogMetadataValue = exports.ExtensionLogMetadataKey = exports.ExtensionLogMessage = exports.ExtensionTerminal = exports.ExtensionDefaultValue = exports.ExtensionEnvironmentVariable = exports.ExtensionConfigurationKey = exports.ExtensionConfigurationSection = exports.ExtensionCommand = void 0;
var ExtensionCommand;
(function (ExtensionCommand) {
    ExtensionCommand["RunAgent"] = "vtcode.run";
})(ExtensionCommand || (exports.ExtensionCommand = ExtensionCommand = {}));
var ExtensionConfigurationSection;
(function (ExtensionConfigurationSection) {
    ExtensionConfigurationSection["Root"] = "vtcode";
})(ExtensionConfigurationSection || (exports.ExtensionConfigurationSection = ExtensionConfigurationSection = {}));
var ExtensionConfigurationKey;
(function (ExtensionConfigurationKey) {
    ExtensionConfigurationKey["Executable"] = "executable";
    ExtensionConfigurationKey["Arguments"] = "arguments";
})(ExtensionConfigurationKey || (exports.ExtensionConfigurationKey = ExtensionConfigurationKey = {}));
var ExtensionEnvironmentVariable;
(function (ExtensionEnvironmentVariable) {
    ExtensionEnvironmentVariable["Executable"] = "VTCODE_EXECUTABLE";
    ExtensionEnvironmentVariable["Arguments"] = "VTCODE_EXECUTABLE_ARGS";
})(ExtensionEnvironmentVariable || (exports.ExtensionEnvironmentVariable = ExtensionEnvironmentVariable = {}));
var ExtensionDefaultValue;
(function (ExtensionDefaultValue) {
    ExtensionDefaultValue["Executable"] = "vtcode";
    ExtensionDefaultValue["Arguments"] = "";
})(ExtensionDefaultValue || (exports.ExtensionDefaultValue = ExtensionDefaultValue = {}));
var ExtensionTerminal;
(function (ExtensionTerminal) {
    ExtensionTerminal["Name"] = "VT Code";
})(ExtensionTerminal || (exports.ExtensionTerminal = ExtensionTerminal = {}));
var ExtensionLogMessage;
(function (ExtensionLogMessage) {
    ExtensionLogMessage["Activation"] = "Activating VT Code extension";
    ExtensionLogMessage["CommandRegistered"] = "Registered VT Code command";
    ExtensionLogMessage["CommandInvoked"] = "Launching VT Code terminal command";
    ExtensionLogMessage["TerminalCreated"] = "Created VT Code terminal instance";
    ExtensionLogMessage["TerminalReused"] = "Reusing existing VT Code terminal instance";
    ExtensionLogMessage["CommandExecuted"] = "Sent command to VT Code terminal";
    ExtensionLogMessage["TerminalMissing"] = "No terminal instance available for VT Code";
    ExtensionLogMessage["Deactivation"] = "Deactivating VT Code extension";
})(ExtensionLogMessage || (exports.ExtensionLogMessage = ExtensionLogMessage = {}));
var ExtensionLogMetadataKey;
(function (ExtensionLogMetadataKey) {
    ExtensionLogMetadataKey["Command"] = "command";
    ExtensionLogMetadataKey["Arguments"] = "arguments";
    ExtensionLogMetadataKey["TerminalName"] = "terminalName";
})(ExtensionLogMetadataKey || (exports.ExtensionLogMetadataKey = ExtensionLogMetadataKey = {}));
var ExtensionLogMetadataValue;
(function (ExtensionLogMetadataValue) {
    ExtensionLogMetadataValue["Empty"] = "empty";
})(ExtensionLogMetadataValue || (exports.ExtensionLogMetadataValue = ExtensionLogMetadataValue = {}));
var ExtensionStringSymbol;
(function (ExtensionStringSymbol) {
    ExtensionStringSymbol["Space"] = " ";
})(ExtensionStringSymbol || (exports.ExtensionStringSymbol = ExtensionStringSymbol = {}));
//# sourceMappingURL=constants.js.map