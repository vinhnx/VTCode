import * as vscode from "vscode";
import { AnalyzeCommand } from "../analyzeCommand";
import * as vtcodeRunner from "../../utils/vtcodeRunner";

// Mock the vtcodeRunner module
jest.mock("../../utils/vtcodeRunner");

describe("AnalyzeCommand", () => {
    let command: AnalyzeCommand;
    let mockContext: any;
    let mockShowInformationMessage: jest.SpiedFunction<typeof vscode.window.showInformationMessage>;
    let mockShowErrorMessage: jest.SpiedFunction<typeof vscode.window.showErrorMessage>;

    beforeEach(() => {
        command = new AnalyzeCommand();
        mockContext = {
            workspace: undefined,
            activeTextEditor: undefined,
            selection: undefined,
            terminal: undefined,
            output: {
                appendLine: jest.fn(),
            } as any,
        };

        // Mock VS Code API
        mockShowInformationMessage = jest.spyOn(vscode.window, "showInformationMessage");
        mockShowErrorMessage = jest.spyOn(vscode.window, "showErrorMessage");

        // Reset mocks
        jest.clearAllMocks();
    });

    afterEach(() => {
        jest.restoreAllMocks();
    });

    describe("execute", () => {
        it("should execute analyze command successfully", async () => {
            // Arrange
            (vtcodeRunner.runVtcodeCommand as jest.Mock).mockResolvedValue(undefined);
            mockShowInformationMessage.mockResolvedValue(undefined as any);

            // Act
            await command.execute(mockContext);

            // Assert
            expect(vtcodeRunner.runVtcodeCommand).toHaveBeenCalledWith(
                ["analyze"],
                {
                    title: "Analyzing workspace with VTCode…",
                    output: mockContext.output,
                }
            );
            expect(mockShowInformationMessage).toHaveBeenCalledWith(
                "VTCode finished analyzing the workspace. Review the VTCode output channel for results."
            );
        });

        it("should handle command execution errors", async () => {
            // Arrange
            const mockError = new Error("Analysis failed");
            (vtcodeRunner.runVtcodeCommand as jest.Mock).mockRejectedValue(mockError);
            mockShowErrorMessage.mockResolvedValue(undefined as any);

            // Act
            await command.execute(mockContext);

            // Assert
            expect(vtcodeRunner.runVtcodeCommand).toHaveBeenCalled();
            expect(mockShowErrorMessage).toHaveBeenCalledWith(
                "Failed to analyze the workspace with VTCode: Analysis failed"
            );
            expect(mockShowInformationMessage).not.toHaveBeenCalled();
        });

        it("should handle CLI unavailability", async () => {
            // Arrange
            (vtcodeRunner.runVtcodeCommand as jest.Mock).mockRejectedValue(
                new Error("CLI not available")
            );
            mockShowErrorMessage.mockResolvedValue(undefined as any);

            // Act
            await command.execute(mockContext);

            // Assert
            expect(mockShowErrorMessage).toHaveBeenCalled();
        });
    });

    describe("command metadata", () => {
        it("should have correct command metadata", () => {
            expect(command.id).toBe("vtcode.runAnalyze");
            expect(command.title).toBe("Analyze Workspace");
            expect(command.description).toBe("Analyze the current workspace with VTCode");
            expect(command.icon).toBe("search");
        });
    });

    describe("canExecute", () => {
        it("should return true when CLI is available", async () => {
            // Arrange
            (vtcodeRunner.ensureCliAvailableForCommand as jest.Mock).mockResolvedValue(true);

            // Act
            const result = await command.canExecute(mockContext);

            // Assert
            expect(result).toBe(true);
        });

        it("should return false when CLI is not available", async () => {
            // Arrange
            (vtcodeRunner.ensureCliAvailableForCommand as jest.Mock).mockResolvedValue(false);

            // Act
            const result = await command.canExecute(mockContext);

            // Assert
            expect(result).toBe(false);
        });
    });
});