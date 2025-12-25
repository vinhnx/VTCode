import * as vscode from "vscode";
import { BaseCommand, type CommandContext } from "../types/command";

/**
 * Command to refresh VT Code quick actions and CLI availability
 */
export class RefreshCommand extends BaseCommand {
    public readonly id = "vtcode.refreshQuickActions";
    public readonly title = "Refresh";
    public readonly description =
        "Refresh VT Code quick actions and CLI availability";
    public readonly icon = "refresh";

    async execute(context: CommandContext): Promise<void> {
        // Refresh quick actions provider if available
        // This would be integrated with the main extension's providers

        void vscode.window.showInformationMessage(
            "VT Code quick actions refreshed."
        );

        // Trigger CLI availability check
        await vscode.commands.executeCommand("vtcode.verifyWorkspaceTrust");
    }
}
