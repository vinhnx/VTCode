import * as vscode from "vscode";
import { VtcodeStreamChunk } from "../vtcodeBackend";

export interface StreamingOptions {
    readonly bufferSize: number;
    readonly updateInterval: number;
    readonly enableSmoothing: boolean;
    readonly showProgress: boolean;
}

export interface StreamMetrics {
    readonly totalChunks: number;
    readonly totalBytes: number;
    readonly startTime: number;
    readonly endTime?: number;
    readonly averageChunkSize: number;
    readonly chunksPerSecond: number;
}

/**
 * Enhanced streaming manager for VT Code responses
 * Provides buffering, smoothing, progress tracking, and metrics
 */
export class StreamingManager implements vscode.Disposable {
    private readonly defaultOptions: StreamingOptions = {
        bufferSize: 10,
        updateInterval: 50,
        enableSmoothing: true,
        showProgress: true,
    };

    private buffer: VtcodeStreamChunk[] = [];
    private metrics: StreamMetrics | null = null;
    private updateTimer: NodeJS.Timeout | null = null;
    private progressDisposable: vscode.Disposable | null = null;
    private readonly onUpdateEmitter = new vscode.EventEmitter<
        VtcodeStreamChunk[]
    >();
    private readonly onCompleteEmitter =
        new vscode.EventEmitter<StreamMetrics>();
    private readonly onErrorEmitter = new vscode.EventEmitter<Error>();

    public readonly onUpdate = this.onUpdateEmitter.event;
    public readonly onComplete = this.onCompleteEmitter.event;
    public readonly onError = this.onErrorEmitter.event;

    /**
     * Stream chunks with enhanced buffering and smoothing
     */
    public async streamChunks(
        chunkGenerator: AsyncGenerator<VtcodeStreamChunk>,
        options?: Partial<StreamingOptions>
    ): Promise<StreamMetrics> {
        const opts = { ...this.defaultOptions, ...options };

        // Initialize metrics
        this.metrics = {
            totalChunks: 0,
            totalBytes: 0,
            startTime: Date.now(),
            averageChunkSize: 0,
            chunksPerSecond: 0,
        };

        // Show progress indicator if enabled
        if (opts.showProgress) {
            this.showProgressIndicator();
        }

        try {
            // Process chunks with buffering
            for await (const chunk of chunkGenerator) {
                await this.processChunk(chunk, opts);
            }

            // Flush remaining buffer
            await this.flushBuffer(opts);

            // Complete streaming
            return this.completeStreaming();
        } catch (error) {
            this.handleError(error as Error);
            throw error;
        } finally {
            this.cleanup();
        }
    }

    /**
     * Process individual chunk with buffering
     */
    private async processChunk(
        chunk: VtcodeStreamChunk,
        options: StreamingOptions
    ): Promise<void> {
        // Add to buffer
        this.buffer.push(chunk);

        // Update metrics
        this.updateMetrics(chunk);

        // Emit update if buffer is full or it's a terminal chunk
        if (
            this.buffer.length >= options.bufferSize ||
            chunk.kind === "done" ||
            chunk.kind === "error"
        ) {
            await this.flushBuffer(options);
        }
    }

    /**
     * Flush buffered chunks to listeners
     */
    private async flushBuffer(options: StreamingOptions): Promise<void> {
        if (this.buffer.length === 0) return;

        // Apply smoothing if enabled
        const chunksToEmit = options.enableSmoothing
            ? this.applySmoothing(this.buffer)
            : [...this.buffer];

        // Emit update
        this.onUpdateEmitter.fire(chunksToEmit);

        // Clear buffer
        this.buffer = [];
    }

    /**
     * Apply smoothing to reduce UI jitter
     */
    private applySmoothing(chunks: VtcodeStreamChunk[]): VtcodeStreamChunk[] {
        // Group text chunks by type to reduce rapid updates
        const smoothed: VtcodeStreamChunk[] = [];
        let currentText = "";

        for (const chunk of chunks) {
            if (chunk.kind === "text") {
                currentText += chunk.text;
            } else {
                // Flush accumulated text before non-text chunk
                if (currentText) {
                    smoothed.push({ kind: "text", text: currentText });
                    currentText = "";
                }
                smoothed.push(chunk);
            }
        }

        // Flush any remaining text
        if (currentText) {
            smoothed.push({ kind: "text", text: currentText });
        }

        return smoothed;
    }

    /**
     * Update streaming metrics
     */
    private updateMetrics(chunk: VtcodeStreamChunk): void {
        if (!this.metrics) return;

        this.metrics.totalChunks++;

        // Estimate bytes (rough approximation)
        const chunkSize = JSON.stringify(chunk).length;
        this.metrics.totalBytes += chunkSize;

        // Calculate averages
        this.metrics.averageChunkSize =
            this.metrics.totalBytes / this.metrics.totalChunks;

        const elapsedSeconds = (Date.now() - this.metrics.startTime) / 1000;
        if (elapsedSeconds > 0) {
            this.metrics.chunksPerSecond =
                this.metrics.totalChunks / elapsedSeconds;
        }
    }

