/**
 * ErrorMessages - User-friendly error explanations and recovery suggestions
 * Maps technical errors to helpful guidance
 */

export interface ErrorMessage {
	title: string;
	description: string;
	suggestion?: string;
	documentationLink?: string;
	retryable?: boolean;
}

const ERROR_MESSAGES: Record<string, ErrorMessage> = {
	// Network errors
	NETWORK_TIMEOUT: {
		title: "Network request timed out",
		description:
			"The request took longer than expected to complete. This can happen with slow connections or overloaded servers.",
		suggestion: "Try again. If the problem persists, check your internet connection.",
		retryable: true,
	},
	NETWORK_ERROR: {
		title: "Network connection error",
		description: "Unable to connect to the AI service. Check your internet connection and try again.",
		suggestion: "Verify your network connectivity and firewall settings.",
		retryable: true,
	},

	// API/Model errors
	RATE_LIMITED: {
		title: "Rate limit exceeded",
		description: "Too many requests have been sent. The service is temporarily throttling requests.",
		suggestion: "Wait a moment and try again. Consider spacing out requests.",
		retryable: true,
	},
	INVALID_API_KEY: {
		title: "API key configuration error",
		description: "The API key is missing, invalid, or expired.",
		suggestion:
			"Check your VTCode configuration and verify the API key is correct. See the troubleshooting guide.",
		documentationLink: "docs/TROUBLESHOOTING.md#api-key",
	},
	MODEL_OVERLOADED: {
		title: "AI model is temporarily unavailable",
		description: "The AI service is experiencing high demand.",
		suggestion: "Try again in a few moments. Use a faster model if available.",
		retryable: true,
	},

	// Token/context errors
	TOKEN_LIMIT_EXCEEDED: {
		title: "Context too large",
		description:
			"The request exceeds the maximum token limit for this model. This happens when the code or context is very large.",
		suggestion:
			"Reduce the context size by selecting less code or removing recent messages. Consider using a model with higher limits.",
		retryable: true,
	},
	CONTEXT_TOO_LARGE: {
		title: "Too much context",
		description: "The combined input is too large for the model to process.",
		suggestion: "Remove some files or messages from the context and try again.",
		retryable: true,
	},

	// Tool execution errors
	TOOL_EXECUTION_FAILED: {
		title: "Tool execution failed",
		description: "The requested tool encountered an error while executing.",
		suggestion: "Check the tool output for details. Some tools may require specific workspace setup.",
		retryable: true,
	},
	TOOL_NOT_FOUND: {
		title: "Tool not available",
		description: "The requested tool is not installed or not available in this context.",
		suggestion: "Verify the tool is properly configured in your VTCode setup.",
	},
	TOOL_PERMISSION_DENIED: {
		title: "Tool execution denied",
		description: "You need to approve this tool execution for security reasons.",
		suggestion: "Review the tool action and approve it to continue.",
	},

	// Workspace errors
	WORKSPACE_NOT_TRUSTED: {
		title: "Workspace not trusted",
		description: "The workspace must be trusted to execute certain operations.",
		suggestion: "Click 'Trust Workspace' in VS Code to enable full functionality.",
	},
	FILE_NOT_FOUND: {
		title: "File not found",
		description: "The requested file does not exist or cannot be accessed.",
		suggestion: "Verify the file path and permissions.",
	},
	WORKSPACE_ERROR: {
		title: "Workspace error",
		description: "An error occurred while accessing the workspace.",
		suggestion: "Check that the workspace is properly configured and files are accessible.",
		retryable: true,
	},

	// Configuration errors
	CONFIG_ERROR: {
		title: "Configuration error",
		description: "The VTCode configuration contains invalid settings.",
		suggestion: "Review your vtcode.toml file and fix any errors.",
		documentationLink: "docs/CONFIGURATION.md",
	},
	INVALID_MODEL: {
		title: "Invalid model specified",
		description: "The selected model is not recognized or not available.",
		suggestion: "Check your model configuration and select an available model.",
		documentationLink: "docs/MODELS.md",
	},

	// System errors
	INTERNAL_ERROR: {
		title: "Internal error occurred",
		description: "An unexpected error occurred in VTCode.",
		suggestion: "Check the output logs for details. Try restarting VS Code if the problem persists.",
		retryable: true,
	},
	OUT_OF_MEMORY: {
		title: "Out of memory",
		description: "VS Code ran out of memory while processing.",
		suggestion: "Close some files or reduce the context size. Consider restarting VS Code.",
	},

	// MCP errors
	MCP_SERVER_ERROR: {
		title: "MCP server error",
		description: "The MCP server encountered an error or is not responding.",
		suggestion: "Check that the MCP server is running and properly configured.",
		retryable: true,
	},
	MCP_DISCONNECTED: {
		title: "MCP server disconnected",
		description: "The connection to the MCP server was lost.",
		suggestion: "The connection will be re-established automatically. If the problem persists, restart the MCP server.",
		retryable: true,
	},
};

/**
 * Get user-friendly error message
 */
export function getErrorMessage(
	errorCode?: string,
	originalError?: Error | string
): ErrorMessage {
	// Check if it's a known error code
	if (errorCode && errorCode in ERROR_MESSAGES) {
		return ERROR_MESSAGES[errorCode];
	}

	// Try to infer from error message
	const errorStr = typeof originalError === "string" ? originalError : originalError?.message || "";

	if (errorStr.includes("timeout") || errorStr.includes("timed out")) {
		return ERROR_MESSAGES.NETWORK_TIMEOUT;
	}
	if (
		errorStr.includes("network") ||
		errorStr.includes("ECONNREFUSED") ||
		errorStr.includes("ENOTFOUND")
	) {
		return ERROR_MESSAGES.NETWORK_ERROR;
	}
	if (errorStr.includes("429") || errorStr.includes("rate limit")) {
		return ERROR_MESSAGES.RATE_LIMITED;
	}
	if (errorStr.includes("401") || errorStr.includes("api key") || errorStr.includes("unauthorized")) {
		return ERROR_MESSAGES.INVALID_API_KEY;
	}
	if (errorStr.includes("token") || errorStr.includes("context")) {
		return ERROR_MESSAGES.TOKEN_LIMIT_EXCEEDED;
	}
	if (errorStr.includes("ENOENT") || errorStr.includes("not found")) {
		return ERROR_MESSAGES.FILE_NOT_FOUND;
	}

	// Default error message
	return {
		title: "An error occurred",
		description: typeof originalError === "string" ? originalError : originalError?.message || "Unknown error",
		suggestion: "Check the output logs for more details or try again.",
		retryable: true,
	};
}

/**
 * Create a formatted error message for display
 */
export function formatErrorMessage(
	errorCode?: string,
	originalError?: Error | string
): string {
	const msg = getErrorMessage(errorCode, originalError);
	let output = `❌ ${msg.title}\n`;

	if (msg.description) {
		output += `\n${msg.description}\n`;
	}

	if (msg.suggestion) {
		output += `\n**Suggestion:** ${msg.suggestion}`;
	}

	if (msg.documentationLink) {
		output += `\n\n[📖 Learn more](${msg.documentationLink})`;
	}

	return output;
}

/**
 * Check if an error is retryable
 */
export function isErrorRetryable(errorCode?: string, originalError?: Error | string): boolean {
	const msg = getErrorMessage(errorCode, originalError);
	return msg.retryable === true;
}
