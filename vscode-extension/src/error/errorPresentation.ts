/**
 * Error message presentation and user-friendly error formatting
 * Converts technical errors to helpful user messages with suggestions
 */

export interface ErrorPresentation {
    readonly title: string;
    readonly message: string;
    readonly suggestion?: string;
    readonly details?: string;
    readonly severity: "error" | "warning" | "info";
}

export class ErrorPresentationHandler {
    /**
     * Convert an error to a user-friendly presentation
     */
    public static format(error: Error | string): ErrorPresentation {
        const errorMessage = typeof error === "string" ? error : error.message;
        const errorStack = error instanceof Error ? error.stack : undefined;

        // Network errors
        if (errorMessage.includes("ECONNREFUSED")) {
            return {
                title: "Connection Failed",
                message:
                    "VTCode cannot connect to the backend service. The service may be starting up or encountered an issue.",
                suggestion: "Try again in a few moments. If the problem persists, restart the extension.",
                severity: "error",
            };
        }

        if (errorMessage.includes("ETIMEDOUT") || errorMessage.includes("timeout")) {
            return {
                title: "Request Timeout",
                message:
                    "The request took too long to complete. The network may be slow or the service is overloaded.",
                suggestion: "Try again with a simpler query, or wait a moment before retrying.",
                severity: "warning",
            };
        }

        if (errorMessage.includes("ENOTFOUND") || errorMessage.includes("DNS")) {
            return {
                title: "Network Unreachable",
                message:
                    "VTCode cannot reach the backend service. Check your internet connection and firewall settings.",
                suggestion: "Verify your network connection and check firewall rules.",
                severity: "error",
            };
        }

        // Token limit errors
        if (errorMessage.includes("token") && errorMessage.includes("limit")) {
            return {
                title: "Token Limit Exceeded",
                message:
                    "Your message or conversation context is too large for the current model. The AI ran out of tokens to process your request.",
                suggestion:
                    "Try a shorter message or start a new conversation. You can also simplify your context.",
                severity: "warning",
            };
        }

        // Rate limit errors
        if (errorMessage.includes("rate limit") || errorMessage.includes("429")) {
            return {
                title: "Rate Limited",
                message:
                    "You've sent too many requests in a short time. The service is temporarily throttling your requests.",
                suggestion: "Wait a moment and try again.",
                severity: "warning",
            };
        }

        // Authentication errors
        if (
            errorMessage.includes("unauthorized") ||
            errorMessage.includes("401") ||
            errorMessage.includes("invalid key")
        ) {
            return {
                title: "Authentication Failed",
                message:
                    "VTCode cannot authenticate with the service. Your API key or credentials may be invalid or expired.",
                suggestion:
                    "Check your configuration settings and ensure your API key is correct. See the documentation for setup instructions.",
                severity: "error",
            };
        }

        // Tool execution errors
        if (errorMessage.includes("tool") && errorMessage.includes("failed")) {
            return {
                title: "Tool Execution Failed",
                message:
                    "A tool that VTCode tried to use encountered an error. This might be a permission issue or an unexpected state.",
                suggestion:
                    "Review the tool output above for details. You may need to adjust permissions or workspace state.",
                severity: "error",
            };
        }

        // File not found errors
        if (errorMessage.includes("ENOENT") || errorMessage.includes("not found")) {
            return {
                title: "File Not Found",
                message: "VTCode tried to access a file that doesn't exist or has been deleted.",
                suggestion: "Check that the file still exists and the path is correct.",
                severity: "warning",
            };
        }

        // Permission errors
        if (errorMessage.includes("EACCES") || errorMessage.includes("permission denied")) {
            return {
                title: "Permission Denied",
                message: "VTCode doesn't have permission to access this resource.",
                suggestion:
                    "Check file permissions or workspace trust settings. You may need to grant additional permissions.",
                severity: "error",
            };
        }

        // JSON parse errors
        if (errorMessage.includes("JSON") || errorMessage.includes("parse")) {
            return {
                title: "Invalid Response Format",
                message:
                    "The response from the AI or a tool was in an unexpected format. This is usually a temporary issue.",
                suggestion: "Try your request again.",
                severity: "warning",
            };
        }

        // Default error
        return {
            title: "Unexpected Error",
            message: errorMessage,
            details: errorStack ? `Details: ${errorStack.split("\n")[1]?.trim()}` : undefined,
            suggestion: "Check the output channel for more details. Please report this if it continues.",
            severity: "error",
        };
    }

    /**
     * Format error for display in chat
     */
    public static formatForChat(error: Error | string): string {
        const presentation = this.format(error);
        let output = `**${presentation.title}**\n\n${presentation.message}`;

        if (presentation.suggestion) {
            output += `\n\n💡 **Suggestion:** ${presentation.suggestion}`;
        }

        if (presentation.details) {
            output += `\n\n${presentation.details}`;
        }

        return output;
    }

    /**
     * Get error context for logging/debugging
     */
    public static getContext(error: Error | string): Record<string, unknown> {
        const presentation = this.format(error);
        return {
            title: presentation.title,
            message: presentation.message,
            severity: presentation.severity,
            timestamp: new Date().toISOString(),
            originalError: typeof error === "string" ? error : error.message,
        };
    }
}

/**
 * Common error messages for various scenarios
 */
export const ERROR_MESSAGES = {
    NO_WORKSPACE: {
        title: "No Workspace Open",
        message: "Please open a workspace or folder to use VTCode.",
        severity: "warning" as const,
    },
    BACKEND_UNAVAILABLE: {
        title: "Backend Service Unavailable",
        message: "The VTCode backend service is not running.",
        suggestion: "Start the backend service or check the documentation.",
        severity: "error" as const,
    },
    CONFIGURATION_INVALID: {
        title: "Invalid Configuration",
        message: "Your VTCode configuration is invalid or incomplete.",
        suggestion: "Review your configuration file and fix any errors.",
        severity: "error" as const,
    },
    WORKSPACE_UNTRUSTED: {
        title: "Workspace Not Trusted",
        message: "VTCode requires a trusted workspace to function safely.",
        suggestion: "Trust this workspace in VS Code settings to use VTCode.",
        severity: "warning" as const,
    },
};
