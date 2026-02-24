import * as vscode from "vscode";
import { TaskTrackerCommand } from "../taskTrackerCommand";

describe("TaskTrackerCommand", () => {
    let command: TaskTrackerCommand;
    let mockContext: any;
    let mockFetchTasks: jest.SpiedFunction<typeof vscode.tasks.fetchTasks>;
    let mockExecuteTask: jest.SpiedFunction<typeof vscode.tasks.executeTask>;
    let mockShowWarningMessage: jest.SpiedFunction<
        typeof vscode.window.showWarningMessage
    >;
    let mockShowQuickPick: jest.SpiedFunction<typeof vscode.window.showQuickPick>;
    let mockExecuteCommand: jest.SpiedFunction<typeof vscode.commands.executeCommand>;

    beforeEach(() => {
        command = new TaskTrackerCommand();
        mockContext = {
            output: {
                appendLine: jest.fn(),
            } as any,
        };

        mockFetchTasks = jest.spyOn(vscode.tasks, "fetchTasks");
        mockExecuteTask = jest.spyOn(vscode.tasks, "executeTask");
        mockShowWarningMessage = jest.spyOn(vscode.window, "showWarningMessage");
        mockShowQuickPick = jest.spyOn(vscode.window, "showQuickPick");
        mockExecuteCommand = jest.spyOn(vscode.commands, "executeCommand");

        mockExecuteCommand.mockResolvedValue(true as any);
        jest.clearAllMocks();
    });

    afterEach(() => {
        jest.restoreAllMocks();
    });

    it("executes task-tracker task when available", async () => {
        const task = {
            name: "Task Tracker",
            definition: { type: "vtcode", command: "task-tracker" },
        } as unknown as vscode.Task;

        mockFetchTasks.mockResolvedValue([task]);
        mockExecuteTask.mockResolvedValue(undefined as any);

        await command.execute(mockContext);

        expect(mockFetchTasks).toHaveBeenCalledWith({ type: "vtcode" });
        expect(mockExecuteTask).toHaveBeenCalledWith(task);
        expect(mockShowWarningMessage).not.toHaveBeenCalled();
    });

    it("shows warning when no compatible tasks exist", async () => {
        mockFetchTasks.mockResolvedValue([]);
        mockShowWarningMessage.mockResolvedValue(undefined);

        await command.execute(mockContext);

        expect(mockShowWarningMessage).toHaveBeenCalledWith(
            "No VT Code task tracker tasks are available. Define a VT Code task in tasks.json to customize the workflow."
        );
        expect(mockExecuteTask).not.toHaveBeenCalled();
    });

    it("shows migration warning when only legacy update-plan tasks exist", async () => {
        const legacyTask = {
            name: "Legacy Update Plan",
            definition: { type: "vtcode", command: "update-plan" },
        } as unknown as vscode.Task;

        mockFetchTasks.mockResolvedValue([legacyTask]);
        mockShowWarningMessage.mockResolvedValue(undefined);

        await command.execute(mockContext);

        expect(mockShowWarningMessage).toHaveBeenCalledWith(
            'No VT Code task tracker tasks are available. Migrate tasks.json entries from command "update-plan" to "task-tracker".'
        );
        expect(mockExecuteTask).not.toHaveBeenCalled();
    });

    it("uses quick pick when multiple compatible tasks are available", async () => {
        const first = {
            name: "Tracker A",
            definition: { type: "vtcode", command: "task-tracker" },
        } as unknown as vscode.Task;
        const second = {
            name: "Tracker B",
            definition: { type: "vtcode", command: "task-tracker" },
        } as unknown as vscode.Task;

        mockFetchTasks.mockResolvedValue([first, second]);
        mockShowQuickPick.mockResolvedValue({
            label: second.name,
            task: second,
        } as any);
        mockExecuteTask.mockResolvedValue(undefined as any);

        await command.execute(mockContext);

        expect(mockShowQuickPick).toHaveBeenCalled();
        expect(mockExecuteTask).toHaveBeenCalledWith(second);
    });

    it("does nothing when quick pick selection is cancelled", async () => {
        const first = {
            name: "Tracker A",
            definition: { type: "vtcode", command: "task-tracker" },
        } as unknown as vscode.Task;
        const second = {
            name: "Tracker B",
            definition: { type: "vtcode", command: "task-tracker" },
        } as unknown as vscode.Task;

        mockFetchTasks.mockResolvedValue([first, second]);
        mockShowQuickPick.mockResolvedValue(undefined);

        await command.execute(mockContext);

        expect(mockExecuteTask).not.toHaveBeenCalled();
    });

    it("exposes updated command metadata", () => {
        expect(command.id).toBe("vtcode.runTaskTrackerTask");
        expect(command.title).toBe("Task Tracker");
        expect(command.description).toBe("Run the VT Code task tracker");
        expect(command.icon).toBe("checklist");
    });
});
