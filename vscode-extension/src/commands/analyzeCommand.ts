import * as vscode from "vscode";
import { BaseCommand, type CommandContext } from "../types/command";
import { runVtcodeCommand } from "../utils/vtcodeRunner";

/**
 * Command to analyze the workspace with VTCode
 */
export class AnalyzeCommand extends BaseCommand {
    public readonly id = "vtcode.runAnalyze";
    public readonly title = "Analyze Workspace";
    public readonly description = "Analyze the workspace with VTCode";
    public readonly icon = "pulse";

    async execute(context: CommandContext): Promise<void> {
        if (!this.ensureCliAvailable(context)) {
            return;
        }

        try {
            await runVtcodeCommand(["analyze"], {
                title: "Analyzing workspace with VTCode…",
                output: context.output,
            });
            void vscode.window.showInformationMessage(
                "VTCode finished analyzing the workspace. Review the VTCode output channel for results."
            );
        } catch (error) {
            this.handleCommandError("analyze the workspace", error);
        }
    }

    private handleCommandError(context: string, error: unknown): void {
        const message = error instanceof Error ? error.message : String(error);
        void vscode.window.showErrorMessage(
            `Failed to ${context} with VTCode: ${message}`
        );
    }
}