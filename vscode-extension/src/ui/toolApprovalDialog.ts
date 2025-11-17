import * as vscode from 'vscode';
import { VtcodeToolCall } from '../vtcodeBackend';

export interface ToolApprovalOptions {
    readonly showPreview: boolean;
    readonly autoApproveSimilar: boolean;
    readonly timeoutMs: number;
}

export interface ToolApprovalResult {
    readonly approved: boolean;
    readonly rememberChoice: boolean;
    readonly choiceDuration?: 'session' | 'forever';
}

/**
 * Enhanced tool approval dialog with better UX
 * Provides detailed context, previews, and approval options
 */
export class ToolApprovalDialog {
    private readonly defaultOptions: ToolApprovalOptions = {
        showPreview: true,
        autoApproveSimilar: false,
        timeoutMs: 30000, // 30 second timeout
    };

    /**
     * Request approval for a tool execution with enhanced UI
     */
    public async requestApproval(
        toolCall: VtcodeToolCall,
        options?: Partial<ToolApprovalOptions>
    ): Promise<ToolApprovalResult> {
        const opts = { ...this.defaultOptions, ...options };
        
        // Create enhanced dialog with tool details
        const toolName = toolCall.name;
        const args = toolCall.args;
        
        // Build detailed information
        const details = this.buildToolDetails(toolName, args);
        const preview = opts.showPreview ? this.generatePreview(toolName, args) : '';
        const riskLevel = this.assessRiskLevel(toolName, args);
        
        // Create buttons with appropriate order and styling
        const approveButton = 'Approve';
        const denyButton = 'Deny';
        const approveSimilarButton = 'Approve & Remember';
        
        const buttons = [approveButton, approveSimilarButton, denyButton];
        
        // Show dialog with enhanced information
        const result = await vscode.window.showInformationMessage(
            `VTCode wants to run: ${toolName}`,
            {
                modal: true,
                detail: this.formatDialogDetails(details, preview, riskLevel),
            },
            ...buttons
        );

        // Handle timeout or cancellation
        if (!result) {
            return {
                approved: false,
                rememberChoice: false,
            };
        }

        // Parse the user's choice
        switch (result) {
            case approveButton:
                return {
                    approved: true,
                    rememberChoice: false,
                };
            
            case approveSimilarButton:
                return {
                    approved: true,
                    rememberChoice: true,
                    choiceDuration: 'session',
                };
            
            case denyButton:
                return {
                    approved: false,
                    rememberChoice: false,
                };
            
            default:
                return {
                    approved: false,
                    rememberChoice: false,
                };
        }
    }

    /**
     * Build detailed tool information for display
     */
    private buildToolDetails(toolName: string, args: Record<string, unknown>): string {
        const details: string[] = [];
        
        // Add tool description
        const description = this.getToolDescription(toolName);
        if (description) {
            details.push(`Description: ${description}`);
        }

        // Add parameter information
        const paramInfo = this.getParameterInfo(toolName, args);
        if (paramInfo) {
            details.push(`Parameters: ${paramInfo}`);
        }

        // Add potential impact
        const impact = this.assessImpact(toolName, args);
        if (impact) {
            details.push(`Impact: ${impact}`);
        }

        return details.join('\n');
    }

    /**
     * Generate preview of what the tool will do
     */
    private generatePreview(toolName: string, args: Record<string, unknown>): string {
        switch (toolName.toLowerCase()) {
            case 'run_terminal_cmd':
            case 'run_shell_command':
                return this.previewShellCommand(args);
            
            case 'apply_diff':
            case 'edit_file':
                return this.previewFileEdit(args);
            
            case 'create_file':
            case 'write_file':
                return this.previewFileCreate(args);
            
            case 'delete_file':
                return this.previewFileDelete(args);
            
            case 'mcp_tool':
                return this.previewMcpTool(args);
            
            default:
                return JSON.stringify(args, null, 2);
        }
    }

    /**
     * Assess risk level of the tool
     */
    private assessRiskLevel(toolName: string, args: Record<string, unknown>): 'low' | 'medium' | 'high' {
        const riskyTools = ['delete_file', 'format_disk', 'rm_rf'];
        const mediumRiskTools = ['apply_diff', 'edit_file', 'run_terminal_cmd'];
        
        const normalizedName = toolName.toLowerCase();
        
        if (riskyTools.some(rt => normalizedName.includes(rt))) {
            return 'high';
        }
        
        if (mediumRiskTools.some(mrt => normalizedName.includes(mrt))) {
            return 'medium';
        }
        
        return 'low';
    }

