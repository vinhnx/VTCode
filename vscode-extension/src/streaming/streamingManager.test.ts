import * as vscode from "vscode";
import { VtcodeStreamChunk } from "../vtcodeBackend";
import {
    StreamingManager,
    createBufferedGenerator,
    createThrottledGenerator,
} from "./streamingManager";

// Mock VS Code API
jest.mock("vscode", () => ({
    window: {
        withProgress: jest.fn(),
    },
    ProgressLocation: {
        Notification: 15,
    },
}));

describe("StreamingManager", () => {
    let manager: StreamingManager;
    let mockGenerator: AsyncGenerator<VtcodeStreamChunk>;

    beforeEach(() => {
        manager = new StreamingManager();

        // Create a mock chunk generator
        mockGenerator = (async function* () {
            yield { kind: "text", text: "Hello" };
            yield { kind: "text", text: " " };
            yield { kind: "text", text: "World" };
            yield { kind: "done" };
        })();

        jest.clearAllMocks();
    });

    afterEach(() => {
        manager.dispose();
    });

    describe("streamChunks", () => {
        it("should stream chunks successfully", async () => {
            const mockProgress = {
                report: jest.fn(),
            };
            const mockToken = {
                isCancellationRequested: false,
            };

            (vscode.window.withProgress as jest.Mock).mockImplementation(
                (options, callback) => {
                    return callback(mockProgress, mockToken);
                }
            );

            const metrics = await manager.streamChunks(mockGenerator);

            expect(metrics.totalChunks).toBe(4); // 3 text + 1 done
            expect(metrics.totalBytes).toBeGreaterThan(0);
            expect(metrics.startTime).toBeLessThanOrEqual(Date.now());
            expect(metrics.endTime).toBeDefined();
        });

        it("should buffer chunks before emitting", async () => {
            const chunks: VtcodeStreamChunk[][] = [];

            manager.onUpdate((bufferedChunks) => {
                chunks.push(bufferedChunks);
            });

            await manager.streamChunks(mockGenerator, { bufferSize: 2 });

            // Should have buffered chunks
            expect(chunks.length).toBeGreaterThan(0);

            // Verify smoothing applied (text chunks should be combined)
            const allTextChunks = chunks
                .flat()
                .filter((c) => c.kind === "text");
            const combinedText = allTextChunks
                .map((c) => (c as any).text)
                .join("");
            expect(combinedText).toContain("Hello World");
        });

        it("should handle errors during streaming", async () => {
            const errorGenerator = (async function* () {
                yield { kind: "text", text: "Start" };
                throw new Error("Streaming error");
            })();

            const errors: Error[] = [];
            manager.onError((error) => {
                errors.push(error);
            });

            await expect(manager.streamChunks(errorGenerator)).rejects.toThrow(
                "Streaming error"
            );
            expect(errors).toHaveLength(1);
            expect(errors[0].message).toBe("Streaming error");
        });

        it("should show progress indicator when enabled", async () => {
            const mockProgress = {
                report: jest.fn(),
            };
            const mockToken = {
                isCancellationRequested: false,
            };

            (vscode.window.withProgress as jest.Mock).mockImplementation(
                (options, callback) => {
                    return callback(mockProgress, mockToken);
                }
            );

            await manager.streamChunks(mockGenerator, { showProgress: true });

            expect(vscode.window.withProgress).toHaveBeenCalledWith(
                expect.objectContaining({
                    location: 15, // Notification
                    title: "VT Code is thinking...",
                    cancellable: true,
                }),
                expect.any(Function)
            );
        });

        it("should disable progress indicator when showProgress is false", async () => {
            await manager.streamChunks(mockGenerator, { showProgress: false });

            expect(vscode.window.withProgress).not.toHaveBeenCalled();
        });

        it("should calculate metrics correctly", async () => {
            const metrics = await manager.streamChunks(mockGenerator);

            expect(metrics.totalChunks).toBe(4);
            expect(metrics.totalBytes).toBeGreaterThan(0);
            expect(metrics.averageChunkSize).toBe(
                metrics.totalBytes / metrics.totalChunks
            );
            expect(metrics.chunksPerSecond).toBeGreaterThan(0);
        });

        it("should handle cancellation", async () => {
            const mockProgress = {
                report: jest.fn(),
            };
            const mockToken = {
                isCancellationRequested: true, // Simulate cancellation
            };

            (vscode.window.withProgress as jest.Mock).mockImplementation(
                (options, callback) => {
                    return callback(mockProgress, mockToken);
                }
            );

            const cancelGenerator = (async function* () {
                yield { kind: "text", text: "Start" };
                await new Promise((resolve) => setTimeout(resolve, 1000)); // Long delay
                yield { kind: "text", text: "End" };
            })();

            const errors: Error[] = [];
            manager.onError((error) => {
                errors.push(error);
            });

            await expect(manager.streamChunks(cancelGenerator)).rejects.toThrow(
                "Streaming cancelled by user"
            );
            expect(errors).toHaveLength(1);
            expect(errors[0].message).toBe("Streaming cancelled by user");
        });
    });

    describe("isStreaming", () => {
        it("should return false when not streaming", () => {
            expect(manager.isStreaming()).toBe(false);
        });

        it("should return true during streaming", async () => {
            const slowGenerator = (async function* () {
                yield { kind: "text", text: "Start" };
                await new Promise((resolve) => setTimeout(resolve, 100));
                yield { kind: "done" };
            })();

            const streamingPromise = manager.streamChunks(slowGenerator);

            // Check during streaming
            expect(manager.isStreaming()).toBe(true);

            await streamingPromise;

            // Check after completion
            expect(manager.isStreaming()).toBe(false);
        });
    });

    describe("getMetrics", () => {
        it("should return null when not streaming", () => {
            expect(manager.getMetrics()).toBeNull();
        });

        it("should return metrics during streaming", async () => {
            const metricsPromise = manager.streamChunks(mockGenerator);
            const metrics = await metricsPromise;

            expect(manager.getMetrics()).toEqual(metrics);
        });
    });

    describe("forceFlush", () => {
        it("should flush remaining buffer", async () => {
            const chunks: VtcodeStreamChunk[][] = [];

            manager.onUpdate((bufferedChunks) => {
                chunks.push(bufferedChunks);
            });

            // Start streaming with large buffer
            const streamingPromise = manager.streamChunks(mockGenerator, {
                bufferSize: 10,
            });

            // Force flush before completion
            await manager.forceFlush();

            await streamingPromise;

            // Should have received flushed chunks
            expect(chunks.length).toBeGreaterThan(0);
        });
    });
});

