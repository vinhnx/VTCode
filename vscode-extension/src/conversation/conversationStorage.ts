import * as vscode from 'vscode';
import { ConversationData } from './conversationManager';
import * as path from 'path';

export interface ConversationMetadata {
    readonly id: string;
    readonly title: string;
    readonly createdAt: number;
    readonly updatedAt: number;
    readonly messageCount: number;
    readonly participants: string[];
    readonly tags: string[];
}

export interface ConversationListItem {
    readonly id: string;
    readonly title: string;
    readonly createdAt: number;
    readonly updatedAt: number;
    readonly messageCount: number;
    readonly participants: string[];
    readonly tags: string[];
}

export interface ConversationStorageConfig {
    readonly maxConversations: number;
    readonly autoSave: boolean;
    readonly storagePath: string;
}

/**
 * Manages persistent storage of conversations
 * Handles saving, loading, searching, and exporting conversations
 */
export class ConversationStorage implements vscode.Disposable {
    private readonly storageUri: vscode.Uri;
    private readonly conversationsDir: vscode.Uri;
    private readonly config: ConversationStorageConfig;
    private readonly onDidChangeConversationsEmitter = new vscode.EventEmitter<void>();
    
    public readonly onDidChangeConversations = this.onDidChangeConversationsEmitter.event;

    constructor(
        private readonly context: vscode.ExtensionContext,
        config?: Partial<ConversationStorageConfig>
    ) {
        this.storageUri = context.globalStorageUri;
        this.conversationsDir = vscode.Uri.joinPath(this.storageUri, 'conversations');
        this.config = {
            maxConversations: config?.maxConversations ?? 100,
            autoSave: config?.autoSave ?? true,
            storagePath: config?.storagePath ?? 'conversations',
        };
    }

    /**
     * Initialize the storage directory
     */
    public async initialize(): Promise<void> {
        try {
            await vscode.workspace.fs.createDirectory(this.conversationsDir);
        } catch (error) {
            console.error('Failed to create conversations directory:', error);
            throw error;
        }
    }

    /**
     * Save a conversation to persistent storage
     */
    public async saveConversation(
        id: string,
        title: string,
        messages: any[],
        participants: string[] = [],
        tags: string[] = []
    ): Promise<void> {
        const now = Date.now();
        const existing = await this.loadConversation(id);
        
        const metadata: ConversationMetadata = {
            id,
            title: title || this.generateTitleFromMessages(messages),
            createdAt: existing?.metadata.createdAt || now,
            updatedAt: now,
            messageCount: messages.length,
            participants: [...new Set([...participants, ...this.extractParticipants(messages)])],
            tags: [...new Set(tags)],
        };

        const conversationData: ConversationData = {
            metadata,
            messages,
        };

        const fileUri = this.getConversationFileUri(id);
        
        try {
            const data = JSON.stringify(conversationData, null, 2);
            await vscode.workspace.fs.writeFile(fileUri, Buffer.from(data, 'utf8'));
            this.onDidChangeConversationsEmitter.fire();
        } catch (error) {
            console.error(`Failed to save conversation ${id}:`, error);
            throw new Error(`Failed to save conversation: ${error}`);
        }
    }

    /**
     * Load a conversation from persistent storage
     */
    public async loadConversation(id: string): Promise<ConversationData | undefined> {
        const fileUri = this.getConversationFileUri(id);
        
        try {
            const data = await vscode.workspace.fs.readFile(fileUri);
            const conversationData = JSON.parse(data.toString()) as ConversationData;
            
            // Validate the loaded data
            if (!this.isValidConversationData(conversationData)) {
                throw new Error('Invalid conversation data format');
            }
            
            return conversationData;
        } catch (error) {
            if ((error as any).code === 'FileNotFound') {
                return undefined;
            }
            console.error(`Failed to load conversation ${id}:`, error);
            return undefined;
        }
    }

    /**
     * Delete a conversation from storage
     */
    public async deleteConversation(id: string): Promise<boolean> {
        const fileUri = this.getConversationFileUri(id);
        
        try {
            await vscode.workspace.fs.delete(fileUri);
            this.onDidChangeConversationsEmitter.fire();
            return true;
        } catch (error) {
            if ((error as any).code === 'FileNotFound') {
                return false;
            }
            console.error(`Failed to delete conversation ${id}:`, error);
            return false;
        }
    }

