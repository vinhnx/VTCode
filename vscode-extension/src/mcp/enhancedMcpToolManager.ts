import { spawn } from "node:child_process";
import * as vscode from "vscode";
import { 
    McpTool, 
    McpProvider, 
    McpToolInvocation, 
    McpToolResult,
    McpToolManager 
} from "../mcpTools";

export interface EnhancedMcpProvider extends McpProvider {
    readonly health: ProviderHealth;
    readonly lastHealthCheck: number;
    readonly toolCache: Map<string, McpTool[]>;
    readonly stats: ProviderStats;
}

export interface ProviderHealth {
    readonly status: 'healthy' | 'unhealthy' | 'unknown';
    readonly responseTime: number;
    readonly lastError?: string;
    readonly consecutiveFailures: number;
}

export interface ProviderStats {
    readonly totalInvocations: number;
    readonly successfulInvocations: number;
    readonly failedInvocations: number;
    readonly averageResponseTime: number;
}

export interface ToolExecutionOptions {
    readonly timeoutMs: number;
    readonly enableStreaming: boolean;
    readonly retryAttempts: number;
    readonly retryDelayMs: number;
}

export interface StreamingToolResult {
    readonly type: 'data' | 'error' | 'complete';
    readonly data?: unknown;
    readonly error?: string;
    readonly progress?: number;
}

/**
 * Enhanced MCP Tool Manager with health checks, caching, and streaming support
 */
export class EnhancedMcpToolManager extends McpToolManager {
    private enhancedProviders = new Map<string, EnhancedMcpProvider>();
    private readonly toolExecutionCache = new Map<string, McpToolResult>();
    private readonly streamingCallbacks = new Map<string, (result: StreamingToolResult) => void>();
    
    private readonly defaultExecutionOptions: ToolExecutionOptions = {
        timeoutMs: 30000,
        enableStreaming: false,
        retryAttempts: 3,
        retryDelayMs: 1000,
    };

    constructor(outputChannel: vscode.OutputChannel) {
        super(outputChannel);
        this.outputChannel.appendLine("[EnhancedMCP] Enhanced MCP manager initialized");
    }

    /**
     * Load providers with health checks
     */
    async loadProviders(workspaceRoot: string): Promise<void> {
        await super.loadProviders(workspaceRoot);
        
        // Enhance loaded providers with health monitoring
        for (const [name, provider] of this.getProviders()) {
            this.enhancedProviders.set(name, {
                ...provider,
                health: {
                    status: 'unknown',
                    responseTime: 0,
                    consecutiveFailures: 0,
                },
                lastHealthCheck: 0,
                toolCache: new Map(),
                stats: {
                    totalInvocations: 0,
                    successfulInvocations: 0,
                    failedInvocations: 0,
                    averageResponseTime: 0,
                },
            });
        }

        // Perform initial health check
        await this.performHealthChecks();
    }

    /**
     * Perform health checks on all providers
     */
    async performHealthChecks(): Promise<void> {
        const checkPromises = Array.from(this.enhancedProviders.entries()).map(
            async ([name, provider]) => {
                try {
                    const startTime = Date.now();
                    await this.checkProviderHealth(provider);
                    const responseTime = Date.now() - startTime;

                    provider.health = {
                        status: 'healthy',
                        responseTime,
                        consecutiveFailures: 0,
                    };
                    provider.lastHealthCheck = Date.now();

                    this.outputChannel.appendLine(
                        `[EnhancedMCP] Provider ${name} is healthy (${responseTime}ms)`
                    );
                } catch (error) {
                    provider.health.consecutiveFailures++;
                    provider.health.status = 'unhealthy';
                    provider.health.lastError = error instanceof Error ? error.message : String(error);

                    this.outputChannel.appendLine(
                        `[EnhancedMCP] Provider ${name} health check failed: ${provider.health.lastError}`
                    );
                }
            }
        );

        await Promise.allSettled(checkPromises);
    }

