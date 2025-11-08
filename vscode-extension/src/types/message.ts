/**
 * Enhanced message interface with metadata and state tracking
 */
export interface ChatMessage {
    /** Unique message identifier */
    readonly id: string;
    /** Message role in the conversation */
    readonly role: 'user' | 'assistant' | 'system' | 'tool' | 'error';
    /** Message content */
    readonly content: string;
    /** Message timestamp */
    readonly timestamp: number;
    /** Optional metadata */
    readonly metadata?: {
        /** Model used for generation */
        model?: string;
        /** Token usage information */
        tokens?: {
            input: number;
            output: number;
            total?: number;
        };
        /** Generation duration in milliseconds */
        duration?: number;
        /** Participant that provided context */
        participantId?: string;
        /** Tool calls made during generation */
        toolCalls?: ToolCall[];
        /** Tool execution results */
        toolResults?: ToolResult[];
    };
    /** Message state */
    readonly state: 'pending' | 'complete' | 'error';
    /** Error information if state is 'error' */
    readonly error?: {
        code: string;
        message: string;
        details?: unknown;
    };
}

/**
 * Tool call information
 */
export interface ToolCall {
    /** Tool name */
    readonly name: string;
    /** Tool arguments */
    readonly arguments: Record<string, unknown>;
    /** Tool call ID */
    readonly id?: string;
}

/**
 * Tool execution result
 */
export interface ToolResult {
    /** Tool name */
    readonly name: string;
    /** Execution result */
    readonly result: unknown;
    /** Exit code if applicable */
    readonly exitCode?: number;
    /** Execution status */
    readonly status: 'success' | 'error' | 'cancelled';
}

/**
 * Conversation metadata
 */
export interface ConversationMetadata {
    /** Conversation title */
    readonly title?: string;
    /** Creation timestamp */
    readonly createdAt: number;
    /** Last update timestamp */
    readonly updatedAt: number;
    /** Total token usage */
    readonly totalTokens?: {
        input: number;
        output: number;
    };
    /** Participant IDs used in conversation */
    readonly participants?: string[];
}

/**
 * Conversation interface
 */
export interface Conversation {
    /** Conversation ID */
    readonly id: string;
    /** Conversation messages */
    readonly messages: ChatMessage[];
    /** Conversation metadata */
    readonly metadata: ConversationMetadata;
}