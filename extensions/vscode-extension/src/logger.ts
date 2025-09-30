import pino, { LoggerOptions } from 'pino';

type LogMetadata = Record<string, unknown>;

const redactPaths = [
    '*.apiKey',
    '*.api_key',
    '*.token',
    '*.password',
    '*.email',
    '*.authorization',
    '*.Authorization'
];

export const maskPII = (value: string): string => {
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

const maskValue = (value: unknown): unknown => {
    if (typeof value === 'string') {
        return maskPII(value);
    }

    if (Array.isArray(value)) {
        return value.map((item) => maskValue(item));
    }

    if (value && typeof value === 'object') {
        const entries = Object.entries(value as Record<string, unknown>).map(([key, val]) => {
            return [key, maskValue(val)];
        });
        return Object.fromEntries(entries);
    }

    return value;
};

const sanitizeMetadata = (metadata: LogMetadata): LogMetadata => {
    return maskValue(metadata) as LogMetadata;
};

const options: LoggerOptions = {
    level: process.env.VT_EXTENSION_LOG_LEVEL ?? 'info',
    redact: {
        paths: redactPaths,
        censor: '[redacted]'
    },
    base: undefined
};

const baseLogger = pino(options);

const buildLogger = () => {
    return {
        info(metadata: LogMetadata, message: string) {
            baseLogger.info(sanitizeMetadata(metadata), maskPII(message));
        },
        error(metadata: LogMetadata, message: string) {
            baseLogger.error(sanitizeMetadata(metadata), maskPII(message));
        },
        warn(metadata: LogMetadata, message: string) {
            baseLogger.warn(sanitizeMetadata(metadata), maskPII(message));
        },
        debug(metadata: LogMetadata, message: string) {
            baseLogger.debug(sanitizeMetadata(metadata), maskPII(message));
        }
    };
};

export const log = buildLogger();