    /**
     * List all conversations with metadata
     */
    public async listConversations(): Promise<ConversationListItem[]> {
        try {
            const entries = await vscode.workspace.fs.readDirectory(this.conversationsDir);
            const conversations: ConversationListItem[] = [];

            for (const [name, type] of entries) {
                if (type === vscode.FileType.File && name.endsWith('.json')) {
                    const id = name.replace('.json', '');
                    const conversation = await this.loadConversation(id);
                    
                    if (conversation) {
                        conversations.push({
                            id: conversation.metadata.id,
                            title: conversation.metadata.title,
                            createdAt: conversation.metadata.createdAt,
                            updatedAt: conversation.metadata.updatedAt,
                            messageCount: conversation.metadata.messageCount,
                            participants: conversation.metadata.participants,
                            tags: conversation.metadata.tags,
                        });
                    }
                }
            }

            // Sort by updatedAt descending (most recent first)
            return conversations.sort((a, b) => b.updatedAt - a.updatedAt);
        } catch (error) {
            console.error('Failed to list conversations:', error);
            return [];
        }
    }

    /**
     * Search conversations by title or tags
     */
    public async searchConversations(query: string): Promise<ConversationListItem[]> {
        const conversations = await this.listConversations();
        const lowerQuery = query.toLowerCase();

        return conversations.filter(conv => 
            conv.title.toLowerCase().includes(lowerQuery) ||
            conv.tags.some(tag => tag.toLowerCase().includes(lowerQuery))
        );
    }

    /**
     * Get recent conversations (most recently updated)
     */
    public async getRecentConversations(limit: number = 10): Promise<ConversationListItem[]> {
        const conversations = await this.listConversations();
        return conversations.slice(0, limit);
    }

    /**
     * Export a conversation to a file
     */
    public async exportConversation(id: string, exportUri: vscode.Uri): Promise<void> {
        const conversation = await this.loadConversation(id);
        
        if (!conversation) {
            throw new Error(`Conversation ${id} not found`);
        }

        const exportData = {
            ...conversation,
            exportDate: Date.now(),
            version: '1.0',
        };

        try {
            const data = JSON.stringify(exportData, null, 2);
            await vscode.workspace.fs.writeFile(exportUri, Buffer.from(data, 'utf8'));
        } catch (error) {
            console.error(`Failed to export conversation ${id}:`, error);
            throw new Error(`Failed to export conversation: ${error}`);
        }
    }

    /**
     * Import a conversation from a file
     */
    public async importConversation(importUri: vscode.Uri): Promise<string> {
        try {
            const data = await vscode.workspace.fs.readFile(importUri);
            const importData = JSON.parse(data.toString());
            
            // Validate import data
            if (!this.isValidConversationData(importData)) {
                throw new Error('Invalid conversation file: missing metadata.id');
            }

            const id = importData.metadata.id;
            const fileUri = this.getConversationFileUri(id);

            // Save the imported conversation
            await vscode.workspace.fs.writeFile(fileUri, Buffer.from(JSON.stringify(importData, null, 2), 'utf8'));
            this.onDidChangeConversationsEmitter.fire();

            return id;
        } catch (error) {
            console.error('Failed to import conversation:', error);
            throw new Error(`Failed to import conversation: ${error}`);
        }
    }

    /**
     * Clear all conversations
     */
    public async clearAll(): Promise<void> {
        try {
            const entries = await vscode.workspace.fs.readDirectory(this.conversationsDir);
            
            for (const [name, type] of entries) {
                if (type === vscode.FileType.File && name.endsWith('.json')) {
                    const fileUri = vscode.Uri.joinPath(this.conversationsDir, name);
                    await vscode.workspace.fs.delete(fileUri);
                }
            }
            
            this.onDidChangeConversationsEmitter.fire();
        } catch (error) {
            console.error('Failed to clear conversations:', error);
            throw error;
        }
    }

    /**
     * Get the URI for a conversation file
     */
    private getConversationFileUri(id: string): vscode.Uri {
        const filename = `${id}.json`;
        return vscode.Uri.joinPath(this.conversationsDir, filename);
    }

    /**
     * Generate a title from the first user message
     */
    private generateTitleFromMessages(messages: any[]): string {
        const firstUserMessage = messages.find(msg => msg.role === 'user');
        if (!firstUserMessage || !firstUserMessage.content) {
            return 'Untitled Conversation';
        }

        // Take first 50 characters, remove newlines, add ellipsis if needed
        const content = firstUserMessage.content.replace(/\n/g, ' ').trim();
        return content.length > 50 ? content.substring(0, 47) + '...' : content;
    }

    /**
     * Extract participants from messages
     */
    private extractParticipants(messages: any[]): string[] {
        const participants = new Set<string>();
        
        for (const message of messages) {
            if (message.role && message.role !== 'system') {
                participants.add(message.role);
            }
            if (message.metadata?.participants) {
                message.metadata.participants.forEach((p: string) => participants.add(p));
            }
        }
        
        return Array.from(participants);
    }

    /**
     * Validate conversation data format
     */
    private isValidConversationData(data: any): data is ConversationData {
        return data && 
               data.metadata && 
               typeof data.metadata.id === 'string' &&
               Array.isArray(data.messages);
    }

    /**
     * Dispose resources
     */
    public dispose(): void {
        this.onDidChangeConversationsEmitter.dispose();
    }
}