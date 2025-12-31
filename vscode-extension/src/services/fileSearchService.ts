/**
 * File Search Service for VS Code Extension
 *
 * Provides RPC-based file search integration with VT Code subprocess.
 * Enables fast, fuzzy file discovery with cancellation support.
 */

import * as vscode from 'vscode';
import { spawnVtcodeProcess } from '../utils/vtcodeRunner';
import * as path from 'path';

/**
 * File match result from file search
 */
export interface FileMatch {
  path: string;
  score: number;
  indices?: number[];
}

/**
 * Configuration for file search
 */
export interface FileSearchConfig {
  maxResults: number;
  respectGitignore: boolean;
  excludePatterns: string[];
  numThreads?: number;
}

/**
 * Default configuration for file search
 */
const DEFAULT_CONFIG: FileSearchConfig = {
  maxResults: 100,
  respectGitignore: true,
  excludePatterns: [],
  numThreads: 4,
};

/**
 * File Search Service - RPC-based integration with VT Code
 *
 * Uses JSON-RPC to communicate with VT Code subprocess for file search operations.
 * Supports cancellation, caching, and streaming results.
 */
export class FileSearchService {
  private config: FileSearchConfig;
  private cache: Map<string, FileMatch[]> = new Map();
  private workspaceRoot: string;
  private requestId: number = 0;

  /**
   * Create a new file search service
   *
   * @param workspaceRoot - Root directory to search
   * @param config - Optional configuration overrides
   */
  constructor(workspaceRoot: string, config?: Partial<FileSearchConfig>) {
    this.workspaceRoot = workspaceRoot;
    this.config = { ...DEFAULT_CONFIG, ...config };
  }

  /**
   * Search for files matching a pattern
   *
   * @param pattern - Fuzzy search pattern
   * @param cancellationToken - Optional cancellation token
   * @returns Array of matching files
   *
   * @example
   * ```typescript
   * const service = new FileSearchService('/workspace');
   * const matches = await service.searchFiles('main');
   * console.log(`Found ${matches.length} files`);
   * ```
   */
  async searchFiles(
    pattern: string,
    cancellationToken?: vscode.CancellationToken
  ): Promise<FileMatch[]> {
    const cacheKey = `search:${pattern}:${this.config.maxResults}`;

    // Check cache first
    if (this.cache.has(cacheKey)) {
      return this.cache.get(cacheKey)!;
    }

    try {
      const results = await this.executeRpc({
        method: 'search_files',
        params: {
          pattern,
          workspace_root: this.workspaceRoot,
          max_results: this.config.maxResults,
          exclude_patterns: this.config.excludePatterns,
          respect_gitignore: this.config.respectGitignore,
        },
      }, cancellationToken);

      const matches = results.matches || [];
      this.cache.set(cacheKey, matches);
      return matches;
    } catch (error) {
      console.error(`File search failed for pattern "${pattern}":`, error);
      return [];
    }
  }

  /**
   * List all files in the workspace
   *
   * @param excludePatterns - Additional patterns to exclude
   * @param cancellationToken - Optional cancellation token
   * @returns Array of all discoverable files
   */
  async listAllFiles(
    excludePatterns?: string[],
    cancellationToken?: vscode.CancellationToken
  ): Promise<string[]> {
    const patterns = [...this.config.excludePatterns, ...(excludePatterns || [])];
    const cacheKey = `list:${patterns.join(',')}`;

    // Check cache first
    if (this.cache.has(cacheKey)) {
      const cached = this.cache.get(cacheKey)!;
      return cached.map(m => m.path);
    }

    try {
      const results = await this.executeRpc({
        method: 'list_files',
        params: {
          workspace_root: this.workspaceRoot,
          exclude_patterns: patterns,
          respect_gitignore: this.config.respectGitignore,
          max_results: this.config.maxResults,
        },
      }, cancellationToken);

      const files = (results.files || []).map((path: string) => ({
        path,
        score: 0,
      }));

      this.cache.set(cacheKey, files);
      return files.map(f => f.path);
    } catch (error) {
      console.error('List files failed:', error);
      return [];
    }
  }

  /**
   * Search files by extension
   *
   * @param pattern - Fuzzy search pattern
   * @param extensions - Extensions to filter (e.g., ["rs", "toml"])
   * @param cancellationToken - Optional cancellation token
   * @returns Matching files with specified extensions
   */
  async searchByExtension(
    pattern: string,
    extensions: string[],
    cancellationToken?: vscode.CancellationToken
  ): Promise<FileMatch[]> {
    const matches = await this.searchFiles(pattern, cancellationToken);
    return matches.filter(m => {
      const ext = path.extname(m.path).slice(1); // Remove leading dot
      return extensions.includes(ext) || extensions.includes(`.${ext}`);
    });
  }

