import { describe, it, expect, beforeEach, vi } from "vitest";
import * as vscode from "vscode";
import { AskSelectionCommand } from "./askSelectionCommand";

describe("AskSelectionCommand", () => {
	let command: AskSelectionCommand;
	let mockExecuteCommand: any;
	let mockContext: any;

	beforeEach(() => {
		mockExecuteCommand = vi.fn();
		command = new AskSelectionCommand(mockExecuteCommand);
		mockContext = {
			activeTextEditor: undefined,
		};
	});

	it("should have correct id and title", () => {
		expect(command.id).toBe("vtcode.askSelection");
		expect(command.title).toBe("Ask About Selection");
	});

	it("should return false when no editor is active", async () => {
		mockContext.activeTextEditor = undefined;
		expect(command.canExecute(mockContext)).toBe(false);
	});

	it("should return true when editor is active", async () => {
		mockContext.activeTextEditor = {
			selection: { isEmpty: false },
		};
		expect(command.canExecute(mockContext)).toBe(true);
	});

	it("should show warning when no editor is open", async () => {
		const showWarning = vi.spyOn(vscode.window, "showWarningMessage");
		vi.spyOn(vscode.window, "activeTextEditor", "get").mockReturnValue(
			undefined
		);
		await command.execute(mockContext);
		expect(showWarning).toHaveBeenCalledWith(
			expect.stringContaining("Open a text editor")
		);
	});

	it("should show warning when selection is empty", async () => {
		const showWarning = vi.spyOn(vscode.window, "showWarningMessage");
		const mockEditor = {
			selection: { isEmpty: true },
		} as any;
		vi.spyOn(vscode.window, "activeTextEditor", "get").mockReturnValue(
			mockEditor
		);
		await command.execute(mockContext);
		expect(showWarning).toHaveBeenCalledWith(
			expect.stringContaining("Highlight text")
		);
	});
});
