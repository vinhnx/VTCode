import * as vscode from "vscode";
import { VtcodeToolCall } from "../vtcodeBackend";
import { ToolApprovalDialog } from "./toolApprovalDialog";

// Mock VS Code API
jest.mock("vscode", () => ({
    window: {
        showInformationMessage: jest.fn(),
        withProgress: jest.fn(),
    },
}));

describe("ToolApprovalDialog", () => {
    let dialog: ToolApprovalDialog;
    let mockToolCall: VtcodeToolCall;

    beforeEach(() => {
        dialog = new ToolApprovalDialog();
        mockToolCall = {
            id: "test-tool-123",
            name: "run_pty_cmd",
            args: {
                command: 'echo "Hello World"',
                cwd: "/test/path",
            },
        };

        jest.clearAllMocks();
    });

    describe("requestApproval", () => {
        it("should approve tool execution when user clicks Approve", async () => {
            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Approve");

            const result = await dialog.requestApproval(mockToolCall);

            expect(result.approved).toBe(true);
            expect(result.rememberChoice).toBe(false);
            expect(vscode.window.showInformationMessage).toHaveBeenCalled();
        });

        it("should deny tool execution when user clicks Deny", async () => {
            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Deny");

            const result = await dialog.requestApproval(mockToolCall);

            expect(result.approved).toBe(false);
            expect(result.rememberChoice).toBe(false);
        });

        it("should approve and remember when user clicks Approve & Remember", async () => {
            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Approve & Remember");

            const result = await dialog.requestApproval(mockToolCall);

            expect(result.approved).toBe(true);
            expect(result.rememberChoice).toBe(true);
            expect(result.choiceDuration).toBe("session");
        });

        it("should deny when user closes the dialog", async () => {
            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue(undefined);

            const result = await dialog.requestApproval(mockToolCall);

            expect(result.approved).toBe(false);
            expect(result.rememberChoice).toBe(false);
        });

        it("should show tool details in the dialog", async () => {
            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Approve");

            await dialog.requestApproval(mockToolCall);

            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                "VT Code wants to run: run_pty_cmd",
                expect.objectContaining({
                    modal: true,
                    detail: expect.stringContaining(
                        'Command: echo "Hello World"'
                    ),
                }),
                "Approve",
                "Approve & Remember",
                "Deny"
            );
        });

        it("should handle high-risk tools appropriately", async () => {
            const dangerousTool: VtcodeToolCall = {
                id: "dangerous-123",
                name: "delete_file",
                args: {
                    path: "/important/file.txt",
                },
            };

            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Deny");

            const result = await dialog.requestApproval(dangerousTool);

            expect(result.approved).toBe(false);
            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                "VT Code wants to run: delete_file",
                expect.objectContaining({
                    detail: expect.stringContaining("⚠️"),
                }),
                expect.anything()
            );
        });
    });

    describe("showToolProgress", () => {
        it("should show progress notification", () => {
            const mockProgress = {
                report: jest.fn(),
            };
            const mockToken = {
                isCancellationRequested: false,
            };

            (vscode.window.withProgress as jest.Mock).mockImplementation(
                (options, callback) => {
                    return callback(mockProgress, mockToken);
                }
            );

            const disposable = dialog.showToolProgress("test-tool", 1000);

            expect(vscode.window.withProgress).toHaveBeenCalled();
            expect(disposable).toBeDefined();
        });
    });

    describe("showToolSummary", () => {
        it("should show success message", () => {
            dialog.showToolSummary("test-tool", true, "Completed successfully");

            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                "test-tool completed successfully: Completed successfully"
            );
        });

        it("should show failure message", () => {
            dialog.showToolSummary("test-tool", false);

            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                "test-tool failed"
            );
        });

        it("should show message without details", () => {
            dialog.showToolSummary("test-tool", true);

            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                "test-tool completed successfully"
            );
        });
    });

    describe("risk assessment", () => {
        it("should identify high-risk tools", async () => {
            const deleteTool: VtcodeToolCall = {
                id: "test-123",
                name: "delete_file",
                args: { path: "/test.txt" },
            };

            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Deny");

            await dialog.requestApproval(deleteTool);

            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                expect.any(String),
                expect.objectContaining({
                    detail: expect.stringContaining("Risk Level: 🔴 HIGH"),
                }),
                expect.anything()
            );
        });

        it("should identify medium-risk tools", async () => {
            const editTool: VtcodeToolCall = {
                id: "test-123",
                name: "apply_diff",
                args: { path: "/test.txt", diff: "some changes" },
            };

            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Approve");

            await dialog.requestApproval(editTool);

            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                expect.any(String),
                expect.objectContaining({
                    detail: expect.stringContaining("Risk Level: 🟡 MEDIUM"),
                }),
                expect.anything()
            );
        });

        it("should identify low-risk tools", async () => {
            const safeTool: VtcodeToolCall = {
                id: "test-123",
                name: "list_files",
                args: { path: "/test" },
            };

            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Approve");

            await dialog.requestApproval(safeTool);

            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                expect.any(String),
                expect.objectContaining({
                    detail: expect.stringContaining("Risk Level: 🟢 LOW"),
                }),
                expect.anything()
            );
        });
    });

    describe("preview generation", () => {
        it("should generate preview for shell commands", async () => {
            const shellTool: VtcodeToolCall = {
                id: "test-123",
                name: "run_pty_cmd",
                args: {
                    command: "npm test",
                    cwd: "/project",
                },
            };

            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Approve");

            await dialog.requestApproval(shellTool);

            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                expect.any(String),
                expect.objectContaining({
                    detail: expect.stringContaining("Command: npm test"),
                }),
                expect.anything()
            );
        });

        it("should generate preview for file edits", async () => {
            const editTool: VtcodeToolCall = {
                id: "test-123",
                name: "apply_diff",
                args: {
                    path: "/src/test.ts",
                    diff: "+ new line\n- old line",
                },
            };

            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Approve");

            await dialog.requestApproval(editTool);

            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                expect.any(String),
                expect.objectContaining({
                    detail: expect.stringContaining("File: /src/test.ts"),
                }),
                expect.anything()
            );
        });

        it("should generate preview for file creation", async () => {
            const createTool: VtcodeToolCall = {
                id: "test-123",
                name: "create_file",
                args: {
                    path: "/src/new.ts",
                    content: 'console.log("Hello");',
                },
            };

            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Approve");

            await dialog.requestApproval(createTool);

            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                expect.any(String),
                expect.objectContaining({
                    detail: expect.stringContaining("File: /src/new.ts"),
                }),
                expect.anything()
            );
        });

        it("should generate warning for file deletion", async () => {
            const deleteTool: VtcodeToolCall = {
                id: "test-123",
                name: "delete_file",
                args: {
                    path: "/src/old.ts",
                },
            };

            (
                vscode.window.showInformationMessage as jest.Mock
            ).mockResolvedValue("Deny");

            await dialog.requestApproval(deleteTool);

            expect(vscode.window.showInformationMessage).toHaveBeenCalledWith(
                expect.any(String),
                expect.objectContaining({
                    detail: expect.stringContaining(
                        "⚠️ File will be permanently deleted"
                    ),
                }),
                expect.anything()
            );
        });
    });
});
