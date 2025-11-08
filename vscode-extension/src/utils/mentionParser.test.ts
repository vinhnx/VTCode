import { parseMentions, getUniqueMentionTypes, hasMentionType, validateMentions } from './mentionParser';

describe('mentionParser', () => {
    describe('parseMentions', () => {
        it('should parse single mention', () => {
            const result = parseMentions('Explain this @code');
            expect(result.hasMentions).toBe(true);
            expect(result.mentions).toHaveLength(1);
            expect(result.mentions[0].type).toBe('code');
            expect(result.mentions[0].raw).toBe('@code');
            expect(result.cleanText).toBe('Explain this');
        });

        it('should parse multiple mentions', () => {
            const result = parseMentions('@workspace analyze @code and @terminal');
            expect(result.hasMentions).toBe(true);
            expect(result.mentions).toHaveLength(3);
            expect(result.mentions.map(m => m.type)).toEqual(['workspace', 'code', 'terminal']);
            expect(result.cleanText).toBe('analyze and');
        });

        it('should handle mentions at start and end', () => {
            const result = parseMentions('@workspace start @code middle @terminal end');
            expect(result.mentions).toHaveLength(3);
            expect(result.cleanText).toBe('start middle end');
        });

        it('should handle no mentions', () => {
            const result = parseMentions('Just a regular message');
            expect(result.hasMentions).toBe(false);
            expect(result.mentions).toHaveLength(0);
            expect(result.cleanText).toBe('Just a regular message');
        });

        it('should handle empty string', () => {
            const result = parseMentions('');
            expect(result.hasMentions).toBe(false);
            expect(result.mentions).toHaveLength(0);
            expect(result.cleanText).toBe('');
        });

        it('should handle only mentions', () => {
            const result = parseMentions('@workspace @code @terminal');
            expect(result.hasMentions).toBe(true);
            expect(result.mentions).toHaveLength(3);
            expect(result.cleanText).toBe('');
        });

        it('should handle duplicate mentions', () => {
            const result = parseMentions('@code and @code again');
            expect(result.mentions).toHaveLength(2);
            expect(result.mentions[0].type).toBe('code');
            expect(result.mentions[1].type).toBe('code');
            expect(result.cleanText).toBe('and again');
        });

        it('should not match email addresses', () => {
            const result = parseMentions('Contact user@example.com @workspace');
            expect(result.mentions).toHaveLength(1);
            expect(result.mentions[0].type).toBe('workspace');
            expect(result.cleanText).toBe('Contact user@example.com');
        });

        it('should handle case sensitivity', () => {
            const result = parseMentions('@Workspace @CODE @Terminal');
            expect(result.mentions.map(m => m.type)).toEqual(['workspace', 'code', 'terminal']);
        });
    });

    describe('getUniqueMentionTypes', () => {
        it('should return unique mention types', () => {
            const parsed = parseMentions('@code and @workspace and @code again');
            const unique = getUniqueMentionTypes(parsed);
            expect(unique).toEqual(['code', 'workspace']);
        });

        it('should return empty array for no mentions', () => {
            const parsed = parseMentions('no mentions here');
            const unique = getUniqueMentionTypes(parsed);
            expect(unique).toEqual([]);
        });
    });

    describe('hasMentionType', () => {
        it('should detect existing mention type', () => {
            const parsed = parseMentions('@code and @workspace');
            expect(hasMentionType(parsed, 'code')).toBe(true);
            expect(hasMentionType(parsed, 'workspace')).toBe(true);
        });

        it('should not detect non-existing mention type', () => {
            const parsed = parseMentions('@code');
            expect(hasMentionType(parsed, 'terminal')).toBe(false);
        });

        it('should be case insensitive', () => {
            const parsed = parseMentions('@CODE');
            expect(hasMentionType(parsed, 'code')).toBe(true);
        });
    });

    describe('validateMentions', () => {
        it('should separate valid and invalid mentions', () => {
            const parsed = parseMentions('@code @workspace @invalid @terminal');
            const available = ['code', 'workspace', 'terminal', 'git'];
            const result = validateMentions(parsed, available);
            
            expect(result.valid).toHaveLength(3);
            expect(result.valid.map(m => m.type)).toEqual(['code', 'workspace', 'terminal']);
            expect(result.invalid).toHaveLength(1);
            expect(result.invalid[0].type).toBe('invalid');
        });

        it('should mark all as valid when all mentions are available', () => {
            const parsed = parseMentions('@code @workspace');
            const available = ['code', 'workspace', 'terminal', 'git'];
            const result = validateMentions(parsed, available);
            
            expect(result.valid).toHaveLength(2);
            expect(result.invalid).toHaveLength(0);
        });

        it('should mark all as invalid when no mentions are available', () => {
            const parsed = parseMentions('@code @workspace');
            const available = ['terminal', 'git'];
            const result = validateMentions(parsed, available);
            
            expect(result.valid).toHaveLength(0);
            expect(result.invalid).toHaveLength(2);
        });
    });
});