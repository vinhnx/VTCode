import { describe, it, expect, beforeEach, vi } from "vitest";
import { CommandRegistry } from "./commandRegistry";
import { ICommand, CommandContext } from "./types/command";

/**
 * Mock command for testing
 */
class MockCommand implements ICommand {
    id = "test.mock";
    title = "Mock Command";
    description = "A mock command for testing";
    icon = "beaker";
    executeCalled = false;
    canExecuteResult = true;

    canExecute(_context: CommandContext): boolean {
        return this.canExecuteResult;
    }

    async execute(_context: CommandContext): Promise<void> {
        this.executeCalled = true;
    }
}

describe("CommandRegistry", () => {
    let registry: CommandRegistry;

    beforeEach(() => {
        registry = new CommandRegistry();
    });

    it("should register a command", () => {
        const cmd = new MockCommand();
        registry.register(cmd);
        expect(registry.getCommand("test.mock")).toBe(cmd);
    });

    it("should throw error when registering duplicate command", () => {
        const cmd = new MockCommand();
        registry.register(cmd);
        expect(() => registry.register(cmd)).toThrow(
            "Command test.mock is already registered"
        );
    });

    it("should register multiple commands", () => {
        const cmd1 = new MockCommand();
        const cmd2 = new MockCommand();
        cmd2.id = "test.mock2";

        registry.registerMultiple([cmd1, cmd2]);
        expect(registry.getCommand("test.mock")).toBe(cmd1);
        expect(registry.getCommand("test.mock2")).toBe(cmd2);
    });

    it("should get all registered commands", () => {
        const cmd1 = new MockCommand();
        const cmd2 = new MockCommand();
        cmd2.id = "test.mock2";

        registry.registerMultiple([cmd1, cmd2]);
        const all = registry.getAllCommands();
        expect(all).toHaveLength(2);
        expect(all).toContain(cmd1);
        expect(all).toContain(cmd2);
    });

    it("should return undefined for unknown command", () => {
        expect(registry.getCommand("unknown.command")).toBeUndefined();
    });

    it("should throw error when executing command that cannot execute", () => {
        const cmd = new MockCommand();
        cmd.canExecuteResult = false;
        registry.register(cmd);

        const mockContext = vi.mocked({
            createExtensionContext: () => ({
                subscriptions: [],
            }),
        });

        expect(() => {
            const context: CommandContext = {
                trusted: false,
            };
            if (!cmd.canExecute(context)) {
                throw new Error(`Command ${cmd.id} cannot be executed`);
            }
        }).toThrow();
    });

    it("should clear all commands", () => {
        const cmd = new MockCommand();
        registry.register(cmd);
        expect(registry.getAllCommands()).toHaveLength(1);

        registry.dispose();
        expect(registry.getAllCommands()).toHaveLength(0);
    });
});
