/**
 * Context7 MCP Integration for VTCode Chat Extension
 *
 * Provides seamless integration with Context7 Model Context Protocol
 * for enhanced documentation retrieval, code understanding, and
 * context-aware assistance.
 *
 * Integration Points:
 * - Library documentation fetching
 * - Code context enhancement
 * - API reference retrieval
 * - Best practices suggestions
 */

import * as vscode from "vscode";
import { McpToolInvocation, McpToolManager } from "./mcpTools";

export interface Context7Config {
    enabled: boolean;
    maxTokens: number;
    cacheResults: boolean;
    cacheTTLSeconds: number;
    autoFetchDocs: boolean;
}

export interface Context7LibraryInfo {
    id: string;
    name: string;
    description: string;
    codeSnippets: number;
    trustScore: number;
    versions?: string[];
}

export interface Context7Documentation {
    libraryId: string;
    topic?: string;
    content: string;
    tokens: number;
    cached: boolean;
}

/**
 * Context7 MCP Integration Manager
 */
export class Context7Integration {
    private config: Context7Config;
    private cache = new Map<
        string,
        { data: Context7Documentation; timestamp: number }
    >();
    private mcpManager: McpToolManager | null;
    private outputChannel: vscode.OutputChannel;

    constructor(
        mcpManager: McpToolManager | null,
        outputChannel: vscode.OutputChannel,
        config?: Partial<Context7Config>
    ) {
        this.mcpManager = mcpManager;
        this.outputChannel = outputChannel;
        this.config = {
            enabled: config?.enabled ?? true,
            maxTokens: config?.maxTokens ?? 5000,
            cacheResults: config?.cacheResults ?? true,
            cacheTTLSeconds: config?.cacheTTLSeconds ?? 3600,
            autoFetchDocs: config?.autoFetchDocs ?? true,
        };

        this.outputChannel.appendLine("[Context7] Integration initialized");
    }

    /**
     * Resolve library ID from library name
     */
    async resolveLibraryId(
        libraryName: string
    ): Promise<Context7LibraryInfo[]> {
        if (!this.config.enabled || !this.mcpManager) {
            throw new Error("Context7 integration is not enabled");
        }

        this.outputChannel.appendLine(
            `[Context7] Resolving library: ${libraryName}`
        );

        try {
            const invocation: McpToolInvocation = {
                provider: "context7",
                tool: "resolve-library-id",
                arguments: { libraryName },
            };

            const result = await this.mcpManager.invokeTool(invocation);

            if (!result.success) {
                throw new Error(result.error || "Failed to resolve library ID");
            }

            // Parse the result
            const libraries = this.parseLibraryResults(result.result);
            this.outputChannel.appendLine(
                `[Context7] Found ${libraries.length} libraries for "${libraryName}"`
            );

            return libraries;
        } catch (error) {
            this.outputChannel.appendLine(
                `[Context7] Error resolving library: ${
                    error instanceof Error ? error.message : String(error)
                }`
            );
            throw error;
        }
    }

    /**
     * Get library documentation
     */
    async getLibraryDocs(
        libraryId: string,
        topic?: string,
        maxTokens?: number
    ): Promise<Context7Documentation> {
        if (!this.config.enabled || !this.mcpManager) {
            throw new Error("Context7 integration is not enabled");
        }

        // Check cache first
        const cacheKey = this.getCacheKey(libraryId, topic);
        if (this.config.cacheResults) {
            const cached = this.cache.get(cacheKey);
            if (cached && !this.isCacheExpired(cached.timestamp)) {
                this.outputChannel.appendLine(
                    `[Context7] Cache hit for ${libraryId}`
                );
                return { ...cached.data, cached: true };
            }
        }

        this.outputChannel.appendLine(
            `[Context7] Fetching docs for ${libraryId}${
                topic ? ` (topic: ${topic})` : ""
            }`
        );

        try {
            const invocation: McpToolInvocation = {
                provider: "context7",
                tool: "get-library-docs",
                arguments: {
                    context7CompatibleLibraryID: libraryId,
                    topic: topic || undefined,
                    tokens: maxTokens || this.config.maxTokens,
                },
            };

            const result = await this.mcpManager.invokeTool(invocation);

            if (!result.success) {
                throw new Error(
                    result.error || "Failed to fetch library documentation"
                );
            }

            const docs: Context7Documentation = {
                libraryId,
                topic,
                content:
                    typeof result.result === "string"
                        ? result.result
                        : JSON.stringify(result.result, null, 2),
                tokens: this.estimateTokens(result.result),
                cached: false,
            };

            // Cache the result
            if (this.config.cacheResults) {
                this.cache.set(cacheKey, {
                    data: docs,
                    timestamp: Date.now(),
                });
            }

            this.outputChannel.appendLine(
                `[Context7] Fetched ${docs.tokens} tokens for ${libraryId}`
            );

            return docs;
        } catch (error) {
            this.outputChannel.appendLine(
                `[Context7] Error fetching docs: ${
                    error instanceof Error ? error.message : String(error)
                }`
            );
            throw error;
        }
    }