    /**
     * Check individual provider health
     */
    private async checkProviderHealth(provider: EnhancedMcpProvider): Promise<void> {
        return new Promise((resolve, reject) => {
            const proc = spawn(
                provider.command,
                [...provider.args, "--health-check"],
                {
                    env: { ...process.env, ...provider.env },
                    timeout: 5000,
                }
            );

            let stdout = "";
            let stderr = "";

            proc.stdout.on("data", (data: Buffer) => {
                stdout += data.toString();
            });

            proc.stderr.on("data", (data: Buffer) => {
                stderr += data.toString();
            });

            proc.on("close", (code: number | null) => {
                if (code === 0) {
                    resolve();
                } else {
                    reject(new Error(`Health check failed with code ${code}: ${stderr || stdout}`));
                }
            });

            proc.on("error", (error: Error) => {
                reject(error);
            });
        });
    }

    /**
     * Discover tools with caching
     */
    async discoverTools(): Promise<void> {
        for (const [providerName, provider] of this.enhancedProviders) {
            try {
                // Check cache first
                const cacheKey = `tools_${providerName}`;
                const cachedTools = provider.toolCache.get(cacheKey);
                
                if (cachedTools) {
                    this.outputChannel.appendLine(
                        `[EnhancedMCP] Using cached tools for ${providerName}`
                    );
                    for (const tool of cachedTools) {
                        const fullName = `${providerName}/${tool.name}`;
                        this.setTool(fullName, { ...tool, provider: providerName });
                    }
                    continue;
                }

                // Discover tools from provider
                const tools = await this.queryProviderTools(provider);
                
                // Cache the discovered tools
                provider.toolCache.set(cacheKey, tools);
                
                // Store tools
                for (const tool of tools) {
                    const fullName = `${providerName}/${tool.name}`;
                    this.setTool(fullName, { ...tool, provider: providerName });
                    this.outputChannel.appendLine(
                        `[EnhancedMCP] Discovered tool: ${fullName}`
                    );
                }

                this.outputChannel.appendLine(
                    `[EnhancedMCP] Discovered ${tools.length} tools from ${providerName}`
                );
            } catch (error) {
                provider.stats.failedInvocations++;
                provider.health.consecutiveFailures++;
                
                this.outputChannel.appendLine(
                    `[EnhancedMCP] Failed to discover tools from ${providerName}: ${
                        error instanceof Error ? error.message : String(error)
                    }`
                );
            }
        }
    }

    /**
     * Invoke tool with enhanced features (retry, timeout, streaming)
     */
    async invokeToolEnhanced(
        invocation: McpToolInvocation,
        options?: Partial<ToolExecutionOptions>
    ): Promise<McpToolResult> {
        const opts = { ...this.defaultExecutionOptions, ...options };
        const startTime = Date.now();
        const provider = this.enhancedProviders.get(invocation.provider);
        
        if (!provider) {
            return {
                success: false,
                error: `Provider not found: ${invocation.provider}`,
                executionTimeMs: Date.now() - startTime,
            };
        }

        // Update stats
        provider.stats.totalInvocations++;

        // Check provider health
        if (provider.health.status === 'unhealthy' && provider.health.consecutiveFailures > 3) {
            const error = `Provider ${invocation.provider} is unhealthy after ${provider.health.consecutiveFailures} failures`;
            this.outputChannel.appendLine(`[EnhancedMCP] ${error}`);
            
            provider.stats.failedInvocations++;
            return {
                success: false,
                error,
                executionTimeMs: Date.now() - startTime,
            };
        }

        // Check execution cache
        const cacheKey = this.getExecutionCacheKey(invocation);
        if (this.toolExecutionCache.has(cacheKey)) {
            this.outputChannel.appendLine(
                `[EnhancedMCP] Using cached result for ${invocation.tool}`
            );
            return this.toolExecutionCache.get(cacheKey)!;
        }

        // Execute with retry logic
        let lastError: Error | null = null;
        
        for (let attempt = 0; attempt <= opts.retryAttempts; attempt++) {
            try {
                if (opts.enableStreaming) {
                    return await this.executeToolWithStreaming(provider, invocation, opts);
                } else {
                    return await this.executeToolWithRetry(provider, invocation, opts);
                }
            } catch (error) {
                lastError = error as Error;
                provider.stats.failedInvocations++;
                provider.health.consecutiveFailures++;
                
                this.outputChannel.appendLine(
                    `[EnhancedMCP] Attempt ${attempt + 1} failed: ${lastError.message}`
                );

                if (attempt < opts.retryAttempts) {
                    await this.sleep(opts.retryDelayMs * (attempt + 1)); // Exponential backoff
                }
            }
        }

        // All attempts failed
        const result: McpToolResult = {
            success: false,
            error: lastError?.message || 'All retry attempts failed',
            executionTimeMs: Date.now() - startTime,
        };

        // Cache the failure for a short time to avoid repeated failures
        this.toolExecutionCache.set(cacheKey, result);
        setTimeout(() => this.toolExecutionCache.delete(cacheKey), 60000); // Clear after 1 minute

        return result;
    }

