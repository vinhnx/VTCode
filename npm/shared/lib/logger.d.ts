export interface SharedLogger {
    info: (...parameters: unknown[]) => void;
    error: (...parameters: unknown[]) => void;
    warn: (...parameters: unknown[]) => void;
    debug: (...parameters: unknown[]) => void;
}

export declare const log: SharedLogger;
