'use strict';

const pino = require('pino');

const LoggerEnvironmentVariable = Object.freeze({
    Environment: 'NODE_ENV',
    Level: 'VT_CODE_LOG_LEVEL'
});

const LoggerDefaultValue = Object.freeze({
    Level: 'info',
    Name: 'vtcode-extension'
});

const LoggerOptionKey = Object.freeze({
    Base: 'base',
    Level: 'level',
    Name: 'name',
    Options: 'options',
    Target: 'target',
    Transport: 'transport'
});

const LoggerOptionValue = Object.freeze({
    Colorize: 'colorize',
    Development: 'development',
    PrettyTarget: 'pino-pretty'
});

const loggerOptions = {};
loggerOptions[LoggerOptionKey.Level] = process.env[LoggerEnvironmentVariable.Level] || LoggerDefaultValue.Level;
loggerOptions[LoggerOptionKey.Base] = {};
loggerOptions[LoggerOptionKey.Base][LoggerOptionKey.Name] = LoggerDefaultValue.Name;

if (process.env[LoggerEnvironmentVariable.Environment] === LoggerOptionValue.Development) {
    loggerOptions[LoggerOptionKey.Transport] = {};
    loggerOptions[LoggerOptionKey.Transport][LoggerOptionKey.Target] = LoggerOptionValue.PrettyTarget;
    loggerOptions[LoggerOptionKey.Transport][LoggerOptionKey.Options] = {};
    loggerOptions[LoggerOptionKey.Transport][LoggerOptionKey.Options][LoggerOptionValue.Colorize] = true;
}

const log = pino(loggerOptions);

module.exports = {
    log
};
