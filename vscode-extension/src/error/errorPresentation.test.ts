/**
 * Unit tests for ErrorPresentationHandler
 */

import * as assert from "assert";
import { ErrorPresentationHandler, ERROR_MESSAGES } from "./errorPresentation";

describe("ErrorPresentationHandler", () => {
    it("should detect ECONNREFUSED errors", () => {
        const error = new Error("ECONNREFUSED: Connection refused");
        const presentation = ErrorPresentationHandler.format(error);

        assert.strictEqual(presentation.title, "Connection Failed");
        assert.ok(presentation.message.includes("backend"));
        assert.ok(presentation.suggestion);
        assert.strictEqual(presentation.severity, "error");
    });

    it("should detect timeout errors", () => {
        const error = new Error("Request timeout after 30000ms");
        const presentation = ErrorPresentationHandler.format(error);

        assert.strictEqual(presentation.title, "Request Timeout");
        assert.ok(presentation.severity === "warning");
    });

    it("should detect DNS/network errors", () => {
        const error = new Error("ENOTFOUND: Failed to resolve hostname");
        const presentation = ErrorPresentationHandler.format(error);

        assert.strictEqual(presentation.title, "Network Unreachable");
        assert.ok(presentation.severity === "error");
    });

    it("should detect token limit errors", () => {
        const error = new Error("This model max token limit is 4096");
        const presentation = ErrorPresentationHandler.format(error);

        assert.strictEqual(presentation.title, "Token Limit Exceeded");
        assert.ok(presentation.severity === "warning");
    });

    it("should detect rate limit errors", () => {
        const error = new Error("429: Too many requests - rate limit exceeded");
        const presentation = ErrorPresentationHandler.format(error);

        assert.strictEqual(presentation.title, "Rate Limited");
        assert.ok(presentation.severity === "warning");
    });

    it("should detect authentication errors", () => {
        const error = new Error("401 Unauthorized: Invalid API key");
        const presentation = ErrorPresentationHandler.format(error);

        assert.strictEqual(presentation.title, "Authentication Failed");
        assert.ok(presentation.severity === "error");
    });

    it("should detect file not found errors", () => {
        const error = new Error("ENOENT: no such file or directory");
        const presentation = ErrorPresentationHandler.format(error);

        assert.strictEqual(presentation.title, "File Not Found");
        assert.ok(presentation.severity === "warning");
    });

    it("should detect permission errors", () => {
        const error = new Error("EACCES: Permission denied");
        const presentation = ErrorPresentationHandler.format(error);

        assert.strictEqual(presentation.title, "Permission Denied");
        assert.ok(presentation.severity === "error");
    });

    it("should format error for chat display", () => {
        const error = new Error("ECONNREFUSED: Connection refused");
        const formatted = ErrorPresentationHandler.formatForChat(error);

        assert.ok(formatted.includes("Connection Failed"));
        assert.ok(formatted.includes("backend"));
        assert.ok(formatted.includes("💡 **Suggestion:**"));
    });

    it("should handle string errors", () => {
        const presentation = ErrorPresentationHandler.format("Something went wrong");
        assert.ok(presentation.title);
        assert.ok(presentation.message);
    });

    it("should provide context for logging", () => {
        const error = new Error("Test error");
        const context = ErrorPresentationHandler.getContext(error);

        assert.ok(context.title);
        assert.ok(context.message);
        assert.ok(context.severity);
        assert.ok(context.timestamp);
        assert.ok(context.originalError);
    });

    it("should use predefined error messages", () => {
        assert.strictEqual(ERROR_MESSAGES.NO_WORKSPACE.severity, "warning");
        assert.strictEqual(ERROR_MESSAGES.BACKEND_UNAVAILABLE.severity, "error");
        assert.strictEqual(ERROR_MESSAGES.CONFIGURATION_INVALID.severity, "error");
        assert.strictEqual(ERROR_MESSAGES.WORKSPACE_UNTRUSTED.severity, "warning");
    });

    it("should default to Unexpected Error for unknown errors", () => {
        const error = new Error("Unknown error type");
        const presentation = ErrorPresentationHandler.format(error);

        assert.strictEqual(presentation.title, "Unexpected Error");
        assert.ok(presentation.suggestion);
    });
});