    /**
     * Execute tool with retry logic
     */
    private async executeToolWithRetry(
        provider: EnhancedMcpProvider,
        invocation: McpToolInvocation,
        options: ToolExecutionOptions
    ): Promise<McpToolResult> {
        const startTime = Date.now();
        
        return new Promise((resolve, reject) => {
            const proc = spawn(
                provider.command,
                [
                    ...provider.args,
                    "--tool",
                    invocation.tool,
                    "--args",
                    JSON.stringify(invocation.arguments),
                ],
                {
                    env: { ...process.env, ...provider.env },
                    timeout: options.timeoutMs,
                }
            );

            let stdout = "";
            let stderr = "";

            proc.stdout.on("data", (data: Buffer) => {
                stdout += data.toString();
            });

            proc.stderr.on("data", (data: Buffer) => {
                stderr += data.toString();
            });

            proc.on("close", (code: number | null) => {
                const executionTime = Date.now() - startTime;
                
                // Update provider stats
                provider.stats.averageResponseTime = 
                    (provider.stats.averageResponseTime + executionTime) / 2;

                if (code === 0) {
                    provider.stats.successfulInvocations++;
                    provider.health.consecutiveFailures = 0;
                    provider.health.status = 'healthy';
                    
                    try {
                        const result = JSON.parse(stdout);
                        const toolResult: McpToolResult = {
                            success: true,
                            result,
                            executionTimeMs: executionTime,
                        };
                        
                        // Cache successful result
                        const cacheKey = this.getExecutionCacheKey(invocation);
                        this.toolExecutionCache.set(cacheKey, toolResult);
                        
                        resolve(toolResult);
                    } catch {
                        // Return raw stdout if not JSON
                        resolve({
                            success: true,
                            result: stdout,
                            executionTimeMs: executionTime,
                        });
                    }
                } else {
                    provider.stats.failedInvocations++;
                    reject(new Error(`Tool execution failed with code ${code}: ${stderr || stdout}`));
                }
            });

            proc.on("error", (error: Error) => {
                reject(error);
            });
        });
    }

