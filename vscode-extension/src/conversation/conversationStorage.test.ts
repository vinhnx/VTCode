import * as vscode from 'vscode';
import { ConversationStorage, ConversationData } from './conversationStorage';

// Mock VS Code API
jest.mock('vscode', () => ({
    workspace: {
        fs: {
            createDirectory: jest.fn(),
            writeFile: jest.fn(),
            readFile: jest.fn(),
            delete: jest.fn(),
            readDirectory: jest.fn(),
        },
        asRelativePath: jest.fn((uri: vscode.Uri) => uri.path),
    },
    Uri: {
        joinPath: jest.fn((...args: any[]) => ({ fsPath: args.map(a => a.fsPath || a).join('/') })),
        file: jest.fn((path: string) => ({ fsPath: path })),
    },
    FileType: {
        File: 1,
        Directory: 2,
    },
    EventEmitter: jest.fn().mockImplementation(() => ({
        event: jest.fn(),
        fire: jest.fn(),
        dispose: jest.fn(),
    })),
}));

describe('ConversationStorage', () => {
    let storage: ConversationStorage;
    let mockContext: any;

    beforeEach(() => {
        mockContext = {
            globalStorageUri: { fsPath: '/mock/storage' },
        };
        
        storage = new ConversationStorage(mockContext);
        
        // Reset all mocks
        jest.clearAllMocks();
        
        // Setup default mock implementations
        (vscode.workspace.fs.createDirectory as jest.Mock).mockResolvedValue(undefined);
        (vscode.workspace.fs.writeFile as jest.Mock).mockResolvedValue(undefined);
        (vscode.workspace.fs.readFile as jest.Mock).mockResolvedValue(Buffer.from('{}'));
        (vscode.workspace.fs.delete as jest.Mock).mockResolvedValue(undefined);
        (vscode.workspace.fs.readDirectory as jest.Mock).mockResolvedValue([]);
    });

    describe('saveConversation', () => {
        it('should save a new conversation', async () => {
            const messages = [
                { role: 'user', content: 'Hello', timestamp: Date.now() },
                { role: 'assistant', content: 'Hi there!', timestamp: Date.now() },
            ];

            await storage.saveConversation('test-123', 'Test Conversation', messages);

            const saved = await storage.loadConversation('test-123');
            expect(saved).toBeDefined();
            expect(saved?.metadata.title).toBe('Test Conversation');
            expect(saved?.messages).toHaveLength(2);
        });

        it('should generate title from first user message if not provided', async () => {
            const messages = [
                { role: 'user', content: 'Explain how to implement a binary search tree', timestamp: Date.now() },
                { role: 'assistant', content: 'Sure! Here\'s how...', timestamp: Date.now() },
            ];

            await storage.saveConversation('test-456', '', messages);

            const saved = await storage.loadConversation('test-456');
            expect(saved?.metadata.title).toBe('Explain how to implement a binary search tree');
        });

        it('should truncate long titles', async () => {
            const longMessage = 'This is a very long message that should be truncated because it exceeds the maximum length for a title';
            const messages = [
                { role: 'user', content: longMessage, timestamp: Date.now() },
            ];

            await storage.saveConversation('test-789', '', messages);

            const saved = await storage.loadConversation('test-789');
            expect(saved?.metadata.title).toContain('...');
            expect(saved?.metadata.title.length).toBeLessThanOrEqual(50);
        });

        it('should update existing conversation', async () => {
            const messages1 = [{ role: 'user', content: 'First message', timestamp: Date.now() }];
            const messages2 = [
                { role: 'user', content: 'First message', timestamp: Date.now() },
                { role: 'assistant', content: 'Response', timestamp: Date.now() },
            ];

            await storage.saveConversation('test-update', 'Original', messages1);
            const firstSaved = await storage.loadConversation('test-update');
            const firstUpdatedAt = firstSaved?.metadata.updatedAt;

            // Wait a bit to ensure different timestamp
            await new Promise(resolve => setTimeout(resolve, 10));

            await storage.saveConversation('test-update', 'Updated', messages2);
            const secondSaved = await storage.loadConversation('test-update');

            expect(secondSaved?.metadata.title).toBe('Updated');
            expect(secondSaved?.metadata.messageCount).toBe(2);
            expect(secondSaved?.metadata.updatedAt).toBeGreaterThan(firstUpdatedAt || 0);
        });
    });

    describe('loadConversation', () => {
        it('should return undefined for non-existent conversation', async () => {
            (vscode.workspace.fs.readFile as jest.Mock).mockRejectedValue(new Error('File not found'));
            
            const result = await storage.loadConversation('non-existent');
            expect(result).toBeUndefined();
        });

        it('should load conversation from disk', async () => {
            const mockData: ConversationData = {
                metadata: {
                    id: 'test-load',
                    title: 'Loaded Conversation',
                    createdAt: Date.now(),
                    updatedAt: Date.now(),
                    messageCount: 3,
                    participants: ['user', 'assistant'],
                    tags: ['test'],
                },
                messages: [
                    { role: 'user', content: 'Hello', timestamp: Date.now() },
                    { role: 'assistant', content: 'Hi', timestamp: Date.now() },
                    { role: 'user', content: 'How are you?', timestamp: Date.now() },
                ],
            };

            (vscode.workspace.fs.readFile as jest.Mock).mockResolvedValue(
                Buffer.from(JSON.stringify(mockData))
            );

            const result = await storage.loadConversation('test-load');
            expect(result).toEqual(mockData);
        });
    });

    describe('deleteConversation', () => {
        it('should delete existing conversation', async () => {
            const messages = [{ role: 'user', content: 'Test', timestamp: Date.now() }];
            await storage.saveConversation('test-delete', 'To Delete', messages);

            const result = await storage.deleteConversation('test-delete');
            expect(result).toBe(true);

            const afterDelete = await storage.loadConversation('test-delete');
            expect(afterDelete).toBeUndefined();
        });

        it('should return false for non-existent conversation', async () => {
            const result = await storage.deleteConversation('non-existent');
            expect(result).toBe(false);
        });
    });

    describe('listConversations', () => {
        it('should return empty array when no conversations', async () => {
            const list = await storage.listConversations();
            expect(list).toEqual([]);
        });

        it('should list all conversations sorted by updatedAt', async () => {
            const messages = [{ role: 'user', content: 'Test', timestamp: Date.now() }];

            await storage.saveConversation('conv-1', 'First', messages);
            await new Promise(resolve => setTimeout(resolve, 10));
            await storage.saveConversation('conv-2', 'Second', messages);
            await new Promise(resolve => setTimeout(resolve, 10));
            await storage.saveConversation('conv-3', 'Third', messages);

            const list = await storage.listConversations();
            expect(list).toHaveLength(3);
            expect(list[0].title).toBe('Third'); // Most recent
            expect(list[1].title).toBe('Second');
            expect(list[2].title).toBe('First'); // Oldest
        });
    });

    describe('searchConversations', () => {
        beforeEach(async () => {
            const messages = [{ role: 'user', content: 'Test', timestamp: Date.now() }];
            await storage.saveConversation('search-1', 'JavaScript Tutorial', messages, [], ['js', 'tutorial']);
            await storage.saveConversation('search-2', 'Python Guide', messages, [], ['python', 'guide']);
            await storage.saveConversation('search-3', 'JavaScript Advanced', messages, [], ['js', 'advanced']);
        });

        it('should search by title', async () => {
            const results = await storage.searchConversations('python');
            expect(results).toHaveLength(1);
            expect(results[0].title).toBe('Python Guide');
        });

        it('should search by tag', async () => {
            const results = await storage.searchConversations('js');
            expect(results).toHaveLength(2);
            expect(results.map(r => r.title)).toContain('JavaScript Tutorial');
            expect(results.map(r => r.title)).toContain('JavaScript Advanced');
        });

        it('should be case insensitive', async () => {
            const results = await storage.searchConversations('JAVASCRIPT');
            expect(results).toHaveLength(2);
        });

        it('should return empty array for no matches', async () => {
            const results = await storage.searchConversations('nonexistent');
            expect(results).toHaveLength(0);
        });
    });

    describe('getRecentConversations', () => {
        it('should return most recent conversations', async () => {
            const messages = [{ role: 'user', content: 'Test', timestamp: Date.now() }];

            for (let i = 1; i <= 5; i++) {
                await storage.saveConversation(`recent-${i}`, `Conversation ${i}`, messages);
                await new Promise(resolve => setTimeout(resolve, 10));
            }

            const recent = await storage.getRecentConversations(3);
            expect(recent).toHaveLength(3);
            expect(recent[0].title).toBe('Conversation 5');
            expect(recent[1].title).toBe('Conversation 4');
            expect(recent[2].title).toBe('Conversation 3');
        });
    });

    describe('export and import', () => {
        it('should export conversation to file', async () => {
            const messages = [
                { role: 'user', content: 'Export test', timestamp: Date.now() },
            ];
            await storage.saveConversation('export-test', 'Export Me', messages);

            const exportUri = { fsPath: '/mock/export.json' } as vscode.Uri;
            await storage.exportConversation('export-test', exportUri);

            expect(vscode.workspace.fs.writeFile).toHaveBeenCalledWith(
                exportUri,
                expect.any(Buffer)
            );

            const writtenContent = (vscode.workspace.fs.writeFile as jest.Mock).mock.calls[0][1];
            const exportedData = JSON.parse(writtenContent.toString());
            expect(exportedData.metadata.title).toBe('Export Me');
            expect(exportedData.messages).toHaveLength(1);
            expect(exportedData.exportDate).toBeDefined();
            expect(exportedData.version).toBe('1.0');
        });

        it('should import conversation from file', async () => {
            const importData = {
                metadata: {
                    id: 'imported-123',
                    title: 'Imported Conversation',
                    createdAt: Date.now(),
                    updatedAt: Date.now(),
                    messageCount: 2,
                    participants: ['user'],
                    tags: ['imported'],
                },
                messages: [
                    { role: 'user', content: 'Imported message', timestamp: Date.now() },
                ],
            };

            const importUri = { fsPath: '/mock/import.json' } as vscode.Uri;
            (vscode.workspace.fs.readFile as jest.Mock).mockResolvedValue(
                Buffer.from(JSON.stringify(importData))
            );

            const id = await storage.importConversation(importUri);
            expect(id).toBe('imported-123');

            const imported = await storage.loadConversation('imported-123');
            expect(imported?.metadata.title).toBe('Imported Conversation');
        });

        it('should throw error for invalid import data', async () => {
            const importUri = { fsPath: '/mock/invalid.json' } as vscode.Uri;
            (vscode.workspace.fs.readFile as jest.Mock).mockResolvedValue(
                Buffer.from(JSON.stringify({ invalid: 'data' }))
            );

            await expect(storage.importConversation(importUri)).rejects.toThrow(
                'Invalid conversation file: missing metadata.id'
            );
        });
    });

    describe('clearAll', () => {
        it('should clear all conversations', async () => {
            const messages = [{ role: 'user', content: 'Test', timestamp: Date.now() }];
            await storage.saveConversation('clear-1', 'First', messages);
            await storage.saveConversation('clear-2', 'Second', messages);

            let list = await storage.listConversations();
            expect(list).toHaveLength(2);

            await storage.clearAll();

            list = await storage.listConversations();
            expect(list).toHaveLength(0);
        });
    });
});