    /**
     * Get documentation for multiple libraries
     */
    async getMultipleLibraryDocs(
        requests: Array<{ libraryId: string; topic?: string }>
    ): Promise<Context7Documentation[]> {
        const promises = requests.map((req) =>
            this.getLibraryDocs(req.libraryId, req.topic)
        );

        return Promise.all(promises);
    }

    /**
     * Auto-detect and fetch relevant documentation
     */
    async autoFetchRelevantDocs(
        context: string
    ): Promise<Context7Documentation[]> {
        if (!this.config.autoFetchDocs) {
            return [];
        }

        this.outputChannel.appendLine(
            "[Context7] Auto-detecting relevant documentation"
        );

        // Extract potential library references from context
        const libraries = this.detectLibraries(context);

        if (libraries.length === 0) {
            this.outputChannel.appendLine(
                "[Context7] No libraries detected in context"
            );
            return [];
        }

        this.outputChannel.appendLine(
            `[Context7] Detected libraries: ${libraries.join(", ")}`
        );

        // Resolve and fetch docs for detected libraries
        const docs: Context7Documentation[] = [];

        for (const libName of libraries) {
            try {
                const resolved = await this.resolveLibraryId(libName);
                if (resolved.length > 0) {
                    const bestMatch = this.selectBestMatch(resolved);
                    const libDocs = await this.getLibraryDocs(bestMatch.id);
                    docs.push(libDocs);
                }
            } catch (error) {
                this.outputChannel.appendLine(
                    `[Context7] Failed to fetch docs for ${libName}: ${error}`
                );
            }
        }

        return docs;
    }

    /**
     * Enhance user query with Context7 documentation
     */
    async enhanceQuery(
        userQuery: string,
        workspaceContext?: string
    ): Promise<string> {
        if (!this.config.enabled) {
            return userQuery;
        }

        this.outputChannel.appendLine(
            "[Context7] Enhancing query with documentation"
        );

        const context = `${userQuery}\n\n${workspaceContext || ""}`;
        const relevantDocs = await this.autoFetchRelevantDocs(context);

        if (relevantDocs.length === 0) {
            return userQuery;
        }

        // Build enhanced query with documentation context
        const docsContext = relevantDocs
            .map(
                (doc) =>
                    `\n\n=== Documentation: ${
                        doc.libraryId
                    } ===\n${doc.content.slice(0, 2000)}`
            )
            .join("\n");

        const enhanced = `${userQuery}\n\n## Available Documentation:${docsContext}`;

        this.outputChannel.appendLine(
            `[Context7] Enhanced query with ${relevantDocs.length} documentation sources`
        );

        return enhanced;
    }

    /**
     * Detect library references in text
     */
    private detectLibraries(text: string): string[] {
        const libraries = new Set<string>();

        // Common patterns for library detection
        const patterns = [
            /import\s+.*?from\s+['"]([^'"]+)['"]/g, // JavaScript/TypeScript imports
            /import\s+(\w+)/g, // Python imports
            /use\s+(\w+)::/g, // Rust use statements
            /#include\s+<([^>]+)>/g, // C/C++ includes
            /require\s*\(['"]([^'"]+)['"]\)/g, // Node.js requires
            /\b(vscode|react|vue|angular|typescript|rust|python|node)\b/gi, // Common libraries
        ];

        for (const pattern of patterns) {
            let match: RegExpExecArray | null;
            while ((match = pattern.exec(text)) !== null) {
                if (match[1]) {
                    libraries.add(match[1]);
                }
            }
        }

        return Array.from(libraries);
    }

    /**
     * Select best library match based on trust score and snippets
     */
    private selectBestMatch(
        libraries: Context7LibraryInfo[]
    ): Context7LibraryInfo {
        // Sort by trust score (descending) and code snippets (descending)
        return libraries.sort((a, b) => {
            const scoreDiff = b.trustScore - a.trustScore;
            if (Math.abs(scoreDiff) > 0.5) {
                return scoreDiff;
            }
            return b.codeSnippets - a.codeSnippets;
        })[0];
    }

    /**
     * Parse library resolution results
     */
    private parseLibraryResults(result: unknown): Context7LibraryInfo[] {
        // Handle different result formats from MCP
        if (typeof result === "string") {
            // Try to parse as JSON
            try {
                const parsed = JSON.parse(result);
                return this.extractLibraries(parsed);
            } catch {
                // Parse text format
                return this.parseTextResults(result);
            }
        }

        return this.extractLibraries(result);
    }

    /**
     * Extract library info from parsed result
     */
    private extractLibraries(data: unknown): Context7LibraryInfo[] {
        const libraries: Context7LibraryInfo[] = [];

        if (typeof data === "object" && data !== null) {
            // Check if it's an array
            if (Array.isArray(data)) {
                for (const item of data) {
                    const lib = this.parseLibraryItem(item);
                    if (lib) {
                        libraries.push(lib);
                    }
                }
            } else {
                // Single library or object with libraries property
                const obj = data as Record<string, unknown>;
                if (obj.libraries && Array.isArray(obj.libraries)) {
                    return this.extractLibraries(obj.libraries);
                }

                const lib = this.parseLibraryItem(data);
                if (lib) {
                    libraries.push(lib);
                }
            }
        }

        return libraries;
    }