    /**
     * Execute tool with streaming support
     */
    private async executeToolWithStreaming(
        provider: EnhancedMcpProvider,
        invocation: McpToolInvocation,
        options: ToolExecutionOptions
    ): Promise<McpToolResult> {
        const startTime = Date.now();
        const streamingId = `${invocation.provider}_${invocation.tool}_${startTime}`;
        
        return new Promise((resolve, reject) => {
            const proc = spawn(
                provider.command,
                [
                    ...provider.args,
                    "--tool",
                    invocation.tool,
                    "--args",
                    JSON.stringify(invocation.arguments),
                    "--stream",
                ],
                {
                    env: { ...process.env, ...provider.env },
                }
            );

            let stdout = "";
            let stderr = "";
            let isStreaming = false;

            proc.stdout.on("data", (data: Buffer) => {
                const chunk = data.toString();
                stdout += chunk;
                
                // Try to parse streaming chunks
                try {
                    const lines = chunk.split('\n').filter(line => line.trim());
                    
                    for (const line of lines) {
                        const streamingResult = JSON.parse(line) as StreamingToolResult;
                        isStreaming = true;
                        
                        // Notify streaming callback if registered
                        const callback = this.streamingCallbacks.get(streamingId);
                        if (callback) {
                            callback(streamingResult);
                        }
                        
                        // Handle progress updates
                        if (streamingResult.type === 'data' && streamingResult.progress) {
                            this.outputChannel.appendLine(
                                `[EnhancedMCP] Progress: ${streamingResult.progress}%`
                            );
                        }
                    }
                } catch {
                    // Not a streaming chunk, accumulate
                }
            });

            proc.stderr.on("data", (data: Buffer) => {
                stderr += data.toString();
            });

            proc.on("close", (code: number | null) => {
                const executionTime = Date.now() - startTime;
                
                if (code === 0) {
                    provider.stats.successfulInvocations++;
                    
                    try {
                        const result = isStreaming ? stdout : JSON.parse(stdout);
                        resolve({
                            success: true,
                            result,
                            executionTimeMs: executionTime,
                        });
                    } catch {
                        resolve({
                            success: true,
                            result: stdout,
                            executionTimeMs: executionTime,
                        });
                    }
                } else {
                    provider.stats.failedInvocations++;
                    reject(new Error(`Streaming tool execution failed: ${stderr || stdout}`));
                }
                
                // Clean up streaming callback
                this.streamingCallbacks.delete(streamingId);
            });

            proc.on("error", (error: Error) => {
                this.streamingCallbacks.delete(streamingId);
                reject(error);
            });

            // Timeout for streaming
            setTimeout(() => {
                proc.kill();
                this.streamingCallbacks.delete(streamingId);
                reject(new Error("Streaming tool execution timed out"));
            }, options.timeoutMs);
        });
    }

    /**
     * Register streaming callback for a tool execution
     */
    public registerStreamingCallback(
        invocation: McpToolInvocation,
        callback: (result: StreamingToolResult) => void
    ): void {
        const key = `${invocation.provider}_${invocation.tool}_${Date.now()}`;
        this.streamingCallbacks.set(key, callback);
    }

    /**
     * Get enhanced provider information
     */
    public getEnhancedProvider(name: string): EnhancedMcpProvider | undefined {
        return this.enhancedProviders.get(name);
    }

    /**
     * Get all enhanced providers
     */
    public getAllEnhancedProviders(): EnhancedMcpProvider[] {
        return Array.from(this.enhancedProviders.values());
    }

    /**
     * Get provider health status
     */
    public getProviderHealth(name: string): ProviderHealth | undefined {
        return this.enhancedProviders.get(name)?.health;
    }

    /**
     * Clear all caches
     */
    public clearCaches(): void {
        this.toolExecutionCache.clear();
        for (const provider of this.enhancedProviders.values()) {
            provider.toolCache.clear();
        }
        this.outputChannel.appendLine("[EnhancedMCP] All caches cleared");
    }

    /**
     * Get execution cache key
     */
    private getExecutionCacheKey(invocation: McpToolInvocation): string {
        return `${invocation.provider}_${invocation.tool}_${JSON.stringify(invocation.arguments)}`;
    }

    /**
     * Sleep helper for retry delays
     */
    private sleep(ms: number): Promise<void> {
        return new Promise(resolve => setTimeout(resolve, ms));
    }

    /**
     * Get providers from base class
     */
    private getProviders(): Map<string, McpProvider> {
        // Access base class providers (would need to be protected in base class)
        // For now, we'll use a workaround
        return (this as any).providers || new Map();
    }

    /**
     * Set tool in base class
     */
    private setTool(name: string, tool: McpTool): void {
        // Access base class tools (would need to be protected in base class)
        // For now, we'll use a workaround
        if ((this as any).tools) {
            (this as any).tools.set(name, tool);
        }
    }
}

/**
 * Factory function to create enhanced MCP tool manager
 */
export async function createEnhancedMcpToolManager(
    outputChannel: vscode.OutputChannel
): Promise<EnhancedMcpToolManager | null> {
    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (!workspaceFolders || workspaceFolders.length === 0) {
        outputChannel.appendLine("[EnhancedMCP] No workspace folder found");
        return null;
    }

    const manager = new EnhancedMcpToolManager(outputChannel);
    await manager.loadProviders(workspaceFolders[0].uri.fsPath);

    return manager;
}