    /**
     * Get tool description
     */
    private getToolDescription(toolName: string): string | undefined {
        const descriptions: Record<string, string> = {
            'run_terminal_cmd': 'Execute a command in the terminal',
            'apply_diff': 'Apply changes to a file',
            'create_file': 'Create a new file',
            'delete_file': 'Delete a file permanently',
            'mcp_tool': 'Execute an external tool via MCP',
        };
        
        return descriptions[toolName] || descriptions[toolName.toLowerCase()];
    }

    /**
     * Get parameter information
     */
    private getParameterInfo(toolName: string, args: Record<string, unknown>): string | undefined {
        const keys = Object.keys(args);
        if (keys.length === 0) return 'No parameters';
        
        return keys.map(key => {
            const value = args[key];
            const preview = typeof value === 'string' 
                ? (value.length > 50 ? `${value.slice(0, 47)}...` : value)
                : JSON.stringify(value);
            return `${key}: ${preview}`;
        }).join(', ');
    }

    /**
     * Assess potential impact
     */
    private assessImpact(toolName: string, args: Record<string, unknown>): string | undefined {
        switch (toolName.toLowerCase()) {
            case 'delete_file':
                return 'File will be permanently deleted';
            
            case 'apply_diff':
                return '💾 File contents will be modified';
            
            case 'run_terminal_cmd': {
                const command = args.command as string || '';
                if (command.includes('rm ') || command.includes('delete')) {
                    return 'Potentially destructive command';
                }
                return '💻 Command will be executed in terminal';
            }
            
            default:
                return undefined;
        }
    }

    /**
     * Preview shell command execution
     */
    private previewShellCommand(args: Record<string, unknown>): string {
        const command = args.command as string || '';
        const cwd = args.cwd as string || process.cwd();
        
        return `Command: ${command}\nDirectory: ${cwd}\n\nThis command will be executed in the terminal.`;
    }

    /**
     * Preview file edit
     */
    private previewFileEdit(args: Record<string, unknown>): string {
        const path = args.path as string || 'unknown file';
        const diff = args.diff as string || '';
        
        return `File: ${path}\n\nChanges:\n${diff.slice(0, 200)}${diff.length > 200 ? '...' : ''}`;
    }

    /**
     * Preview file creation
     */
    private previewFileCreate(args: Record<string, unknown>): string {
        const path = args.path as string || 'unknown file';
        const content = args.content as string || '';
        
        return `File: ${path}\n\nContent preview:\n${content.slice(0, 150)}${content.length > 150 ? '...' : ''}`;
    }

    /**
     * Preview file deletion
     */
    private previewFileDelete(args: Record<string, unknown>): string {
        const path = args.path as string || 'unknown file';
        return `File will be permanently deleted: ${path}`;
    }

    /**
     * Preview MCP tool execution
     */
    private previewMcpTool(args: Record<string, unknown>): string {
        const toolName = args.toolName as string || 'unknown tool';
        const toolArgs = args.args || {};
        
        return `MCP Tool: ${toolName}\nArguments: ${JSON.stringify(toolArgs, null, 2)}`;
    }

    /**
     * Format dialog details for display
     */
    private formatDialogDetails(details: string, preview: string, riskLevel: string): string {
        const parts: string[] = [];
        
        if (details) {
            parts.push(details);
        }
        
        if (preview) {
            parts.push('\nPreview:\n' + preview);
        }
        
        // Add risk indicator
        parts.push(`\nRisk Level: ${riskLevel.toUpperCase()}`);
        
        return parts.join('\n');
    }

    /**
     * Show a progress notification for long-running tools
     */
    public showToolProgress(toolName: string, durationMs: number): vscode.Disposable {
        return vscode.window.withProgress({
            location: vscode.ProgressLocation.Notification,
            title: `Running ${toolName}...`,
            cancellable: true,
        }, async (progress, token) => {
            const startTime = Date.now();
            
            return new Promise<void>((resolve) => {
                const interval = setInterval(() => {
                    const elapsed = Date.now() - startTime;
                    const percentage = Math.min((elapsed / durationMs) * 100, 100);
                    
                    progress.report({ 
                        increment: percentage - (progress as any).lastIncrement || 0,
                        message: `${Math.round(percentage)}% complete`
                    });
                    
                    (progress as any).lastIncrement = percentage;
                    
                    if (elapsed >= durationMs || token.isCancellationRequested) {
                        clearInterval(interval);
                        resolve();
                    }
                }, 100);
            });
        });
    }

    /**
     * Show a summary of tool execution results
     */
    public showToolSummary(toolName: string, success: boolean, details?: string): void {
        const message = `${toolName} ${success ? 'completed successfully' : 'failed'}`;

        if (details) {
            vscode.window.showInformationMessage(`${message}: ${details}`);
        } else {
            vscode.window.showInformationMessage(message);
        }
    }
}