    /**
     * Parse single library item
     */
    private parseLibraryItem(item: unknown): Context7LibraryInfo | null {
        if (typeof item !== "object" || item === null) {
            return null;
        }

        const obj = item as Record<string, unknown>;

        if (typeof obj.id === "string" && typeof obj.name === "string") {
            return {
                id: obj.id,
                name: obj.name,
                description:
                    typeof obj.description === "string" ? obj.description : "",
                codeSnippets:
                    typeof obj.codeSnippets === "number" ? obj.codeSnippets : 0,
                trustScore:
                    typeof obj.trustScore === "number" ? obj.trustScore : 0,
                versions: Array.isArray(obj.versions)
                    ? obj.versions
                    : undefined,
            };
        }

        return null;
    }

    /**
     * Parse text-based result format
     */
    private parseTextResults(text: string): Context7LibraryInfo[] {
        const libraries: Context7LibraryInfo[] = [];
        const lines = text.split("\n");

        let currentLib: Partial<Context7LibraryInfo> | null = null;

        for (const line of lines) {
            const trimmed = line.trim();

            if (trimmed.startsWith("- Context7-compatible library ID:")) {
                const id = trimmed.split(":")[1]?.trim();
                if (id) {
                    currentLib = { id };
                }
            } else if (trimmed.startsWith("- Title:") && currentLib) {
                currentLib.name = trimmed.split(":")[1]?.trim() || "";
            } else if (trimmed.startsWith("- Description:") && currentLib) {
                currentLib.description = trimmed.split(":")[1]?.trim() || "";
            } else if (trimmed.startsWith("- Code Snippets:") && currentLib) {
                const snippets = trimmed.split(":")[1]?.trim();
                currentLib.codeSnippets = snippets
                    ? Number.parseInt(snippets)
                    : 0;
            } else if (trimmed.startsWith("- Trust Score:") && currentLib) {
                const score = trimmed.split(":")[1]?.trim();
                currentLib.trustScore = score ? Number.parseFloat(score) : 0;
            } else if (
                trimmed === "----------" &&
                currentLib?.id &&
                currentLib?.name
            ) {
                libraries.push(currentLib as Context7LibraryInfo);
                currentLib = null;
            }
        }

        // Add last library if exists
        if (currentLib?.id && currentLib?.name) {
            libraries.push(currentLib as Context7LibraryInfo);
        }

        return libraries;
    }

    /**
     * Generate cache key
     */
    private getCacheKey(libraryId: string, topic?: string): string {
        return `${libraryId}:${topic || "default"}`;
    }

    /**
     * Check if cache entry is expired
     */
    private isCacheExpired(timestamp: number): boolean {
        const age = Date.now() - timestamp;
        return age > this.config.cacheTTLSeconds * 1000;
    }

    /**
     * Estimate token count
     */
    private estimateTokens(content: unknown): number {
        const text =
            typeof content === "string" ? content : JSON.stringify(content);
        // Rough estimate: ~4 characters per token
        return Math.ceil(text.length / 4);
    }

    /**
     * Clear cache
     */
    clearCache(): void {
        this.cache.clear();
        this.outputChannel.appendLine("[Context7] Cache cleared");
    }

    /**
     * Get cache statistics
     */
    getCacheStats(): { entries: number; totalSize: number } {
        let totalSize = 0;
        for (const entry of this.cache.values()) {
            totalSize += entry.data.tokens;
        }

        return {
            entries: this.cache.size,
            totalSize,
        };
    }

    /**
     * Update configuration
     */
    updateConfig(config: Partial<Context7Config>): void {
        this.config = { ...this.config, ...config };
        this.outputChannel.appendLine("[Context7] Configuration updated");
    }
}

/**
 * Factory function to create Context7 integration
 */
export async function createContext7Integration(
    mcpManager: McpToolManager | null,
    outputChannel: vscode.OutputChannel,
    config?: Partial<Context7Config>
): Promise<Context7Integration | null> {
    try {
        if (!mcpManager) {
            outputChannel.appendLine("[Context7] MCP manager not available");
            return null;
        }

        // Check if Context7 provider is available
        const mcpTools = mcpManager.getAvailableTools();
        const hasContext7 = mcpTools.some(
            (tool) => tool.provider === "context7"
        );

        if (!hasContext7) {
            outputChannel.appendLine(
                "[Context7] Provider not found in MCP configuration"
            );
            return null;
        }

        const integration = new Context7Integration(
            mcpManager,
            outputChannel,
            config
        );
        outputChannel.appendLine("[Context7] Integration created successfully");

        return integration;
    } catch (error) {
        outputChannel.appendLine(
            `[Context7] Failed to create integration: ${
                error instanceof Error ? error.message : String(error)
            }`
        );
        return null;
    }
}