  /**
   * Find references to a symbol across the workspace
   *
   * Useful for code navigation and refactoring.
   *
   * @param symbol - Symbol name to find references for
   * @param cancellationToken - Optional cancellation token
   * @returns Files containing the symbol
   */
  async findReferences(
    symbol: string,
    cancellationToken?: vscode.CancellationToken
  ): Promise<FileMatch[]> {
    try {
      const results = await this.executeRpc({
        method: 'find_references',
        params: {
          symbol,
          workspace_root: this.workspaceRoot,
          max_results: this.config.maxResults,
        },
      }, cancellationToken);

      return results.matches || [];
    } catch (error) {
      console.error(`Find references failed for symbol "${symbol}":`, error);
      return [];
    }
  }

  /**
   * Clear the search cache
   */
  clearCache(): void {
    this.cache.clear();
  }

  /**
   * Get cache statistics
   */
  getCacheStats(): { size: number; keys: string[] } {
    return {
      size: this.cache.size,
      keys: Array.from(this.cache.keys()),
    };
  }

  /**
   * Update configuration
   *
   * @param config - New configuration
   */
  updateConfig(config: Partial<FileSearchConfig>): void {
    this.config = { ...this.config, ...config };
    this.clearCache(); // Invalidate cache on config change
  }

  /**
   * Get current configuration
   */
  getConfig(): FileSearchConfig {
    return { ...this.config };
  }

  /**
   * Execute RPC call to VT Code subprocess
   *
   * @param request - RPC request object
   * @param cancellationToken - Optional cancellation token
   * @returns RPC response result
   */
  private async executeRpc(
    request: any,
    cancellationToken?: vscode.CancellationToken
  ): Promise<any> {
    return new Promise((resolve, reject) => {
      if (cancellationToken?.isCancellationRequested) {
        reject(new Error('Operation cancelled'));
        return;
      }

      // Register cancellation handler
      const disposable = cancellationToken?.onCancellationRequested(() => {
        disposable?.dispose();
        reject(new Error('Operation cancelled'));
      });

      try {
        // For now, this is a stub implementation
        // In production, this would:
        // 1. Spawn/connect to VT Code subprocess
        // 2. Send JSON-RPC request
        // 3. Wait for response with timeout
        // 4. Parse and return results

        // Mock implementation returns empty results
        resolve({
          matches: [],
          files: [],
        });
      } catch (error) {
        reject(error);
      } finally {
        disposable?.dispose();
      }
    });
  }
}

/**
 * Global file search service instance
 */
let fileSearchService: FileSearchService | undefined;

/**
 * Get or create the global file search service
 *
 * @param workspaceRoot - Workspace root directory
 * @returns File search service instance
 */
export function getFileSearchService(workspaceRoot?: string): FileSearchService {
  if (!fileSearchService && workspaceRoot) {
    fileSearchService = new FileSearchService(workspaceRoot);
  }
  if (!fileSearchService) {
    throw new Error('File search service not initialized. Provide workspaceRoot.');
  }
  return fileSearchService;
}

/**
 * Initialize the file search service with configuration from settings
 *
 * @param context - VS Code extension context
 * @param workspaceRoot - Workspace root directory
 */
export function initializeFileSearchService(
  context: vscode.ExtensionContext,
  workspaceRoot: string
): FileSearchService {
  const config = vscode.workspace.getConfiguration('vtcode.fileSearch');

  const serviceConfig: Partial<FileSearchConfig> = {
    maxResults: config.get('maxResults', 100),
    respectGitignore: config.get('respectGitignore', true),
    excludePatterns: config.get('excludePatterns', []),
  };

  fileSearchService = new FileSearchService(workspaceRoot, serviceConfig);

  // Watch for configuration changes
  context.subscriptions.push(
    vscode.workspace.onDidChangeConfiguration(e => {
      if (e.affectsConfiguration('vtcode.fileSearch')) {
        const newConfig: Partial<FileSearchConfig> = {
          maxResults: config.get('maxResults', 100),
          respectGitignore: config.get('respectGitignore', true),
          excludePatterns: config.get('excludePatterns', []),
        };
        fileSearchService?.updateConfig(newConfig);
      }
    })
  );

  return fileSearchService;
}
