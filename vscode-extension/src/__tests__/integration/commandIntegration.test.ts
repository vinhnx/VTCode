import * as vscode from "vscode";
import { CommandRegistry } from "../../commandRegistry";
import { AskCommand, AnalyzeCommand, OpenConfigCommand } from "../../commands";
import * as vtcodeRunner from "../../utils/vtcodeRunner";

// Mock the vtcodeRunner module
jest.mock("../../utils/vtcodeRunner");

describe("Command Integration Tests", () => {
    let registry: CommandRegistry;
    let mockContext: any;

    beforeEach(() => {
        registry = new CommandRegistry();
        mockContext = {
            workspace: undefined,
            activeTextEditor: undefined,
            selection: undefined,
            terminal: undefined,
            output: {
                appendLine: jest.fn(),
            } as any,
        };

        // Reset mocks
        jest.clearAllMocks();
    });

    afterEach(() => {
        jest.restoreAllMocks();
    });

    describe("Command Registration and Execution", () => {
        it("should register and execute multiple commands", async () => {
            // Arrange
            const askCommand = new AskCommand();
            const analyzeCommand = new AnalyzeCommand();
            const openConfigCommand = new OpenConfigCommand();

            registry.registerAll([askCommand, analyzeCommand, openConfigCommand]);

            // Mock command execution
            (vtcodeRunner.runVtcodeCommand as jest.Mock).mockResolvedValue(undefined);
            (vtcodeRunner.ensureCliAvailableForCommand as jest.Mock).mockResolvedValue(true);
            jest.spyOn(vscode.window, "showInputBox").mockResolvedValue("Test question");
            jest.spyOn(vscode.window, "showInformationMessage").mockResolvedValue(undefined as any);

            // Act & Assert
            // Execute ask command
            const askResult = await registry.executeCommand("vtcode.askAgent", mockContext);
            expect(askResult).toBeUndefined();
            expect(vtcodeRunner.runVtcodeCommand).toHaveBeenCalledWith(
                ["ask", "Test question"],
                expect.any(Object)
            );

            // Execute analyze command
            const analyzeResult = await registry.executeCommand("vtcode.runAnalyze", mockContext);
            expect(analyzeResult).toBeUndefined();
            expect(vtcodeRunner.runVtcodeCommand).toHaveBeenCalledWith(
                ["analyze"],
                expect.any(Object)
            );

            // Execute open config command
            const openConfigResult = await registry.executeCommand("vtcode.openConfig", mockContext);
            expect(openConfigResult).toBeUndefined();
        });

        it("should handle command execution errors gracefully", async () => {
            // Arrange
            const askCommand = new AskCommand();
            registry.register(askCommand);

            const mockError = new Error("Command execution failed");
            (vtcodeRunner.runVtcodeCommand as jest.Mock).mockRejectedValue(mockError);
            jest.spyOn(vscode.window, "showInputBox").mockResolvedValue("Test question");
            jest.spyOn(vscode.window, "showErrorMessage").mockResolvedValue(undefined as any);

            // Act
            const result = await registry.executeCommand("vtcode.askAgent", mockContext);

            // Assert
            expect(result).toBeUndefined();
            expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
                "Failed to ask with VTCode: Command execution failed"
            );
        });

        it("should handle non-existent commands", async () => {
            // Arrange
            const askCommand = new AskCommand();
            registry.register(askCommand);

            // Act
            const result = await registry.executeCommand("vtcode.nonExistentCommand", mockContext);

            // Assert
            expect(result).toBeUndefined();
            expect(vscode.window.showErrorMessage).toHaveBeenCalledWith(
                'Command "vtcode.nonExistentCommand" not found'
            );
        });

        it("should check command availability before execution", async () => {
            // Arrange
            const analyzeCommand = new AnalyzeCommand();
            registry.register(analyzeCommand);

            (vtcodeRunner.ensureCliAvailableForCommand as jest.Mock).mockResolvedValue(false);

            // Act
            const result = await registry.executeCommand("vtcode.runAnalyze", mockContext);

            // Assert
            expect(result).toBe(false);
            expect(vtcodeRunner.runVtcodeCommand).not.toHaveBeenCalled();
        });
    });

    describe("Command Registry Management", () => {
        it("should register and unregister commands", () => {
            // Arrange
            const askCommand = new AskCommand();
            const analyzeCommand = new AnalyzeCommand();

            // Act
            registry.register(askCommand);
            registry.register(analyzeCommand);

            // Assert
            expect(registry.getAll()).toHaveLength(2);
            expect(registry.get("vtcode.askAgent")).toBe(askCommand);
            expect(registry.get("vtcode.runAnalyze")).toBe(analyzeCommand);

            // Unregister
            registry.unregister("vtcode.askAgent");
            expect(registry.getAll()).toHaveLength(1);
            expect(registry.get("vtcode.askAgent")).toBeUndefined();
        });

        it("should clear all registered commands", () => {
            // Arrange
            const askCommand = new AskCommand();
            const analyzeCommand = new AnalyzeCommand();
            registry.registerAll([askCommand, analyzeCommand]);

            // Act
            registry.clear();

            // Assert
            expect(registry.getAll()).toHaveLength(0);
        });

        it("should prevent duplicate command registration", () => {
            // Arrange
            const askCommand1 = new AskCommand();
            const askCommand2 = new AskCommand();

            // Act
            registry.register(askCommand1);
            registry.register(askCommand2);

            // Assert
            expect(registry.getAll()).toHaveLength(1);
            expect(registry.get("vtcode.askAgent")).toBe(askCommand2);
        });
    });

    describe("Command Context Handling", () => {
        it("should pass context to commands", async () => {
            // Arrange
            const askCommand = new AskCommand();
            registry.register(askCommand);

            const customContext = {
                ...mockContext,
                workspace: {
                    uri: { fsPath: "/custom/workspace" },
                    name: "Custom Workspace",
                    index: 0,
                } as vscode.WorkspaceFolder,
            };

            (vtcodeRunner.runVtcodeCommand as jest.Mock).mockResolvedValue(undefined);
            jest.spyOn(vscode.window, "showInputBox").mockResolvedValue("Test question");
            jest.spyOn(vscode.window, "showInformationMessage").mockResolvedValue(undefined as any);

            // Act
            await registry.executeCommand("vtcode.askAgent", customContext);

            // Assert
            expect(vtcodeRunner.runVtcodeCommand).toHaveBeenCalledWith(
                ["ask", "Test question"],
                expect.objectContaining({
                    output: customContext.output,
                })
            );
        });
    });
});