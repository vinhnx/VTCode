import * as vscode from "vscode";
import { ChatMessage, Conversation, ConversationMetadata } from "../types/message";
import { ConversationStorage } from "./conversationStorage";

/**
 * Manages conversation state and persistence
 */
export class ConversationManager {
    private currentConversation: Conversation | undefined;
    private messageHistory: ChatMessage[] = [];
    private readonly maxHistorySize = 1000; // Maximum messages to keep in memory
    private storage: ConversationStorage;

    constructor(context: vscode.ExtensionContext, private readonly output: vscode.OutputChannel) {
        this.output.appendLine('[ConversationManager] Initialized');
        this.storage = new ConversationStorage(context);
        
        // Initialize storage
        this.storage.initialize().catch(error => {
            this.output.appendLine(`[ConversationManager] Failed to initialize storage: ${error}`);
        });
    }

    /**
     * Create a new conversation
     */
    public async createConversation(title?: string): Promise<Conversation> {
        const now = Date.now();
        const conversation: Conversation = {
            id: this.generateConversationId(),
            messages: [],
            metadata: {
                title: title || "New Conversation",
                createdAt: now,
                updatedAt: now,
                totalTokens: { input: 0, output: 0 },
                participants: [],
            },
        };
        
        this.currentConversation = conversation;
        this.messageHistory = [];
        
        // Save to persistent storage
        await this.storage.saveConversation(
            conversation.id,
            conversation.metadata.title,
            conversation.messages,
            conversation.metadata.participants
        );
        
        this.output.appendLine(`[ConversationManager] Created conversation: ${conversation.id}`);
        return conversation;
    }

    /**
     * Get the current conversation
     */
    public getCurrentConversation(): Conversation | undefined {
        return this.currentConversation;
    }

    /**
     * Add a message to the current conversation
     */
    public async addMessage(message: ChatMessage): Promise<void> {
        if (!this.currentConversation) {
            await this.createConversation();
        }

        // Add to conversation
        this.currentConversation!.messages.push(message);
        this.currentConversation!.metadata.updatedAt = Date.now();

        // Add to history
        this.messageHistory.push(message);
        
        // Trim history if it gets too large
        if (this.messageHistory.length > this.maxHistorySize) {
            this.messageHistory = this.messageHistory.slice(-this.maxHistorySize);
        }

        // Update metadata
        this.updateConversationMetadata(message);

        // Save to persistent storage
        await this.storage.saveConversation(
            this.currentConversation!.id,
            this.currentConversation!.metadata.title,
            this.currentConversation!.messages,
            this.currentConversation!.metadata.participants
        );
    }

    /**
     * Update conversation metadata based on message
     */
    private updateConversationMetadata(message: ChatMessage): void {
        if (!this.currentConversation) return;

        const metadata = this.currentConversation.metadata;

        // Update token counts
        if (message.metadata?.tokens) {
            metadata.totalTokens = {
                input: (metadata.totalTokens?.input || 0) + message.metadata.tokens.input,
                output: (metadata.totalTokens?.output || 0) + message.metadata.tokens.output,
            };
        }

        // Update participants
        if (message.metadata?.participantId) {
            if (!metadata.participants.includes(message.metadata.participantId)) {
                metadata.participants.push(message.metadata.participantId);
            }
        }

        // Update title based on first user message
        if (message.role === 'user' && metadata.title === 'New Conversation') {
            const preview = message.content.slice(0, 50);
            metadata.title = preview.length < message.content.length 
                ? `${preview}...` 
                : preview;
        }
    }

    /**
     * Get recent messages for context building
     */
    public getRecentMessages(count: number = 12): ChatMessage[] {
        return this.messageHistory.slice(-count);
    }

    /**
     * Get messages by role
     */
    public getMessagesByRole(role: ChatMessage['role']): ChatMessage[] {
        return this.messageHistory.filter(msg => msg.role === role);
    }

    /**
     * Clear the current conversation
     */
    public async clearConversation(): Promise<void> {
        if (this.currentConversation) {
            // Save conversation to storage before clearing
            await this.storage.saveConversation(
                this.currentConversation.id,
                this.currentConversation.metadata.title,
                this.currentConversation.messages,
                this.currentConversation.metadata.participants
            );
        }
        
        this.currentConversation = undefined;
        this.messageHistory = [];
        await this.createConversation();
    }

