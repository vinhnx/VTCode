import * as vscode from "vscode";
import { BaseCommand, type CommandContext } from "../types/command";
import { type VtcodeTaskDefinition } from "../utils/vtcodeRunner";

/**
 * Command to update the VT Code task plan
 */
export class UpdatePlanCommand extends BaseCommand {
    public readonly id = "vtcode.runUpdatePlanTask";
    public readonly title = "Update Plan";
    public readonly description = "Update the VT Code task plan";
    public readonly icon = "checklist";

    async execute(context: CommandContext): Promise<void> {
        if (!this.ensureCliAvailable(context)) {
            return;
        }

        const tasks = await vscode.tasks.fetchTasks({ type: "vtcode" });
        const updatePlanTasks = tasks.filter(
            (task) =>
                (task.definition as VtcodeTaskDefinition).command ===
                "update-plan"
        );

        if (updatePlanTasks.length === 0) {
            void vscode.window.showWarningMessage(
                "No VT Code update plan tasks are available. Define a VT Code task in tasks.json to customize the workflow."
            );
            return;
        }

        let taskToRun: vscode.Task | undefined;
        if (updatePlanTasks.length === 1) {
            taskToRun = updatePlanTasks[0];
        } else {
            const pickItems = updatePlanTasks.map((task) => ({
                label: task.name,
                task,
            }));
            const selection = await vscode.window.showQuickPick(pickItems, {
                placeHolder: "Select the VT Code plan task to run",
            });
            taskToRun = selection?.task;
        }

        if (!taskToRun) {
            return;
        }

        await this.flushIdeContextSnapshot(context);
        await vscode.tasks.executeTask(taskToRun);
    }
}
