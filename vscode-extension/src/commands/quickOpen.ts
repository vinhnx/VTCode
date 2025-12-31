/**
 * Quick Open Command Integration
 *
 * Replaces VS Code's built-in quick file picker with vtcode-powered file search
 * for faster, more intelligent file discovery.
 */

import * as vscode from 'vscode';
import { FileSearchService, getFileSearchService } from '../services/fileSearchService';

/**
 * Interface for quick pick item with file metadata
 */
export interface FileQuickPickItem extends vscode.QuickPickItem {
  path: string;
  score?: number;
}

/**
 * Quick open controller for file search
 */
export class QuickOpenController {
  private fileSearchService: FileSearchService;
  private quickPick: vscode.QuickPick<FileQuickPickItem> | null = null;
  private searchTimeout: NodeJS.Timeout | null = null;
  private lastQuery: string = '';

  constructor(fileSearchService: FileSearchService) {
    this.fileSearchService = fileSearchService;
  }

  /**
   * Show quick file open picker
   *
   * @returns Selected file path or undefined if cancelled
   */
  async showQuickFileOpen(): Promise<string | undefined> {
    const quickPick = vscode.window.createQuickPick<FileQuickPickItem>();
    this.quickPick = quickPick;

    quickPick.placeholder = 'Search files (type to filter)...';
    quickPick.busy = true;
    quickPick.show();

    let searchTimeout: NodeJS.Timeout | null = null;

    // Handle input changes with debouncing
    quickPick.onDidChangeValue(async (value: string) => {
      if (searchTimeout) {
        clearTimeout(searchTimeout);
      }

      if (!value) {
        quickPick.items = [];
        quickPick.busy = false;
        return;
      }

      quickPick.busy = true;
      this.lastQuery = value;

      // Debounce search (150ms)
      searchTimeout = setTimeout(async () => {
        try {
          const matches = await this.fileSearchService.searchFiles(value);

          // Check if query changed while we were searching
          if (value !== this.lastQuery) {
            return;
          }

          const items: FileQuickPickItem[] = matches.map(match => ({
            label: this.getFilename(match.path),
            description: match.path,
            path: match.path,
            score: match.score,
            detail: this.getFilePath(match.path),
          }));

          quickPick.items = items;
          quickPick.busy = false;
        } catch (error) {
          console.error('Quick open search failed:', error);
          quickPick.items = [];
          quickPick.busy = false;
        }
      }, 150);
    });

    // Handle selection
    return new Promise<string | undefined>(resolve => {
      quickPick.onDidAccept(async () => {
        const selection = quickPick.selectedItems[0];
        quickPick.hide();
        if (selection?.path) {
          resolve(selection.path);
        } else {
          resolve(undefined);
        }
      });

      quickPick.onDidHide(() => {
        quickPick.dispose();
        this.quickPick = null;
        if (searchTimeout) {
          clearTimeout(searchTimeout);
        }
        resolve(undefined);
      });
    });
  }

  /**
   * Show quick open picker for specific file type
   *
   * @param extensions - File extensions to filter
   * @returns Selected file path or undefined
   */
  async showQuickOpenForType(extensions: string[]): Promise<string | undefined> {
    const extList = extensions.join(', ');
    const quickPick = vscode.window.createQuickPick<FileQuickPickItem>();
    this.quickPick = quickPick;

    quickPick.placeholder = `Search ${extList} files...`;
    quickPick.busy = true;
    quickPick.show();

    let searchTimeout: NodeJS.Timeout | null = null;

    quickPick.onDidChangeValue(async (value: string) => {
      if (searchTimeout) {
        clearTimeout(searchTimeout);
      }

      if (!value) {
        quickPick.items = [];
        quickPick.busy = false;
        return;
      }

      quickPick.busy = true;

      searchTimeout = setTimeout(async () => {
        try {
          const matches = await this.fileSearchService.searchByExtension(value, extensions);

          const items: FileQuickPickItem[] = matches.map(match => ({
            label: this.getFilename(match.path),
            description: match.path,
            path: match.path,
            score: match.score,
          }));

          quickPick.items = items;
          quickPick.busy = false;
        } catch (error) {
          console.error('Quick open search failed:', error);
          quickPick.items = [];
          quickPick.busy = false;
        }
      }, 150);
    });

    return new Promise<string | undefined>(resolve => {
      quickPick.onDidAccept(async () => {
        const selection = quickPick.selectedItems[0];
        quickPick.hide();
        resolve(selection?.path);
      });

      quickPick.onDidHide(() => {
        quickPick.dispose();
        this.quickPick = null;
        if (searchTimeout) {
          clearTimeout(searchTimeout);
        }
        resolve(undefined);
      });
    });
  }

