import * as vscode from "vscode";
import { BaseParticipant, type ParticipantContext } from "../types/participant";

/**
 * Git participant provides git repository context
 */
export class GitParticipant extends BaseParticipant {
    public readonly id = "git";
    public readonly displayName = "Git";
    public readonly description = "Provides git repository context and change information";
    public readonly icon = "git-branch";

    canHandle(context: ParticipantContext): boolean {
        // Available when git context is provided
        return context.git !== undefined;
    }

    async resolveReferenceContext(message: string, context: ParticipantContext): Promise<string> {
        if (!this.extractMention(message, this.id)) {
            return message;
        }

        const git = context.git;
        if (!git) {
            return message;
        }

        // Clean the message first
        const cleanedMessage = this.cleanMessage(message, this.id);

        // Build git context
        let gitContext = `\n\n## Git Context\n`;
        gitContext += `Branch: ${git.branch}\n`;
        
        if (git.repoPath) {
            gitContext += `Repository: ${git.repoPath}\n`;
        }

        // Add change information
        if (git.changes && git.changes.length > 0) {
            gitContext += `\nChanges in working directory:\n`;
            git.changes.forEach((change, index) => {
                gitContext += `${index + 1}. ${change}\n`;
            });
        } else {
            gitContext += `\nWorking directory is clean (no changes)\n`;
        }

        // Add git status summary
        const statusSummary = this.getGitStatusSummary(git.changes);
        if (statusSummary) {
            gitContext += `\nStatus: ${statusSummary}\n`;
        }

        return `${cleanedMessage}${gitContext}`;
    }

    private getGitStatusSummary(changes: string[]): string | undefined {
        if (!changes || changes.length === 0) {
            return "Clean working directory";
        }

        const statusCounts = changes.reduce((acc, change) => {
            const status = change.split(/\s+/)[0];
            acc[status] = (acc[status] || 0) + 1;
            return acc;
        }, {} as Record<string, number>);

        const summaries: string[] = [];
        if (statusCounts['M']) summaries.push(`${statusCounts['M']} modified`);
        if (statusCounts['A']) summaries.push(`${statusCounts['A']} added`);
        if (statusCounts['D']) summaries.push(`${statusCounts['D']} deleted`);
        if (statusCounts['??']) summaries.push(`${statusCounts['??']} untracked`);

        return summaries.join(', ');
    }
}