describe("createThrottledGenerator", () => {
    it("should throttle chunk emission", async () => {
        const fastGenerator = (async function* () {
            yield { kind: "text", text: "1" };
            yield { kind: "text", text: "2" };
            yield { kind: "text", text: "3" };
            yield { kind: "done" };
        })();

        const throttled = createThrottledGenerator(fastGenerator, 50);

        const startTime = Date.now();
        const chunks: VtcodeStreamChunk[] = [];

        for await (const chunk of throttled) {
            chunks.push(chunk);
        }

        const elapsed = Date.now() - startTime;

        // Should take at least 150ms (3 chunks * 50ms throttle)
        expect(elapsed).toBeGreaterThanOrEqual(150);
        expect(chunks).toHaveLength(4);
    });

    it("should pass through all chunks", async () => {
        const sourceGenerator = (async function* () {
            yield { kind: "text", text: "A" };
            yield { kind: "text", text: "B" };
            yield { kind: "reasoning", text: "Thinking" };
            yield { kind: "done" };
        })();

        const throttled = createThrottledGenerator(sourceGenerator, 10);

        const chunks: VtcodeStreamChunk[] = [];
        for await (const chunk of throttled) {
            chunks.push(chunk);
        }

        expect(chunks).toHaveLength(4);
        expect(chunks[0]).toEqual({ kind: "text", text: "A" });
        expect(chunks[1]).toEqual({ kind: "text", text: "B" });
        expect(chunks[2]).toEqual({ kind: "reasoning", text: "Thinking" });
        expect(chunks[3]).toEqual({ kind: "done" });
    });
});

describe("createBufferedGenerator", () => {
    it("should buffer chunks before emitting", async () => {
        const sourceGenerator = (async function* () {
            yield { kind: "text", text: "1" };
            yield { kind: "text", text: "2" };
            yield { kind: "text", text: "3" };
            yield { kind: "text", text: "4" };
            yield { kind: "done" };
        })();

        const buffered = createBufferedGenerator(sourceGenerator, 3, 1000); // Large timeout to test size-based buffering

        const batches: VtcodeStreamChunk[][] = [];
        for await (const batch of buffered) {
            batches.push(batch);
        }

        // Should have at least 2 batches (3 items + 2 items)
        expect(batches.length).toBeGreaterThanOrEqual(2);

        // First batch should have 3 items
        expect(batches[0].length).toBe(3);

        // Verify all chunks are present
        const allChunks = batches.flat();
        expect(allChunks).toHaveLength(5);
    });

    it("should emit based on time threshold", async () => {
        const slowGenerator = (async function* () {
            yield { kind: "text", text: "1" };
            await new Promise((resolve) => setTimeout(resolve, 150)); // Wait for time threshold
            yield { kind: "text", text: "2" };
            yield { kind: "done" };
        })();

        const buffered = createBufferedGenerator(slowGenerator, 10, 100); // Small time threshold

        const batches: VtcodeStreamChunk[][] = [];
        for await (const batch of buffered) {
            batches.push(batch);
        }

        // Should have multiple batches due to time threshold
        expect(batches.length).toBeGreaterThan(1);
    });

    it("should emit remaining chunks at end", async () => {
        const sourceGenerator = (async function* () {
            yield { kind: "text", text: "1" };
            yield { kind: "text", text: "2" };
            yield { kind: "done" };
        })();

        const buffered = createBufferedGenerator(sourceGenerator, 5, 1000); // Large buffer size

        const batches: VtcodeStreamChunk[][] = [];
        for await (const batch of buffered) {
            batches.push(batch);
        }

        // Should emit all chunks at the end
        expect(batches.length).toBe(1);
        expect(batches[0]).toHaveLength(3);
    });
});
