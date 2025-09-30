"use strict";
var __importDefault = (this && this.__importDefault) || function (mod) {
    return (mod && mod.__esModule) ? mod : { "default": mod };
};
Object.defineProperty(exports, "__esModule", { value: true });
exports.log = exports.maskPII = void 0;
const pino_1 = __importDefault(require("pino"));
const redactPaths = [
    '*.apiKey',
    '*.api_key',
    '*.token',
    '*.password',
    '*.email',
    '*.authorization',
    '*.Authorization'
];
const maskPII = (value) => {
    if (value.length <= 3) {
        return '*'.repeat(value.length);
    }
    if (value.includes('@')) {
        const [localPart, domain] = value.split('@');
        const visibleLocal = localPart.slice(0, Math.min(2, localPart.length));
        return `${visibleLocal}***@${domain}`;
    }
    if (value.length <= 6) {
        return `${value[0]}***${value[value.length - 1]}`;
    }
    return `${value.slice(0, 3)}***${value.slice(-3)}`;
};
exports.maskPII = maskPII;
const maskValue = (value) => {
    if (typeof value === 'string') {
        return (0, exports.maskPII)(value);
    }
    if (Array.isArray(value)) {
        return value.map((item) => maskValue(item));
    }
    if (value && typeof value === 'object') {
        const entries = Object.entries(value).map(([key, val]) => {
            return [key, maskValue(val)];
        });
        return Object.fromEntries(entries);
    }
    return value;
};
const sanitizeMetadata = (metadata) => {
    return maskValue(metadata);
};
const options = {
    level: process.env.VT_EXTENSION_LOG_LEVEL ?? 'info',
    redact: {
        paths: redactPaths,
        censor: '[redacted]'
    },
    base: undefined
};
const baseLogger = (0, pino_1.default)(options);
const buildLogger = () => {
    return {
        info(metadata, message) {
            baseLogger.info(sanitizeMetadata(metadata), (0, exports.maskPII)(message));
        },
        error(metadata, message) {
            baseLogger.error(sanitizeMetadata(metadata), (0, exports.maskPII)(message));
        },
        warn(metadata, message) {
            baseLogger.warn(sanitizeMetadata(metadata), (0, exports.maskPII)(message));
        },
        debug(metadata, message) {
            baseLogger.debug(sanitizeMetadata(metadata), (0, exports.maskPII)(message));
        }
    };
};
exports.log = buildLogger();
//# sourceMappingURL=logger.js.map