  /**
   * Cancel the currently active quick open
   */
  cancel(): void {
    if (this.quickPick) {
      this.quickPick.hide();
    }
  }

  /**
   * Get filename from path
   */
  private getFilename(filepath: string): string {
    return filepath.split(/[\\/]/).pop() || filepath;
  }

  /**
   * Get directory path without filename
   */
  private getFilePath(filepath: string): string {
    const parts = filepath.split(/[\\/]/);
    parts.pop();
    return parts.join('/');
  }
}

/**
 * Register quick open commands
 *
 * @param context - VS Code extension context
 * @param fileSearchService - File search service instance
 */
export function registerQuickOpenCommands(
  context: vscode.ExtensionContext,
  fileSearchService: FileSearchService
): void {
  const controller = new QuickOpenController(fileSearchService);

  // Command: Open File (Cmd+P / Ctrl+P)
  context.subscriptions.push(
    vscode.commands.registerCommand('vtcode.quickOpen', async () => {
      try {
        const filePath = await controller.showQuickFileOpen();
        if (filePath) {
          const uri = vscode.Uri.file(filePath);
          const doc = await vscode.workspace.openTextDocument(uri);
          await vscode.window.showTextDocument(doc);
        }
      } catch (error) {
        console.error('Quick open command failed:', error);
        vscode.window.showErrorMessage('Quick open failed');
      }
    })
  );

  // Command: Open Rust File (Cmd+P rust)
  context.subscriptions.push(
    vscode.commands.registerCommand('vtcode.quickOpenRust', async () => {
      try {
        const filePath = await controller.showQuickOpenForType(['rs']);
        if (filePath) {
          const uri = vscode.Uri.file(filePath);
          const doc = await vscode.workspace.openTextDocument(uri);
          await vscode.window.showTextDocument(doc);
        }
      } catch (error) {
        console.error('Quick open rust command failed:', error);
        vscode.window.showErrorMessage('Quick open failed');
      }
    })
  );

  // Command: Open TypeScript File
  context.subscriptions.push(
    vscode.commands.registerCommand('vtcode.quickOpenTypescript', async () => {
      try {
        const filePath = await controller.showQuickOpenForType(['ts', 'tsx']);
        if (filePath) {
          const uri = vscode.Uri.file(filePath);
          const doc = await vscode.workspace.openTextDocument(uri);
          await vscode.window.showTextDocument(doc);
        }
      } catch (error) {
        console.error('Quick open typescript command failed:', error);
        vscode.window.showErrorMessage('Quick open failed');
      }
    })
  );

  // Command: Open Python File
  context.subscriptions.push(
    vscode.commands.registerCommand('vtcode.quickOpenPython', async () => {
      try {
        const filePath = await controller.showQuickOpenForType(['py']);
        if (filePath) {
          const uri = vscode.Uri.file(filePath);
          const doc = await vscode.workspace.openTextDocument(uri);
          await vscode.window.showTextDocument(doc);
        }
      } catch (error) {
        console.error('Quick open python command failed:', error);
        vscode.window.showErrorMessage('Quick open failed');
      }
    })
  );
}
