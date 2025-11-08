import * as vscode from "vscode";
import { WorkspaceParticipant } from "../workspaceParticipant";
import { ParticipantContext } from "../../types/participant";

// Mock VS Code API
jest.mock("vscode");

describe("WorkspaceParticipant", () => {
    let participant: WorkspaceParticipant;
    let mockContext: ParticipantContext;

    beforeEach(() => {
        participant = new WorkspaceParticipant();
        mockContext = {
            workspace: {
                uri: {
                    fsPath: "/workspace/test-project",
                },
                name: "test-project",
                index: 0,
            } as vscode.WorkspaceFolder,
            activeFile: undefined,
            terminal: undefined,
            git: undefined,
            commandHistory: [],
        };

        // Mock vscode.workspace.findFiles
        (vscode.workspace.findFiles as jest.Mock).mockResolvedValue([
            { fsPath: "/workspace/test-project/src/file1.ts" },
            { fsPath: "/workspace/test-project/src/file2.ts" },
        ]);

        // Mock vscode.workspace.asRelativePath
        (vscode.workspace.asRelativePath as jest.Mock).mockImplementation(
            (path: string) => path.replace("/workspace/test-project/", "")
        );

        // Mock vscode.window.visibleTextEditors
        (vscode.window.visibleTextEditors as any) = [
            {
                document: {
                    fileName: "/workspace/test-project/src/file1.ts",
                },
            },
            {
                document: {
                    fileName: "/workspace/test-project/src/file2.ts",
                },
            },
        ];
    });

    afterEach(() => {
        jest.clearAllMocks();
    });

    describe("canHandle", () => {
        it("should return true when workspace is available", () => {
            expect(participant.canHandle(mockContext)).toBe(true);
        });

        it("should return false when workspace is undefined", () => {
            mockContext.workspace = undefined;
            expect(participant.canHandle(mockContext)).toBe(false);
        });
    });

    describe("resolveReferenceContext", () => {
        it("should add workspace context when @workspace is mentioned", async () => {
            // Arrange
            const message = "@workspace What files are in this project?";

            // Act
            const result = await participant.resolveReferenceContext(message, mockContext);

            // Assert
            expect(result).toContain("What files are in this project?");
            expect(result).toContain("## Workspace Context");
            expect(result).toContain("Workspace: test-project");
            expect(result).toContain("Path: /workspace/test-project");
            expect(result).toContain("Files in workspace: 2");
            expect(result).toContain("Currently open files:");
            expect(result).toContain("src/file1.ts");
            expect(result).toContain("src/file2.ts");
        });

        it("should not modify message when @workspace is not mentioned", async () => {
            // Arrange
            const message = "What files are in this project?";

            // Act
            const result = await participant.resolveReferenceContext(message, mockContext);

            // Assert
            expect(result).toBe("What files are in this project?");
        });

        it("should handle message without workspace", async () => {
            // Arrange
            const message = "@workspace What files?";
            mockContext.workspace = undefined;

            // Act
            const result = await participant.resolveReferenceContext(message, mockContext);

            // Assert
            expect(result).toBe("What files?");
        });

        it("should include active file information if available", async () => {
            // Arrange
            const message = "@workspace Analyze this file";
            mockContext.activeFile = {
                path: "/workspace/test-project/src/main.ts",
                language: "typescript",
            };

            // Act
            const result = await participant.resolveReferenceContext(message, mockContext);

            // Assert
            expect(result).toContain("Active file: src/main.ts");
        });

        it("should clean @workspace mention from message", async () => {
            // Arrange
            const message = "@workspace   What files?  ";

            // Act
            const result = await participant.resolveReferenceContext(message, mockContext);

            // Assert
            expect(result).toContain("What files?");
            expect(result).not.toContain("@workspace");
        });

        it("should handle case-insensitive @workspace mention", async () => {
            // Arrange
            const message = "@WORKSPACE What files?";

            // Act
            const result = await participant.resolveReferenceContext(message, mockContext);

            // Assert
            expect(result).toContain("## Workspace Context");
        });
    });

    describe("participant metadata", () => {
        it("should have correct participant metadata", () => {
            expect(participant.id).toBe("workspace");
            expect(participant.displayName).toBe("Workspace");
            expect(participant.description).toBe("Provides workspace-wide context and file information");
            expect(participant.icon).toBe("folder");
        });
    });
});