    /**
     * Show progress indicator
     */
    private showProgressIndicator(): void {
        this.progressDisposable = vscode.window.withProgress(
            {
                location: vscode.ProgressLocation.Notification,
                title: "VT Code is thinking...",
                cancellable: true,
            },
            async (progress, token) => {
                return new Promise<void>((resolve) => {
                    let lastUpdate = Date.now();

                    const updateProgress = () => {
                        if (!this.metrics) return;

                        const elapsed = Date.now() - this.metrics.startTime;
                        const estimatedTotalTime = this.estimateTotalTime();

                        if (estimatedTotalTime > 0) {
                            const progressPercent = Math.min(
                                (elapsed / estimatedTotalTime) * 100,
                                90
                            );
                            progress.report({
                                increment: progressPercent,
                                message: this.getProgressMessage(),
                            });
                        }

                        // Check for cancellation
                        if (token.isCancellationRequested) {
                            this.handleCancellation();
                        }

                        // Continue updating
                        if (this.updateTimer) {
                            this.updateTimer = setTimeout(updateProgress, 100);
                        }
                    };

                    this.updateTimer = setTimeout(updateProgress, 100);

                    // Resolve when streaming completes
                    this.onComplete(() => {
                        clearTimeout(this.updateTimer!);
                        this.updateTimer = null;
                        resolve();
                    });
                });
            }
        );
    }

    /**
     * Estimate total streaming time based on current metrics
     */
    private estimateTotalTime(): number {
        if (!this.metrics || this.metrics.totalChunks < 5) {
            return 30000; // Default 30 seconds for initial chunks
        }

        // Estimate based on average chunk processing time
        const avgTimePerChunk =
            (Date.now() - this.metrics.startTime) / this.metrics.totalChunks;
        const estimatedChunks = this.metrics.totalChunks * 2; // Assume we're halfway
        return avgTimePerChunk * estimatedChunks;
    }

    /**
     * Get progress message based on current state
     */
    private getProgressMessage(): string {
        if (!this.metrics) return "Processing...";

        const { totalChunks, chunksPerSecond, totalBytes } = this.metrics;
        const mb = (totalBytes / 1024 / 1024).toFixed(2);

        if (chunksPerSecond > 10) {
            return `Processing ${totalChunks} chunks (${mb} MB) at ${chunksPerSecond.toFixed(
                1
            )} chunks/sec`;
        } else if (totalChunks < 10) {
            return "Initializing...";
        } else {
            return `Processing... ${totalChunks} chunks received`;
        }
    }

    /**
     * Complete streaming and return final metrics
     */
    private completeStreaming(): StreamMetrics {
        if (!this.metrics) {
            throw new Error("Streaming not started");
        }

        // Mark end time
        this.metrics = {
            ...this.metrics,
            endTime: Date.now(),
        };

        // Emit completion event
        this.onCompleteEmitter.fire(this.metrics);

        return this.metrics;
    }

    /**
     * Handle streaming error
     */
    private handleError(error: Error): void {
        this.onErrorEmitter.fire(error);
    }

    /**
     * Handle cancellation
     */
    private handleCancellation(): void {
        const error = new Error("Streaming cancelled by user");
        this.handleError(error);
    }

    /**
     * Cleanup resources
     */
    private cleanup(): void {
        // Clear update timer
        if (this.updateTimer) {
            clearTimeout(this.updateTimer);
            this.updateTimer = null;
        }

        // Dispose progress indicator
        if (this.progressDisposable) {
            this.progressDisposable.dispose();
            this.progressDisposable = null;
        }

        // Clear buffer
        this.buffer = [];
    }

    /**
     * Get current streaming metrics
     */
    public getMetrics(): StreamMetrics | null {
        return this.metrics;
    }

    /**
     * Check if currently streaming
     */
    public isStreaming(): boolean {
        return this.metrics !== null && !this.metrics.endTime;
    }

    /**
     * Force flush of current buffer
     */
    public async forceFlush(): Promise<void> {
        if (this.buffer.length > 0) {
            await this.flushBuffer(this.defaultOptions);
        }
    }

    /**
     * Dispose resources
     */
    public dispose(): void {
        this.cleanup();
        this.onUpdateEmitter.dispose();
        this.onCompleteEmitter.dispose();
        this.onErrorEmitter.dispose();
    }
}

/**
 * Create a throttled generator for smoother streaming
 */
export function createThrottledGenerator<T>(
    generator: AsyncGenerator<T>,
    minIntervalMs: number = 50
): AsyncGenerator<T> {
    let lastEmitTime = 0;

    return (async function* () {
        for await (const item of generator) {
            const now = Date.now();
            const timeSinceLastEmit = now - lastEmitTime;

            if (timeSinceLastEmit < minIntervalMs) {
                await new Promise((resolve) =>
                    setTimeout(resolve, minIntervalMs - timeSinceLastEmit)
                );
            }

            yield item;
            lastEmitTime = Date.now();
        }
    })();
}

/**
 * Create a buffered generator for batching small chunks
 */
export function createBufferedGenerator<T>(
    generator: AsyncGenerator<T>,
    bufferSize: number = 5,
    bufferTimeMs: number = 100
): AsyncGenerator<T[]> {
    return (async function* () {
        const buffer: T[] = [];
        let lastEmitTime = Date.now();

        for await (const item of generator) {
            buffer.push(item);

            const now = Date.now();
            const timeInBuffer = now - lastEmitTime;

            if (buffer.length >= bufferSize || timeInBuffer >= bufferTimeMs) {
                yield [...buffer];
                buffer.length = 0;
                lastEmitTime = now;
            }
        }

        // Emit any remaining items
        if (buffer.length > 0) {
            yield [...buffer];
        }
    })();
}