    /**
     * Load conversation from storage
     */
    public async loadConversation(id: string): Promise<Conversation | undefined> {
        const data = await this.storage.loadConversation(id);
        if (!data) {
            this.output.appendLine(`[ConversationManager] Conversation not found: ${id}`);
            return undefined;
        }

        const conversation: Conversation = {
            id: data.metadata.id,
            messages: data.messages,
            metadata: {
                title: data.metadata.title,
                createdAt: data.metadata.createdAt,
                updatedAt: data.metadata.updatedAt,
                totalTokens: { input: 0, output: 0 }, // Calculate from messages if needed
                participants: data.metadata.participants,
            },
        };

        this.currentConversation = conversation;
        this.messageHistory = [...data.messages];
        
        this.output.appendLine(`[ConversationManager] Loaded conversation: ${id}`);
        return conversation;
    }

    /**
     * List all conversations from storage
     */
    public async listConversations(): Promise<any[]> {
        return await this.storage.listConversations();
    }

    /**
     * Delete a conversation
     */
    public async deleteConversation(id: string): Promise<boolean> {
        const deleted = await this.storage.deleteConversation(id);
        if (deleted && this.currentConversation?.id === id) {
            this.currentConversation = undefined;
            await this.createConversation();
        }
        return deleted;
    }

    /**
     * Get conversation statistics
     */
    public getStatistics() {
        return {
            totalMessages: this.messageHistory.length,
            userMessages: this.getMessagesByRole('user').length,
            assistantMessages: this.getMessagesByRole('assistant').length,
            toolMessages: this.getMessagesByRole('tool').length,
            errorMessages: this.getMessagesByRole('error').length,
            totalTokens: this.currentConversation?.metadata.totalTokens,
        };
    }

    /**
     * Generate unique conversation ID
     */
    private generateConversationId(): string {
        return `conv_${Date.now()}_${Math.random().toString(36).slice(2, 11)}`;
    }

    /**
     * Generate unique message ID
     */
    private generateMessageId(): string {
        return `msg_${Date.now()}_${Math.random().toString(36).slice(2, 11)}`;
    }

    /**
     * Create a new message with ID and timestamp
     */
    public createMessage(
        role: ChatMessage['role'],
        content: string,
        metadata?: ChatMessage['metadata']
    ): ChatMessage {
        return {
            id: this.generateMessageId(),
            role,
            content,
            timestamp: Date.now(),
            metadata,
            state: 'complete',
        };
    }

    /**
     * Create a pending message (for streaming responses)
     */
    public createPendingMessage(
        role: ChatMessage['role'],
        content: string = ''
    ): ChatMessage {
        return {
            id: this.generateMessageId(),
            role,
            content,
            timestamp: Date.now(),
            state: 'pending',
        };
    }

    /**
     * Update a pending message with final content
     */
    public updatePendingMessage(
        messageId: string,
        updates: Partial<Omit<ChatMessage, 'id' | 'timestamp'>>
    ): void {
        const messageIndex = this.messageHistory.findIndex(msg => msg.id === messageId);
        if (messageIndex >= 0) {
            const message = this.messageHistory[messageIndex];
            this.messageHistory[messageIndex] = {
                ...message,
                ...updates,
                state: updates.state || 'complete',
            };
        }

        // Also update in conversation if it exists
        if (this.currentConversation) {
            const convMessageIndex = this.currentConversation.messages.findIndex(
                msg => msg.id === messageId
            );
            if (convMessageIndex >= 0) {
                const message = this.currentConversation.messages[convMessageIndex];
                this.currentConversation.messages[convMessageIndex] = {
                    ...message,
                    ...updates,
                    state: updates.state || 'complete',
                };
            }
        }
    }

    /**
     * Export conversation as JSON
     */
    public exportConversation(): string {
        if (!this.currentConversation) {
            return JSON.stringify({ error: 'No active conversation' }, null, 2);
        }

        return JSON.stringify(this.currentConversation, null, 2);
    }

    public dispose(): void {
        this.storage.dispose();
        this.output.appendLine('[ConversationManager] Disposed');
    }
}