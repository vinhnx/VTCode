/**
 * @mention parser for VT Code extension
 * Extracts participant mentions from chat messages and provides context resolution
 */

export interface Mention {
    readonly type: string; // e.g., "workspace", "code", "terminal", "git"
    readonly raw: string; // e.g., "@workspace", "@code"
    readonly startIndex: number;
    readonly endIndex: number;
}

export interface ParsedMessage {
    readonly originalText: string;
    readonly cleanText: string; // Text without @mentions
    readonly mentions: Mention[];
    readonly hasMentions: boolean;
}

/**
 * Regular expression to match @mentions
 * Matches: @workspace, @code, @terminal, @git, etc.
 * Does not match: email addresses, URLs with @ symbols
 */
const MENTION_REGEX = /@(\w+)(?!\w)/g;

/**
 * Parse a message for @mentions
 * @param text The message text to parse
 * @returns Parsed message with mentions extracted
 */
export function parseMentions(text: string): ParsedMessage {
    const mentions: Mention[] = [];
    let cleanText = text;
    let match: RegExpExecArray | null;

    // Reset regex state
    MENTION_REGEX.lastIndex = 0;

    // Find all mentions
    while ((match = MENTION_REGEX.exec(text)) !== null) {
        const [fullMatch, mentionType] = match;
        const startIndex = match.index;
        const endIndex = startIndex + fullMatch.length;

        mentions.push({
            type: mentionType.toLowerCase(),
            raw: fullMatch,
            startIndex,
            endIndex,
        });
    }

    // Remove mentions from text to create clean version
    // Process in reverse order to maintain indices
    cleanText = text;
    const sortedMentions = [...mentions].sort(
        (a, b) => b.startIndex - a.startIndex
    );

    for (const mention of sortedMentions) {
        cleanText =
            cleanText.substring(0, mention.startIndex) +
            cleanText.substring(mention.endIndex);
    }

    // Clean up extra whitespace
    cleanText = cleanText.replace(/\s+/g, " ").trim();

    return {
        originalText: text,
        cleanText,
        mentions,
        hasMentions: mentions.length > 0,
    };
}

/**
 * Get unique mention types from a parsed message
 * @param parsedMessage The parsed message
 * @returns Array of unique mention types
 */
export function getUniqueMentionTypes(parsedMessage: ParsedMessage): string[] {
    return [...new Set(parsedMessage.mentions.map((m) => m.type))];
}

/**
 * Check if a message contains a specific mention type
 * @param parsedMessage The parsed message
 * @param type The mention type to check for
 * @returns True if the message contains the mention type
 */
export function hasMentionType(
    parsedMessage: ParsedMessage,
    type: string
): boolean {
    return parsedMessage.mentions.some((m) => m.type === type.toLowerCase());
}

/**
 * Replace mentions with descriptive placeholders
 * Useful for showing users what context will be added
 * @param parsedMessage The parsed message
 * @returns Text with mentions replaced by descriptions
 */
export function replaceMentionsWithDescriptions(
    parsedMessage: ParsedMessage,
    descriptions: Map<string, string>
): string {
    let result = parsedMessage.originalText;

    // Process mentions in reverse order to maintain indices
    const sortedMentions = [...parsedMessage.mentions].sort(
        (a, b) => b.startIndex - a.startIndex
    );

    for (const mention of sortedMentions) {
        const description =
            descriptions.get(mention.type) || `[${mention.type} context]`;
        result =
            result.substring(0, mention.startIndex) +
            description +
            result.substring(mention.endIndex);
    }

    return result;
}

/**
 * Validate mention types against available participants
 * @param parsedMessage The parsed message
 * @param availableTypes Array of available participant types
 * @returns Object with valid and invalid mentions
 */
export function validateMentions(
    parsedMessage: ParsedMessage,
    availableTypes: string[]
): {
    valid: Mention[];
    invalid: Mention[];
} {
    const valid: Mention[] = [];
    const invalid: Mention[] = [];

    for (const mention of parsedMessage.mentions) {
        if (availableTypes.includes(mention.type)) {
            valid.push(mention);
        } else {
            invalid.push(mention);
        }
    }

    return { valid, invalid };
}
