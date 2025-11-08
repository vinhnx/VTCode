import { describe, it, expect, beforeEach } from "vitest";
import { ParticipantRegistry } from "./participantRegistry";
import { ChatParticipant, ParticipantContext } from "./types/participant";

/**
 * Mock participant for testing
 */
class MockParticipant implements ChatParticipant {
    id = "test.mock";
    displayName = "@test";
    description = "A mock participant for testing";
    icon = "beaker";
    canHandleResult = true;
    resolveContextResult = "Mock context";

    canHandle(_context: ParticipantContext): boolean {
        return this.canHandleResult;
    }

    async resolveReferenceContext(
        _message: string,
        _context: ParticipantContext
    ): Promise<string> {
        return this.resolveContextResult;
    }
}

describe("ParticipantRegistry", () => {
    let registry: ParticipantRegistry;

    beforeEach(() => {
        registry = new ParticipantRegistry();
    });

    it("should register a participant", () => {
        const participant = new MockParticipant();
        registry.register(participant);
        expect(registry.getParticipant("test.mock")).toBe(participant);
    });

    it("should throw error when registering duplicate participant", () => {
        const participant = new MockParticipant();
        registry.register(participant);
        expect(() => registry.register(participant)).toThrow(
            "Participant test.mock is already registered"
        );
    });

    it("should register multiple participants", () => {
        const p1 = new MockParticipant();
        const p2 = new MockParticipant();
        p2.id = "test.mock2";

        registry.registerMultiple([p1, p2]);
        expect(registry.getParticipant("test.mock")).toBe(p1);
        expect(registry.getParticipant("test.mock2")).toBe(p2);
    });

    it("should get all registered participants", () => {
        const p1 = new MockParticipant();
        const p2 = new MockParticipant();
        p2.id = "test.mock2";

        registry.registerMultiple([p1, p2]);
        const all = registry.getAllParticipants();
        expect(all).toHaveLength(2);
    });

    it("should get applicable participants for context", () => {
        const p1 = new MockParticipant();
        const p2 = new MockParticipant();
        p2.id = "test.mock2";
        p2.canHandleResult = false;

        registry.registerMultiple([p1, p2]);
        const applicable = registry.getApplicableParticipants({});
        expect(applicable).toHaveLength(1);
        expect(applicable[0]).toBe(p1);
    });

    it("should parse @-mentions from message", () => {
        const mentions = registry.parseMentions("Ask @workspace about @code");
        expect(mentions).toEqual(["workspace", "code"]);
    });

    it("should parse mentions with no matches", () => {
        const mentions = registry.parseMentions("Ask about code");
        expect(mentions).toHaveLength(0);
    });

    it("should resolve specific participant", async () => {
        const p1 = new MockParticipant();
        registry.register(p1);

        const result = await registry.resolveParticipant(
            "test.mock",
            "Hello",
            {}
        );
        expect(result).toBe("Mock context");
    });

    it("should throw error resolving unknown participant", async () => {
        await expect(
            registry.resolveParticipant("unknown", "Hello", {})
        ).rejects.toThrow("Participant unknown not found");
    });

    it("should resolve all applicable participants", async () => {
        const p1 = new MockParticipant();
        p1.resolveContextResult = "Context 1";
        const p2 = new MockParticipant();
        p2.id = "test.mock2";
        p2.resolveContextResult = "Context 2";

        registry.registerMultiple([p1, p2]);
        const enriched = await registry.resolveAllApplicable("Hello", {});

        expect(enriched).toContain("Hello");
        expect(enriched).toContain("Context 1");
        expect(enriched).toContain("Context 2");
    });

    it("should clear all participants", () => {
        const p1 = new MockParticipant();
        registry.register(p1);
        expect(registry.getAllParticipants()).toHaveLength(1);

        registry.clear();
        expect(registry.getAllParticipants()).toHaveLength(0);
    });
});
