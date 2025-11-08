import { describe, it, expect, beforeEach, vi } from "vitest";
import { StatusIndicator, formatMetrics } from "./statusIndicator";

describe("StatusIndicator", () => {
	let indicator: StatusIndicator;
	let updateCallback: ReturnType<typeof vi.fn>;

	beforeEach(() => {
		updateCallback = vi.fn();
		indicator = new StatusIndicator(updateCallback);
	});

	it("should initialize with idle status", () => {
		const state = indicator.getState();
		expect(state.status).toBe("idle");
	});

	it("should update status to thinking", () => {
		indicator.setThinking(true, "Processing...");
		const state = indicator.getState();
		expect(state.status).toBe("thinking");
		expect(state.message).toBe("Processing...");
		expect(updateCallback).toHaveBeenCalled();
	});

	it("should reset thinking status", () => {
		indicator.setThinking(true);
		indicator.setThinking(false);
		const state = indicator.getState();
		expect(state.status).toBe("idle");
	});

	it("should update status to streaming", () => {
		indicator.setStreaming(true, 50, 100);
		const state = indicator.getState();
		expect(state.status).toBe("streaming");
		expect(state.progress).toEqual({ current: 50, total: 100 });
		expect(state.message).toBe("Streaming response...");
	});

	it("should update status to executing", () => {
		indicator.setExecuting(true, "ls_files", 1, 5);
		const state = indicator.getState();
		expect(state.status).toBe("executing");
		expect(state.message).toContain("Executing ls_files");
		expect(state.progress).toEqual({ current: 1, total: 5 });
	});

	it("should set error status", () => {
		indicator.setError("Something went wrong");
		const state = indicator.getState();
		expect(state.status).toBe("error");
		expect(state.message).toBe("Something went wrong");
	});

	it("should update metrics", () => {
		indicator.setMetrics({
			tokensUsed: 150,
			modelName: "gpt-4",
			elapsedTime: 2500,
		});
		const state = indicator.getState();
		expect(state.metrics?.tokensUsed).toBe(150);
		expect(state.metrics?.modelName).toBe("gpt-4");
		expect(state.metrics?.elapsedTime).toBe(2500);
	});

	it("should format status correctly", () => {
		indicator.setThinking(true);
		indicator.setMetrics({
			tokensUsed: 100,
			modelName: "gpt-4",
		});

		const formatted = indicator.formatStatus();
		expect(formatted).toContain("Thinking");
		expect(formatted).toContain("gpt-4");
	});

	it("should track elapsed time", (done) => {
		indicator.setThinking(true);
		setTimeout(() => {
			const elapsed = indicator.getElapsedTime();
			expect(elapsed).toBeGreaterThan(0);
			done();
		}, 100);
	});

	it("should return correct indicator class", () => {
		indicator.updateState({ status: "streaming" });
		expect(indicator.getIndicatorClass()).toBe("status-streaming");
	});

	it("should return correct dot state", () => {
		indicator.updateState({ status: "thinking" });
		expect(indicator.getDotState()).toBe("active");

		indicator.updateState({ status: "error" });
		expect(indicator.getDotState()).toBe("error");

		indicator.updateState({ status: "idle" });
		expect(indicator.getDotState()).toBe("idle");
	});

	it("should reset state", () => {
		indicator.setThinking(true);
		indicator.setMetrics({ tokensUsed: 100 });
		indicator.reset();

		const state = indicator.getState();
		expect(state.status).toBe("idle");
		expect(state.metrics).toBeUndefined();
	});

	it("should clear metrics", () => {
		indicator.setMetrics({ tokensUsed: 100 });
		indicator.clearMetrics();

		const state = indicator.getState();
		expect(state.metrics).toBeUndefined();
	});
});

describe("formatMetrics", () => {
	it("should format elapsed time correctly", () => {
		const formatted = formatMetrics({ elapsedTime: 1500 });
		expect(formatted).toContain("1.5s");
	});

	it("should format tokens correctly", () => {
		const formatted = formatMetrics({ tokensUsed: 250 });
		expect(formatted).toContain("250 tokens");
	});

	it("should format model name correctly", () => {
		const formatted = formatMetrics({ modelName: "gpt-4" });
		expect(formatted).toContain("gpt-4");
	});

	it("should format participant name correctly", () => {
		const formatted = formatMetrics({ participantName: "workspace" });
		expect(formatted).toContain("@workspace");
	});

	it("should format all metrics together", () => {
		const formatted = formatMetrics({
			elapsedTime: 2500,
			tokensUsed: 350,
			modelName: "gpt-4",
			participantName: "code",
		});

		expect(formatted).toContain("2.5s");
		expect(formatted).toContain("350 tokens");
		expect(formatted).toContain("gpt-4");
		expect(formatted).toContain("@code");
		expect(formatted).toContain("|");
	});

	it("should handle empty metrics", () => {
		const formatted = formatMetrics({});
		expect(formatted).toBe("");
	});
});
