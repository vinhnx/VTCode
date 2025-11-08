/**
 * StatusIndicator - Enhanced status display for chat operations
 * Shows real-time information about streaming, tokens, model, and execution
 */

export interface StatusIndicatorState {
	status: "idle" | "thinking" | "streaming" | "executing" | "error";
	message?: string;
	progress?: {
		current: number;
		total: number;
	};
	metrics?: {
		elapsedTime?: number; // milliseconds
		tokensUsed?: number;
		modelName?: string;
		participantName?: string;
	};
}

export class StatusIndicator {
	private state: StatusIndicatorState = { status: "idle" };
	private startTime: number = 0;
	private updateCallback?: (state: StatusIndicatorState) => void;

	constructor(private onUpdate?: (state: StatusIndicatorState) => void) {
		this.updateCallback = onUpdate;
	}

	/**
	 * Update the status state
	 */
	public updateState(state: Partial<StatusIndicatorState>): void {
		this.state = { ...this.state, ...state };
		if (!this.startTime && state.status && state.status !== "idle") {
			this.startTime = Date.now();
		}
		if (state.status === "idle") {
			this.startTime = 0;
		}
		this.notifyUpdate();
	}

	/**
	 * Set thinking/processing status
	 */
	public setThinking(thinking: boolean, message?: string): void {
		this.updateState({
			status: thinking ? "thinking" : "idle",
			message: message || (thinking ? "Thinking..." : undefined),
		});
	}

	/**
	 * Set streaming status with progress
	 */
	public setStreaming(active: boolean, current?: number, total?: number): void {
		this.updateState({
			status: active ? "streaming" : "idle",
			progress: active && current !== undefined ? { current, total: total || 100 } : undefined,
			message: active ? "Streaming response..." : undefined,
		});
	}

	/**
	 * Set executing status (tool execution)
	 */
	public setExecuting(active: boolean, toolName?: string, current?: number, total?: number): void {
		this.updateState({
			status: active ? "executing" : "idle",
			message: active ? `Executing ${toolName || "tool"}${current && total ? ` (${current}/${total})` : ""}...` : undefined,
			progress: active && current !== undefined ? { current, total: total || 1 } : undefined,
		});
	}

	/**
	 * Set error status
	 */
	public setError(message: string): void {
		this.updateState({
			status: "error",
			message,
		});
	}

	/**
	 * Update metrics (tokens, elapsed time, etc.)
	 */
	public setMetrics(metrics: Partial<StatusIndicatorState["metrics"]>): void {
		if (!this.state.metrics) {
			this.state.metrics = {};
		}
		this.state.metrics = { ...this.state.metrics, ...metrics };
		this.notifyUpdate();
	}

	/**
	 * Get elapsed time since status started
	 */
	public getElapsedTime(): number {
		if (!this.startTime) return 0;
		return Math.floor((Date.now() - this.startTime) / 100) / 10; // Round to 0.1s
	}

	/**
	 * Get current state
	 */
	public getState(): Readonly<StatusIndicatorState> {
		return { ...this.state };
	}

	/**
	 * Format status for display
	 */
	public formatStatus(): string {
		const elapsed = this.getElapsedTime();
		const time = elapsed > 0 ? ` | ${elapsed.toFixed(1)}s` : "";
		const tokens = this.state.metrics?.tokensUsed ? ` | ${this.state.metrics.tokensUsed} tokens` : "";
		const model = this.state.metrics?.modelName ? ` | ${this.state.metrics.modelName}` : "";
		const progress = this.state.progress ? ` (${this.state.progress.current}/${this.state.progress.total})` : "";

		const statusText = {
			idle: "Ready",
			thinking: "Thinking",
			streaming: "Streaming",
			executing: "Executing",
			error: "Error",
		};

		return `${statusText[this.state.status]}${progress}${time}${tokens}${model}`;
	}

	/**
	 * Get status indicator HTML element properties
	 */
	public getIndicatorClass(): string {
		return `status-${this.state.status}`;
	}

	/**
	 * Get status dot state (for animation)
	 */
	public getDotState(): "idle" | "active" | "success" | "error" {
		switch (this.state.status) {
			case "error":
				return "error";
			case "idle":
				return "idle";
			case "thinking":
			case "streaming":
			case "executing":
				return "active";
			default:
				return "idle";
		}
	}

	/**
	 * Notify listeners of state changes
	 */
	private notifyUpdate(): void {
		if (this.updateCallback) {
			this.updateCallback(this.getState());
		}
	}

	/**
	 * Reset status to idle
	 */
	public reset(): void {
		this.state = { status: "idle" };
		this.startTime = 0;
		this.notifyUpdate();
	}

	/**
	 * Clear all metrics
	 */
	public clearMetrics(): void {
		this.state.metrics = undefined;
		this.notifyUpdate();
	}
}

/**
 * Helper to format metrics for display
 */
export function formatMetrics(metrics: StatusIndicatorState["metrics"]): string {
	const parts: string[] = [];

	if (metrics?.elapsedTime) {
		const seconds = Math.floor(metrics.elapsedTime / 1000);
		const ms = metrics.elapsedTime % 1000;
		if (seconds > 0) {
			parts.push(`${seconds}.${Math.floor(ms / 100)}s`);
		} else {
			parts.push(`${ms}ms`);
		}
	}

	if (metrics?.tokensUsed) {
		parts.push(`${metrics.tokensUsed} tokens`);
	}

	if (metrics?.modelName) {
		parts.push(metrics.modelName);
	}

	if (metrics?.participantName) {
		parts.push(`@${metrics.participantName}`);
	}

	return parts.join(" | ");
}
