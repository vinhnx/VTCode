import * as vscode from "vscode";
import { BaseCommand, type CommandContext } from "../types/command";

/**
 * Command to trust the workspace for VT Code
 */
export class TrustWorkspaceCommand extends BaseCommand {
    public readonly id = "vtcode.trustWorkspace";
    public readonly title = "Trust Workspace";
    public readonly description =
        "Grant workspace trust to enable VT Code automation";
    public readonly icon = "shield";

    async execute(context: CommandContext): Promise<void> {
        if (vscode.workspace.isTrusted) {
            void vscode.window.showInformationMessage(
                "This workspace is already trusted for VT Code automation."
            );
            return;
        }

        const trustedNow = await this.requestWorkspaceTrust(
            "allow VT Code to process prompts with human oversight"
        );
        if (trustedNow) {
            void vscode.window.showInformationMessage(
                "Workspace trust granted. VT Code can now process prompts with human-in-the-loop safeguards."
            );
            return;
        }

        const selection = await vscode.window.showInformationMessage(
            "Workspace trust is still required for VT Code. Open the trust management settings?",
            "Manage Workspace Trust"
        );
        if (selection === "Manage Workspace Trust") {
            await vscode.commands.executeCommand(
                "workbench.action.manageTrust"
            );
            if (vscode.workspace.isTrusted) {
                void vscode.window.showInformationMessage(
                    "Workspace trust granted. VT Code can now process prompts with human-in-the-loop safeguards."
                );
            }
        }
    }

    private async requestWorkspaceTrust(action: string): Promise<boolean> {
        if (vscode.workspace.isTrusted) {
            return true;
        }

        // Use the VS Code workspace trust API if available
        const trustApi = vscode.workspace as typeof vscode.workspace & {
            requestWorkspaceTrust?: (opts?: {
                message?: string;
                modal?: boolean;
                buttons?: ReadonlyArray<vscode.MessageItem>;
            }) => Thenable<boolean | undefined>;
        };

        const requestFn = trustApi.requestWorkspaceTrust;
        if (typeof requestFn === "function") {
            try {
                const granted = await requestFn({
                    message: `VT Code requires a trusted workspace to ${action}.`,
                    modal: true,
                });
                return granted === true;
            } catch (error) {
                // Fall through to manual trust management
            }
        }

        return false;
    }
}
