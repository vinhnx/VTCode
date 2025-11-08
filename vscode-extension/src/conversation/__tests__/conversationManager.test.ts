import * as vscode from "vscode";
import { ConversationManager } from "../conversationManager";
import { ChatMessage } from "../../types/message";

// Mock VS Code API
jest.mock("vscode");

describe("ConversationManager", () => {
    let manager: ConversationManager;
    let mockOutputChannel: any;

    beforeEach(() => {
        mockOutputChannel = {
            appendLine: jest.fn(),
        } as vscode.OutputChannel;

        manager = new ConversationManager(mockOutputChannel);
        jest.clearAllMocks();
    });

    describe("createConversation", () => {
        it("should create a new conversation with default title", () => {
            // Act
            const conversation = manager.createConversation();

            // Assert
            expect(conversation).toBeDefined();
            expect(conversation.id).toMatch(/^conv_\d+_[a-z0-9]+$/);
            expect(conversation.messages).toEqual([]);
            expect(conversation.metadata.title).toBe("New Conversation");
            expect(conversation.metadata.createdAt).toBeDefined();
            expect(conversation.metadata.updatedAt).toBeDefined();
            expect(conversation.metadata.totalTokens).toEqual({ input: 0, output: 0 });
            expect(conversation.metadata.participants).toEqual([]);
        });

        it("should create a new conversation with custom title", () => {
            // Act
            const conversation = manager.createConversation("Test Conversation");

            // Assert
            expect(conversation.metadata.title).toBe("Test Conversation");
        });
    });

    describe("addMessage", () => {
        it("should add a message to the current conversation", () => {
            // Arrange
            const message: ChatMessage = {
                id: "msg_123",
                role: "user",
                content: "Hello",
                timestamp: Date.now(),
                state: "complete",
            };

            // Act
            manager.addMessage(message);

            // Assert
            const conversation = manager.getCurrentConversation();
            expect(conversation?.messages).toContain(message);
            expect(conversation?.metadata.updatedAt).toBeGreaterThanOrEqual(
                conversation?.metadata.createdAt || 0
            );
        });

        it("should update token counts in metadata", () => {
            // Arrange
            const message: ChatMessage = {
                id: "msg_123",
                role: "assistant",
                content: "Hello back",
                timestamp: Date.now(),
                state: "complete",
                metadata: {
                    tokens: {
                        input: 10,
                        output: 20,
                    },
                },
            };

            // Act
            manager.addMessage(message);

            // Assert
            const conversation = manager.getCurrentConversation();
            expect(conversation?.metadata.totalTokens).toEqual({
                input: 10,
                output: 20,
            });
        });

        it("should update participants in metadata", () => {
            // Arrange
            const message: ChatMessage = {
                id: "msg_123",
                role: "assistant",
                content: "Hello",
                timestamp: Date.now(),
                state: "complete",
                metadata: {
                    participantId: "workspace",
                },
            };

            // Act
            manager.addMessage(message);

            // Assert
            const conversation = manager.getCurrentConversation();
            expect(conversation?.metadata.participants).toContain("workspace");
        });

        it("should update conversation title based on first user message", () => {
            // Arrange
            const message: ChatMessage = {
                id: "msg_123",
                role: "user",
                content: "What is the meaning of life and everything else?",
                timestamp: Date.now(),
                state: "complete",
            };

            // Act
            manager.addMessage(message);

            // Assert
            const conversation = manager.getCurrentConversation();
            expect(conversation?.metadata.title).toBe(
                "What is the meaning of life and everything else?"
            );
        });

        it("should truncate long messages in title", () => {
            // Arrange
            const longMessage = "a".repeat(100);
            const message: ChatMessage = {
                id: "msg_123",
                role: "user",
                content: longMessage,
                timestamp: Date.now(),
                state: "complete",
            };

            // Act
            manager.addMessage(message);

            // Assert
            const conversation = manager.getCurrentConversation();
            expect(conversation?.metadata.title).toBe("a".repeat(50) + "...");
        });
    });

    describe("getRecentMessages", () => {
        it("should return recent messages", () => {
            // Arrange
            const messages: ChatMessage[] = [
                { id: "msg_1", role: "user", content: "Message 1", timestamp: 1, state: "complete" },
                { id: "msg_2", role: "assistant", content: "Message 2", timestamp: 2, state: "complete" },
                { id: "msg_3", role: "user", content: "Message 3", timestamp: 3, state: "complete" },
            ];
            messages.forEach(msg => manager.addMessage(msg));

            // Act
            const recent = manager.getRecentMessages(2);

            // Assert
            expect(recent).toHaveLength(2);
            expect(recent[0].content).toBe("Message 2");
            expect(recent[1].content).toBe("Message 3");
        });

        it("should return all messages if count exceeds message count", () => {
            // Arrange
            const message: ChatMessage = {
                id: "msg_1",
                role: "user",
                content: "Message 1",
                timestamp: 1,
                state: "complete",
            };
            manager.addMessage(message);

            // Act
            const recent = manager.getRecentMessages(10);

            // Assert
            expect(recent).toHaveLength(1);
        });
    });

    describe("getMessagesByRole", () => {
        it("should return messages filtered by role", () => {
            // Arrange
            const messages: ChatMessage[] = [
                { id: "msg_1", role: "user", content: "User 1", timestamp: 1, state: "complete" },
                { id: "msg_2", role: "assistant", content: "Assistant 1", timestamp: 2, state: "complete" },
                { id: "msg_3", role: "user", content: "User 2", timestamp: 3, state: "complete" },
                { id: "msg_4", role: "tool", content: "Tool 1", timestamp: 4, state: "complete" },
            ];
            messages.forEach(msg => manager.addMessage(msg));

            // Act
            const userMessages = manager.getMessagesByRole("user");
            const assistantMessages = manager.getMessagesByRole("assistant");
            const toolMessages = manager.getMessagesByRole("tool");

            // Assert
            expect(userMessages).toHaveLength(2);
            expect(assistantMessages).toHaveLength(1);
            expect(toolMessages).toHaveLength(1);
        });
    });

    describe("clearConversation", () => {
        it("should clear current conversation and create new one", () => {
            // Arrange
            const message: ChatMessage = {
                id: "msg_1",
                role: "user",
                content: "Message 1",
                timestamp: 1,
                state: "complete",
            };
            manager.addMessage(message);
            const oldConversation = manager.getCurrentConversation();

            // Act
            manager.clearConversation();

            // Assert
            const newConversation = manager.getCurrentConversation();
            expect(newConversation).not.toBe(oldConversation);
            expect(newConversation?.messages).toEqual([]);
            expect(newConversation?.metadata.title).toBe("New Conversation");
        });
    });

    describe("createMessage", () => {
        it("should create a new message with ID and timestamp", () => {
            // Act
            const message = manager.createMessage("user", "Hello");

            // Assert
            expect(message.id).toMatch(/^msg_\d+_[a-z0-9]+$/);
            expect(message.role).toBe("user");
            expect(message.content).toBe("Hello");
            expect(message.timestamp).toBeDefined();
            expect(message.state).toBe("complete");
        });

        it("should include metadata if provided", () => {
            // Arrange
            const metadata = {
                model: "gpt-4",
                tokens: { input: 10, output: 20 },
            };

            // Act
            const message = manager.createMessage("assistant", "Hello", metadata);

            // Assert
            expect(message.metadata).toEqual(metadata);
        });
    });

    describe("createPendingMessage", () => {
        it("should create a pending message", () => {
            // Act
            const message = manager.createPendingMessage("assistant");

            // Assert
            expect(message.id).toBeDefined();
            expect(message.role).toBe("assistant");
            expect(message.content).toBe("");
            expect(message.state).toBe("pending");
        });
    });

    describe("updatePendingMessage", () => {
        it("should update pending message with final content", () => {
            // Arrange
            const pendingMessage = manager.createPendingMessage("assistant");
            manager.addMessage(pendingMessage);

            // Act
            manager.updatePendingMessage(pendingMessage.id, {
                content: "Final response",
                state: "complete",
                metadata: { model: "gpt-4" },
            });

            // Assert
            const conversation = manager.getCurrentConversation();
            const updatedMessage = conversation?.messages.find(msg => msg.id === pendingMessage.id);
            expect(updatedMessage?.content).toBe("Final response");
            expect(updatedMessage?.state).toBe("complete");
            expect(updatedMessage?.metadata?.model).toBe("gpt-4");
        });
    });

    describe("getStatistics", () => {
        it("should return conversation statistics", () => {
            // Arrange
            const messages: ChatMessage[] = [
                { id: "msg_1", role: "user", content: "User 1", timestamp: 1, state: "complete" },
                { id: "msg_2", role: "assistant", content: "Assistant 1", timestamp: 2, state: "complete" },
                { id: "msg_3", role: "user", content: "User 2", timestamp: 3, state: "complete" },
                { id: "msg_4", role: "tool", content: "Tool 1", timestamp: 4, state: "complete" },
                { id: "msg_5", role: "error", content: "Error 1", timestamp: 5, state: "complete" },
            ];
            messages.forEach(msg => manager.addMessage(msg));

            // Act
            const stats = manager.getStatistics();

            // Assert
            expect(stats.totalMessages).toBe(5);
            expect(stats.userMessages).toBe(2);
            expect(stats.assistantMessages).toBe(1);
            expect(stats.toolMessages).toBe(1);
            expect(stats.errorMessages).toBe(1);
            expect(stats.totalTokens).toEqual({ input: 0, output: 0 });
        });
    });

    describe("exportConversation", () => {
        it("should export conversation as JSON", () => {
            // Arrange
            const message: ChatMessage = {
                id: "msg_1",
                role: "user",
                content: "Hello",
                timestamp: 1,
                state: "complete",
            };
            manager.addMessage(message);

            // Act
            const exported = manager.exportConversation();

            // Assert
            const parsed = JSON.parse(exported);
            expect(parsed.id).toBeDefined();
            expect(parsed.messages).toHaveLength(1);
            expect(parsed.messages[0].content).toBe("Hello");
            expect(parsed.metadata).toBeDefined();
        });

        it("should return error when no active conversation", () => {
            // Arrange
            manager.clearConversation();
            manager["currentConversation"] = undefined;

            // Act
            const exported = manager.exportConversation();

            // Assert
            const parsed = JSON.parse(exported);
            expect(parsed.error).toBe("No active conversation");
        });
    });
});