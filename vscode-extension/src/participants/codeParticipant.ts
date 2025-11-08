import * as vscode from "vscode";
import { BaseParticipant, type ParticipantContext } from "../types/participant";

/**
 * Code participant provides code-specific context and analysis
 */
export class CodeParticipant extends BaseParticipant {
    public readonly id = "code";
    public readonly displayName = "Code";
    public readonly description = "Provides code context, syntax, and analysis";
    public readonly icon = "code";

    canHandle(context: ParticipantContext): boolean {
        // Available when there's an active file with code
        return context.activeFile !== undefined && 
               context.activeFile.language !== 'text' &&
               context.activeFile.language !== 'markdown';
    }

    async resolveReferenceContext(message: string, context: ParticipantContext): Promise<string> {
        if (!this.extractMention(message, this.id)) {
            return message;
        }

        const activeFile = context.activeFile;
        if (!activeFile) {
            return message;
        }

        // Clean the message first
        const cleanedMessage = this.cleanMessage(message, this.id);

        // Get file information
        const filePath = activeFile.path;
        const language = activeFile.language;
        const workspace = context.workspace;
        
        let relativePath = filePath;
        if (workspace && this.isFileInWorkspace(filePath, context)) {
            relativePath = vscode.workspace.asRelativePath(filePath, false);
        }

        // Build code context
        let codeContext = `\n\n## Code Context\n`;
        codeContext += `File: ${relativePath}\n`;
        codeContext += `Language: ${language}\n`;

        // Add selection information if available
        if (activeFile.selection && !activeFile.selection.isEmpty) {
            const startLine = activeFile.selection.start.line + 1;
            const endLine = activeFile.selection.end.line + 1;
            codeContext += `Selection: Lines ${startLine}-${endLine}\n`;
            
            // Add the selected code if content is available
            if (activeFile.content) {
                const lines = activeFile.content.split('\n');
                const selectedLines = lines.slice(
                    activeFile.selection.start.line,
                    activeFile.selection.end.line + 1
                );
                if (selectedLines.length > 0) {
                    codeContext += `\nSelected code:\n\`\`\`${language}\n${selectedLines.join('\n')}\n\`\`\`\n`;
                }
            }
        } else if (activeFile.content) {
            // Add a snippet of the file if no selection
            const lines = activeFile.content.split('\n');
            const snippetLines = lines.slice(0, 50); // First 50 lines
            if (snippetLines.length > 0) {
                codeContext += `\nFile snippet:\n\`\`\`${language}\n${snippetLines.join('\n')}\n\`\`\`\n`;
            }
        }

        // Add language-specific information
        const languageInfo = this.getLanguageInfo(language);
        if (languageInfo) {
            codeContext += `\nLanguage details: ${languageInfo}\n`;
        }

        return `${cleanedMessage}${codeContext}`;
    }

    private getLanguageInfo(language: string): string | undefined {
        const languageMap: Record<string, string> = {
            'typescript': 'Statically typed superset of JavaScript',
            'javascript': 'Dynamic scripting language',
            'python': 'Interpreted, high-level programming language',
            'rust': 'Systems programming language with memory safety',
            'go': 'Compiled language designed for simplicity',
            'java': 'Object-oriented programming language',
            'cpp': 'C++ - Systems programming language',
            'c': 'C - Low-level systems programming language',
            'ruby': 'Dynamic, object-oriented scripting language',
            'php': 'Server-side scripting language',
            'swift': 'Apple\'s modern programming language',
            'kotlin': 'Modern language for JVM and Android',
            'csharp': 'Microsoft .NET programming language',
            'fsharp': 'Functional-first .NET programming language',
            'haskell': 'Pure functional programming language',
            'scala': 'JVM language combining OOP and functional',
            'clojure': 'Lisp dialect for the JVM',
            'elixir': 'Functional language for concurrent systems',
            'erlang': 'Concurrent functional programming language',
            'r': 'Language for statistical computing',
            'julia': 'High-level dynamic programming language',
            'matlab': 'Numerical computing environment',
            'sql': 'Database query language',
            'html': 'Markup language for web pages',
            'css': 'Style sheet language for web pages',
            'json': 'JavaScript Object Notation',
            'yaml': 'YAML Ain\'t Markup Language',
            'xml': 'Extensible Markup Language',
            'markdown': 'Lightweight markup language',
            'dockerfile': 'Docker container definition',
            'shellscript': 'Shell scripting language',
            'bash': 'Bourne Again Shell scripting',
            'powershell': 'Microsoft automation and configuration tool',
        };

        return languageMap[language.toLowerCase()];
    }
}