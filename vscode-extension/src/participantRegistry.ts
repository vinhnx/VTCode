import * as vscode from "vscode";
import { ChatParticipant, ParticipantContext } from "./types/participant";

/**
 * Registry for managing VTCode chat participants
 */
export class ParticipantRegistry {
    private participants = new Map<string, ChatParticipant>();
    private disposables: vscode.Disposable[] = [];

    /**
     * Register a participant with the registry
     */
    public register(participant: ChatParticipant): void {
        this.participants.set(participant.id, participant);
    }

    /**
     * Register multiple participants at once
     */
    public registerAll(participants: ChatParticipant[]): void {
        for (const participant of participants) {
            this.register(participant);
        }
    }

    /**
     * Get a participant by ID
     */
    public get(id: string): ChatParticipant | undefined {
        return this.participants.get(id);
    }

    /**
     * Get all registered participants
     */
    public getAll(): ChatParticipant[] {
        return Array.from(this.participants.values());
    }

    /**
     * Unregister a participant
     */
    public unregister(id: string): void {
        this.participants.delete(id);
    }

    /**
     * Clear all registered participants
     */
    public clear(): void {
        this.participants.clear();
        this.disposables.forEach(d => d.dispose());
        this.disposables = [];
    }

    /**
     * Find participants that can handle the given context
     */
    public findEligibleParticipants(context: ParticipantContext): ChatParticipant[] {
        return this.getAll().filter(participant => participant.canHandle(context));
    }

    /**
     * Resolve context for a message by applying all eligible participants
     */
    public async resolveContext(
        message: string,
        context: ParticipantContext
    ): Promise<string> {
        let enhancedMessage = message;
        const eligibleParticipants = this.findEligibleParticipants(context);

        for (const participant of eligibleParticipants) {
            try {
                enhancedMessage = await participant.resolveReferenceContext(
                    enhancedMessage,
                    context
                );
            } catch (error) {
                const errorMessage = error instanceof Error ? error.message : String(error);
                void vscode.window.showWarningMessage(
                    `Participant "${participant.displayName}" failed to resolve context: ${errorMessage}`
                );
                // Continue with other participants even if one fails
            }
        }

        return enhancedMessage;
    }

    /**
     * Get participant suggestions for autocomplete
     */
    public getParticipantSuggestions(): Array<{
        label: string;
        description: string;
        insertText: string;
    }> {
        return this.getAll().map(participant => ({
            label: participant.id,
            description: participant.description || "",
            insertText: `${participant.id} `,
        }));
    }

    /**
     * Dispose of all registered participants
     */
    public dispose(): void {
        this.clear();
    }
}