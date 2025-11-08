import { describe, it, expect, beforeEach, vi } from "vitest";
import { AskCommand } from "./askCommand";
import { CommandContext } from "../types/command";
import * as vscode from "vscode";

describe("AskCommand", () => {
    let command: AskCommand;
    let mockBackend: any;

    beforeEach(() => {
        mockBackend = {
            ask: vi.fn(),
        };
        command = new AskCommand(mockBackend);
    });

    it("should have correct id, title, and icon", () => {
        expect(command.id).toBe("vtcode.ask");
        expect(command.title).toBe("Ask VTCode Agent");
        expect(command.icon).toBe("comment-discussion");
    });

    it("should be able to execute in any context", () => {
        const context: CommandContext = { trusted: false };
        expect(command.canExecute(context)).toBe(true);
    });

    it("should have description", () => {
        expect(command.description).toBeDefined();
        expect(command.description).toContain("VTCode agent");
    });

    it("should handle empty question gracefully", async () => {
        // Mock the input box to return empty string
        const showInputBox = vi
            .spyOn(vscode.window, "showInputBox")
            .mockResolvedValue("");

        const context: CommandContext = { trusted: true };
        await command.execute(context);

        expect(showInputBox).toHaveBeenCalled();
        // No error should be thrown
    });

    it("should handle cancelled input gracefully", async () => {
        // Mock the input box to return undefined (cancelled)
        const showInputBox = vi
            .spyOn(vscode.window, "showInputBox")
            .mockResolvedValue(undefined);

        const context: CommandContext = { trusted: true };
        await command.execute(context);

        expect(showInputBox).toHaveBeenCalled();
        // No error should be thrown
    });

    it("should show information message on success", async () => {
        const showInputBox = vi
            .spyOn(vscode.window, "showInputBox")
            .mockResolvedValue("What is this code?");
        const showMessage = vi
            .spyOn(vscode.window, "showInformationMessage")
            .mockResolvedValue(undefined);

        const context: CommandContext = { trusted: true };
        await command.execute(context);

        expect(showInputBox).toHaveBeenCalled();
        expect(showMessage).toHaveBeenCalledWith(expect.stringContaining("finished"));
    });

    it("should prompt user with correct message", async () => {
        const showInputBox = vi
            .spyOn(vscode.window, "showInputBox")
            .mockResolvedValue("Explain this function");

        const context: CommandContext = { trusted: true };
        await command.execute(context);

        expect(showInputBox).toHaveBeenCalledWith(
            expect.objectContaining({
                prompt: expect.stringContaining("help with"),
                ignoreFocusOut: true,
            })
        );
    });

    afterEach(() => {
        vi.clearAllMocks();
    });
});
