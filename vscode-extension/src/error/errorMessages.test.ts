import { describe, it, expect } from "vitest";
import {
	getErrorMessage,
	formatErrorMessage,
	isErrorRetryable,
} from "./errorMessages";

describe("errorMessages", () => {
	describe("getErrorMessage", () => {
		it("should return known error message by code", () => {
			const msg = getErrorMessage("NETWORK_TIMEOUT");
			expect(msg.title).toBe("Network request timed out");
			expect(msg.retryable).toBe(true);
		});

		it("should infer network timeout from error message", () => {
			const msg = getErrorMessage(undefined, "Request timed out");
			expect(msg.title).toContain("timeout");
		});

		it("should infer rate limit error from status code", () => {
			const msg = getErrorMessage(undefined, "Error 429: Too Many Requests");
			expect(msg.title).toContain("Rate limit");
		});

		it("should infer API key error", () => {
			const msg = getErrorMessage(undefined, "401 Unauthorized");
			expect(msg.title).toContain("API key");
		});

		it("should infer token limit error", () => {
			const msg = getErrorMessage(undefined, "Token limit exceeded");
			expect(msg.title).toContain("Context");
		});

		it("should handle Error objects", () => {
			const error = new Error("File not found");
			const msg = getErrorMessage(undefined, error);
			expect(msg.title).toContain("File not found");
		});

		it("should return default error for unknown codes", () => {
			const msg = getErrorMessage("UNKNOWN_ERROR");
			expect(msg.title).toBe("An error occurred");
		});

		it("should return default error for missing input", () => {
			const msg = getErrorMessage();
			expect(msg.title).toBe("An error occurred");
		});

		it("should have suggestion for retryable errors", () => {
			const msg = getErrorMessage("NETWORK_TIMEOUT");
			expect(msg.suggestion).toBeDefined();
			expect(msg.suggestion).toContain("try again");
		});

		it("should have documentation link for config errors", () => {
			const msg = getErrorMessage("CONFIG_ERROR");
			expect(msg.documentationLink).toBeDefined();
		});
	});

	describe("formatErrorMessage", () => {
		it("should format error message with title and description", () => {
			const formatted = formatErrorMessage("NETWORK_TIMEOUT");
			expect(formatted).toContain("⤫ ");
			expect(formatted).toContain("Network request timed out");
			expect(formatted).toContain("timed out");
		});

		it("should include suggestion when available", () => {
			const formatted = formatErrorMessage("RATE_LIMITED");
			expect(formatted).toContain("**Suggestion:**");
		});

		it("should include documentation link when available", () => {
			const formatted = formatErrorMessage("CONFIG_ERROR");
			expect(formatted).toContain("[📖 Learn more]");
			expect(formatted).toContain("CONFIGURATION.md");
		});

		it("should handle custom error messages", () => {
			const formatted = formatErrorMessage(
				undefined,
				"Custom error message"
			);
			expect(formatted).toContain("Custom error message");
		});

		it("should have proper formatting", () => {
			const formatted = formatErrorMessage("NETWORK_ERROR");
			expect(formatted).toContain("\n");
			expect(formatted).startsWith("⤫ ");
		});
	});

	describe("isErrorRetryable", () => {
		it("should return true for retryable errors", () => {
			expect(isErrorRetryable("NETWORK_TIMEOUT")).toBe(true);
			expect(isErrorRetryable("RATE_LIMITED")).toBe(true);
			expect(isErrorRetryable("TOOL_EXECUTION_FAILED")).toBe(true);
		});

		it("should return false for non-retryable errors", () => {
			expect(isErrorRetryable("INVALID_API_KEY")).toBe(false);
			expect(isErrorRetryable("CONFIG_ERROR")).toBe(false);
			expect(isErrorRetryable("TOOL_NOT_FOUND")).toBe(false);
		});

		it("should infer retryability from error message", () => {
			expect(isErrorRetryable(undefined, "timeout")).toBe(true);
			expect(isErrorRetryable(undefined, "API key invalid")).toBe(false);
		});

		it("should default to retryable for unknown errors", () => {
			expect(isErrorRetryable("UNKNOWN")).toBe(true);
		});
	});

	describe("error categories", () => {
		it("should have network errors", () => {
			const timeout = getErrorMessage("NETWORK_TIMEOUT");
			const error = getErrorMessage("NETWORK_ERROR");
			expect(timeout.retryable).toBe(true);
			expect(error.retryable).toBe(true);
		});

		it("should have API/model errors", () => {
			const rateLimit = getErrorMessage("RATE_LIMITED");
			const overloaded = getErrorMessage("MODEL_OVERLOADED");
			expect(rateLimit.retryable).toBe(true);
			expect(overloaded.retryable).toBe(true);
		});

		it("should have token/context errors", () => {
			const tokenLimit = getErrorMessage("TOKEN_LIMIT_EXCEEDED");
			const contextLarge = getErrorMessage("CONTEXT_TOO_LARGE");
			expect(tokenLimit.suggestion).toBeDefined();
			expect(contextLarge.suggestion).toBeDefined();
		});

		it("should have tool execution errors", () => {
			const failed = getErrorMessage("TOOL_EXECUTION_FAILED");
			const notFound = getErrorMessage("TOOL_NOT_FOUND");
			expect(failed.retryable).toBe(true);
			expect(notFound.retryable).toBe(false);
		});

		it("should have workspace errors", () => {
			const trusted = getErrorMessage("WORKSPACE_NOT_TRUSTED");
			const notFound = getErrorMessage("FILE_NOT_FOUND");
			expect(trusted.suggestion).toContain("Trust");
			expect(notFound.suggestion).toContain("file path");
		});

		it("should have MCP errors", () => {
			const serverError = getErrorMessage("MCP_SERVER_ERROR");
			const disconnected = getErrorMessage("MCP_DISCONNECTED");
			expect(serverError.retryable).toBe(true);
			expect(disconnected.retryable).toBe(true);
		});
	});

	describe("error message consistency", () => {
		const errorCodes = [
			"NETWORK_TIMEOUT",
			"NETWORK_ERROR",
			"RATE_LIMITED",
			"INVALID_API_KEY",
			"MODEL_OVERLOADED",
			"TOKEN_LIMIT_EXCEEDED",
			"TOOL_EXECUTION_FAILED",
			"WORKSPACE_NOT_TRUSTED",
			"CONFIG_ERROR",
			"MCP_SERVER_ERROR",
		];

		errorCodes.forEach((code) => {
			it(`should have complete message for ${code}`, () => {
				const msg = getErrorMessage(code);
				expect(msg.title).toBeDefined();
				expect(msg.title.length).toBeGreaterThan(0);
				expect(msg.description).toBeDefined();
				expect(msg.description.length).toBeGreaterThan(0);
			});
		});
	});
});
