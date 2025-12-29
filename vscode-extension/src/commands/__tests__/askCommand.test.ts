import * as vscode from "vscode";
import * as vtcodeRunner from "../../utils/vtcodeRunner";
import { AskCommand } from "../askCommand";

// Mock the vtcodeRunner module
jest.mock("../../utils/vtcodeRunner");

describe("AskCommand", () => {
    let command: AskCommand;
    let mockContext: any;
    let mockShowInputBox: jest.SpiedFunction<typeof vscode.window.showInputBox>;
    let mockShowInformationMessage: jest.SpiedFunction<
        typeof vscode.window.showInformationMessage
    >;
    let mockShowErrorMessage: jest.SpiedFunction<
        typeof vscode.window.showErrorMessage
    >;

    beforeEach(() => {
        command = new AskCommand();
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
        mockShowInputBox = jest.spyOn(vscode.window, "showInputBox");
        mockShowInformationMessage = jest.spyOn(
            vscode.window,
            "showInformationMessage"
        );
        mockShowErrorMessage = jest.spyOn(vscode.window, "showErrorMessage");

        // Reset mocks
        jest.clearAllMocks();
    });

    afterEach(() => {
        jest.restoreAllMocks();
    });

    describe("execute", () => {
        it("should prompt user for question and execute vtcode command", async () => {
            // Arrange
            const mockQuestion = "What is the meaning of life?";
            mockShowInputBox.mockResolvedValue(mockQuestion);
            (vtcodeRunner.runVtcodeCommand as jest.Mock).mockResolvedValue(
                undefined
            );
            mockShowInformationMessage.mockResolvedValue(undefined as any);

            // Act
            await command.execute(mockContext);

            // Assert
            expect(mockShowInputBox).toHaveBeenCalledWith({
                prompt: "What would you like the VT Code agent to help with?",
                placeHolder: "Summarize src/main.rs",
                ignoreFocusOut: true,
            });
            expect(vtcodeRunner.runVtcodeCommand).toHaveBeenCalledWith(
                ["ask", mockQuestion],
                {
                    title: "Asking VT Code…",
                    output: mockContext.output,
                }
            );
            expect(mockShowInformationMessage).toHaveBeenCalledWith(
                "VT Code finished processing your request. Check the VT Code output channel for details."
            );
        });

        it("should handle empty question gracefully", async () => {
            // Arrange
            mockShowInputBox.mockResolvedValue("");
            (vtcodeRunner.runVtcodeCommand as jest.Mock).mockResolvedValue(
                undefined
            );

            // Act
            await command.execute(mockContext);

            // Assert
            expect(vtcodeRunner.runVtcodeCommand).not.toHaveBeenCalled();
            expect(mockShowInformationMessage).not.toHaveBeenCalled();
        });

        it("should handle cancelled input", async () => {
            // Arrange
            mockShowInputBox.mockResolvedValue(undefined);
            (vtcodeRunner.runVtcodeCommand as jest.Mock).mockResolvedValue(
                undefined
            );

            // Act
            await command.execute(mockContext);

            // Assert
            expect(vtcodeRunner.runVtcodeCommand).not.toHaveBeenCalled();
            expect(mockShowInformationMessage).not.toHaveBeenCalled();
        });

        it("should handle command execution errors", async () => {
            // Arrange
            const mockQuestion = "Test question";
            const mockError = new Error("Command failed");
            mockShowInputBox.mockResolvedValue(mockQuestion);
            (vtcodeRunner.runVtcodeCommand as jest.Mock).mockRejectedValue(
                mockError
            );
            mockShowErrorMessage.mockResolvedValue(undefined as any);

            // Act
            await command.execute(mockContext);

            // Assert
            expect(vtcodeRunner.runVtcodeCommand).toHaveBeenCalled();
            expect(mockShowErrorMessage).toHaveBeenCalledWith(
                "Failed to ask with VT Code: Command failed"
            );
            expect(mockShowInformationMessage).not.toHaveBeenCalled();
        });

        it("should trim whitespace from question", async () => {
            // Arrange
            const mockQuestion = "  Test question  ";
            mockShowInputBox.mockResolvedValue(mockQuestion);
            (vtcodeRunner.runVtcodeCommand as jest.Mock).mockResolvedValue(
                undefined
            );
            mockShowInformationMessage.mockResolvedValue(undefined as any);

            // Act
            await command.execute(mockContext);

            // Assert
            expect(vtcodeRunner.runVtcodeCommand).toHaveBeenCalledWith(
                ["ask", "Test question"],
                expect.any(Object)
            );
        });
    });

    describe("command metadata", () => {
        it("should have correct command metadata", () => {
            expect(command.id).toBe("vtcode.askAgent");
            expect(command.title).toBe("Ask Agent");
            expect(command.description).toBe(
                "Ask the VT Code agent a question"
            );
            expect(command.icon).toBe("comment-discussion");
        });
    });
});
