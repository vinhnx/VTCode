"use strict";
Object.defineProperty(exports, "__esModule", { value: true });
exports.ExtensionDefaultValue = exports.ExtensionConfigKey = exports.ExtensionCommand = void 0;
var ExtensionCommand;
(function (ExtensionCommand) {
    ExtensionCommand["ShowGreeting"] = "vtcode.showGreeting";
})(ExtensionCommand || (exports.ExtensionCommand = ExtensionCommand = {}));
var ExtensionConfigKey;
(function (ExtensionConfigKey) {
    ExtensionConfigKey["ActivationMessage"] = "VT_EXTENSION_ACTIVATION_MESSAGE";
    ExtensionConfigKey["GreetingMessage"] = "VT_EXTENSION_GREETING_MESSAGE";
})(ExtensionConfigKey || (exports.ExtensionConfigKey = ExtensionConfigKey = {}));
var ExtensionDefaultValue;
(function (ExtensionDefaultValue) {
    ExtensionDefaultValue["ActivationMessage"] = "VT Code extension activated.";
    ExtensionDefaultValue["GreetingMessage"] = "Welcome to the VT Code VS Code extension.";
})(ExtensionDefaultValue || (exports.ExtensionDefaultValue = ExtensionDefaultValue = {}));
//# sourceMappingURL=constants.js.map