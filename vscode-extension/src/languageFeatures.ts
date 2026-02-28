import * as vscode from "vscode";

interface SectionMetadata {
    readonly label: string;
    readonly description: string;
}

interface KeyMetadata {
    readonly detail: string;
    readonly documentation: string;
    readonly insertText?: string;
}

const SECTION_METADATA: Record<string, SectionMetadata> = {
    agent: {
        label: "Agent",
        description: "Core VT Code agent behavior and model defaults.",
    },
    "agent.onboarding": {
        label: "Onboarding",
        description: "Customize the guidance presented when VT Code starts.",
    },
    prompt_cache: {
        label: "Prompt Cache",
        description: "Caching behavior that reduces repeated API calls.",
    },
    security: {
        label: "Security",
        description: "Safety controls including human-in-the-loop enforcement.",
    },
    tools: {
        label: "Tool Policies",
        description: "Global defaults and execution limits for VT Code tools.",
    },
    "tools.policies": {
        label: "Tool Policy Overrides",
        description: "Per-tool allow, prompt, or deny overrides.",
    },
    commands: {
        label: "Command Safelist",
        description: "Allow and deny lists for terminal commands.",
    },
    mcp: {
        label: "Model Context Protocol",
        description: "Settings for connecting VT Code to MCP-compatible tools.",
    },
    "mcp.providers": {
        label: "MCP Provider",
        description: "Declare individual MCP tool providers.",
    },
    acp: {
        label: "Agent Client Protocol",
        description: "Configure IDE integrations that communicate via ACP.",
    },
};

const SECTION_KEYS: Record<string, Record<string, KeyMetadata>> = {
    agent: {
        provider: {
            detail: "Primary LLM provider",
            documentation:
                "Sets the default provider that VT Code should use for conversations.",
            insertText: 'provider = "${1|openai,anthropic,gemini,openrouter|}"',
        },
        api_key_env: {
            detail: "Environment variable for the API key",
            documentation:
                "Points VT Code at the environment variable that stores credentials for the selected provider.",
            insertText: 'api_key_env = "${1:OPENAI_API_KEY}"',
        },
        default_model: {
            detail: "Default model identifier",
            documentation:
                "Name of the model to use when an explicit model is not requested.",
            insertText: 'default_model = "${1:gpt-5-nano}"',
        },
        theme: {
            detail: "Terminal theme",
            documentation: "Controls the VT Code terminal interface theme.",
            insertText:
                'theme = "${1|ciapre,ciapre-dark,ciapre-blue,solarized-dark,solarized-light,solarized-dark-hc,gruvbox-dark,gruvbox-dark-hard,gruvbox-light,gruvbox-light-hard,gruvbox-material,gruvbox-material-dark,gruvbox-material-light,zenburn,tomorrow,tomorrow-night,tomorrow-night-blue,tomorrow-night-bright,tomorrow-night-burns,tomorrow-night-eighties,catppuccin-macchiato,kanagawa|}"',
        },
        todo_planning_mode: {
            detail: "Enable structured planning",
            documentation:
                "Toggle to have VT Code create TODO plans for larger tasks automatically.",
            insertText: "todo_planning_mode = ${1|true,false|}",
        },
        ui_surface: {
            detail: "UI rendering surface",
            documentation:
                "Choose how VT Code should present output in the terminal UI.",
            insertText: 'ui_surface = "${1|auto,alternate,inline|}"',
        },
        reasoning_effort: {
            detail: "Reasoning effort level",
            documentation:
                "Controls how much deliberation VT Code applies when solving a task.",
            insertText: 'reasoning_effort = "${1|low,medium,high|}"',
        },
    },
    "agent.onboarding": {
        enabled: {
            detail: "Toggle onboarding",
            documentation:
                "Determines whether VT Code shows the onboarding introduction at startup.",
            insertText: "enabled = ${1|true,false|}",
        },
        intro_text: {
            detail: "Custom introduction",
            documentation:
                "Overrides the greeting text shown when VT Code launches.",
            insertText: 'intro_text = "${1}"',
        },
        include_project_overview: {
            detail: "Include project overview",
            documentation:
                "Adds a workspace overview to the onboarding output when true.",
            insertText: "include_project_overview = ${1|true,false|}",
        },
        include_guideline_highlights: {
            detail: "Include guideline highlights",
            documentation:
                "Highlights AGENTS.md guidance during onboarding when enabled.",
            insertText: "include_guideline_highlights = ${1|true,false|}",
        },
    },
    prompt_cache: {
        enabled: {
            detail: "Enable prompt cache",
            documentation:
                "Turns caching on or off for repeated prompt responses.",
            insertText: "enabled = ${1|true,false|}",
        },
        cache_dir: {
            detail: "Cache directory",
            documentation:
                "Filesystem path where VT Code should persist prompt cache entries.",
            insertText: 'cache_dir = "${1:~/.vtcode/cache/prompts}"',
        },
        max_entries: {
            detail: "Maximum cache entries",
            documentation:
                "Controls the total number of cached responses to keep.",
            insertText: "max_entries = ${1:1000}",
        },
        enable_auto_cleanup: {
            detail: "Automatic cleanup",
            documentation:
                "Whether VT Code should prune stale cache entries automatically.",
            insertText: "enable_auto_cleanup = ${1|true,false|}",
        },
        "providers.openai.prompt_cache_retention": {
            detail: "OpenAI prompt cache retention",
            documentation:
                'Optional Responses API prompt cache retention string (for example "24h") to improve cache hits on repeated prompts.',
            insertText: 'providers.openai.prompt_cache_retention = "24h"',
        },
    },
    security: {
        human_in_the_loop: {
            detail: "Require human approval",
            documentation:
                "When true, VT Code pauses for approval before executing high-impact tools.",
            insertText: "human_in_the_loop = ${1|true,false|}",
        },
        require_write_tool_for_claims: {
            detail: "Enforce write tool usage",
            documentation:
                "Ensures VT Code must use explicit write tools before claiming file changes.",
            insertText: "require_write_tool_for_claims = ${1|true,false|}",
        },
        auto_apply_detected_patches: {
            detail: "Auto-apply patches",
            documentation:
                "Automatically apply detected patches without confirmation (dangerous).",
            insertText: "auto_apply_detected_patches = ${1|true,false|}",
        },
    },
    tools: {
        default_policy: {
            detail: "Default tool policy",
            documentation:
                "Global default for tools without explicit overrides (allow, prompt, or deny).",
            insertText: 'default_policy = "${1|allow,prompt,deny|}"',
        },
        max_tool_loops: {
            detail: "Max tool loops",
            documentation: "Upper bound on tool iterations per agent turn.",
            insertText: "max_tool_loops = ${1:20}",
        },
        max_repeated_tool_calls: {
            detail: "Max repeated calls",
            documentation:
                "Prevents the agent from looping on the same tool excessively.",
            insertText: "max_repeated_tool_calls = ${1:2}",
        },
    },
    "tools.policies": {
        apply_patch: {
            detail: "apply_patch policy",
            documentation: "Control whether apply_patch requires approval.",
            insertText: 'apply_patch = "${1|allow,prompt,deny|}"',
        },
        bash: {
            detail: "bash policy",
            documentation: "Policy override for shell execution.",
            insertText: 'bash = "${1|allow,prompt,deny|}"',
        },
        run_pty_cmd: {
            detail: "run_pty_cmd policy",
            documentation: "Policy override for standard pty commands.",
            insertText: 'run_pty_cmd = "${1|allow,prompt,deny|}"',
        },
        write_file: {
            detail: "write_file policy",
            documentation: "Policy override for direct file writes.",
            insertText: 'write_file = "${1|allow,prompt,deny|}"',
        },
    },
    commands: {
        allow_list: {
            detail: "Allowed commands",
            documentation: "Exact commands always permitted without approval.",
            insertText: 'allow_list = [${1:"git status"}]',
        },
        deny_list: {
            detail: "Denied commands",
            documentation: "Commands that are always blocked.",
            insertText: 'deny_list = [${1:"rm"}]',
        },
        allow_glob: {
            detail: "Allowed glob patterns",
            documentation:
                "Glob patterns that mark commands as always allowed.",
            insertText: 'allow_glob = [${1:"git *"}]',
        },
        deny_glob: {
            detail: "Denied glob patterns",
            documentation: "Glob patterns that block matching commands.",
            insertText: 'deny_glob = [${1:"sudo *"}]',
        },
        allow_regex: {
            detail: "Allowed regex patterns",
            documentation:
                "Optional regex expressions for allowlisting commands.",
            insertText: "allow_regex = [${1}]",
        },
        deny_regex: {
            detail: "Denied regex patterns",
            documentation: "Optional regex expressions for blocking commands.",
            insertText: "deny_regex = [${1}]",
        },
    },
    mcp: {
        enabled: {
            detail: "Enable MCP integration",
            documentation:
                "Allows VT Code to connect to Model Context Protocol tools.",
            insertText: "enabled = ${1|true,false|}",
        },
        max_concurrent_connections: {
            detail: "Maximum concurrent connections",
            documentation:
                "Limits how many MCP connections VT Code may open simultaneously.",
            insertText: "max_concurrent_connections = ${1:5}",
        },
        request_timeout_seconds: {
            detail: "MCP request timeout",
            documentation:
                "How long VT Code should wait before timing out an MCP request.",
            insertText: "request_timeout_seconds = ${1:30}",
        },
        retry_attempts: {
            detail: "Retry attempts",
            documentation: "Number of times VT Code retries failed MCP calls.",
            insertText: "retry_attempts = ${1:3}",
        },
    },
    "mcp.providers": {
        name: {
            detail: "Provider name",
            documentation: "Unique identifier for the MCP provider entry.",
            insertText: 'name = "${1}"',
        },
        command: {
            detail: "Executable command",
            documentation: "Command invoked to launch the MCP provider.",
            insertText: 'command = "${1:uvx}"',
        },
        args: {
            detail: "Command arguments",
            documentation: "Arguments passed to the provider command.",
            insertText: "args = [${1}]",
        },
        enabled: {
            detail: "Enable provider",
            documentation:
                "Toggles whether the MCP provider should be activated.",
            insertText: "enabled = ${1|true,false|}",
        },
    },
    acp: {
        enabled: {
            detail: "Enable ACP integration",
            documentation:
                "Controls whether the Agent Client Protocol bridge is active.",
            insertText: "enabled = ${1|true,false|}",
        },
    },
};

export const VT_CODE_DOCUMENT_SELECTOR: vscode.DocumentSelector = [
    { language: "vtcode-config", scheme: "file" },
    { language: "vtcode-config", scheme: "untitled" },
    { pattern: "**/vtcode.toml", scheme: "file" },
];

const SECTION_COMPLETIONS = createSectionCompletions();

export function registerVtcodeLanguageFeatures(
    context: vscode.ExtensionContext
): vscode.Disposable[] {
    const selector = VT_CODE_DOCUMENT_SELECTOR;

    const completionProvider = vscode.languages.registerCompletionItemProvider(
        selector,
        {
            provideCompletionItems(document, position) {
                const currentSection = getCurrentSection(document, position);
                const linePrefix = document
                    .lineAt(position.line)
                    .text.slice(0, position.character);

                if (/^\s*\[/.test(linePrefix)) {
                    return SECTION_COMPLETIONS;
                }

                if (!currentSection) {
                    return SECTION_COMPLETIONS;
                }

                const keyEntries = getKeyMetadata(currentSection);
                if (!keyEntries) {
                    return undefined;
                }

                return keyEntries.map(([key, metadata]) =>
                    createKeyCompletion(key, metadata)
                );
            },
        },
        "[",
        " "
    );

    const hoverProvider = vscode.languages.registerHoverProvider(selector, {
        provideHover(document, position) {
            const headerHover = getSectionHover(document, position);
            if (headerHover) {
                return headerHover;
            }

            const keyHover = getKeyHover(document, position);
            if (keyHover) {
                return keyHover;
            }

            return undefined;
        },
    });

    const symbolProvider = vscode.languages.registerDocumentSymbolProvider(
        selector,
        {
            provideDocumentSymbols(document) {
                return createDocumentSymbols(document);
            },
        }
    );

    const disposables = [completionProvider, hoverProvider, symbolProvider];
    context.subscriptions.push(...disposables);
    return disposables;
}

function createSectionCompletions(): vscode.CompletionItem[] {
    const entries: Array<[string, SectionMetadata & { snippet: string }]> = [
        ["agent", { ...SECTION_METADATA.agent, snippet: "[agent]\n$0" }],
        [
            "agent.onboarding",
            {
                ...SECTION_METADATA["agent.onboarding"],
                snippet: "[agent.onboarding]\n$0",
            },
        ],
        [
            "prompt_cache",
            { ...SECTION_METADATA.prompt_cache, snippet: "[prompt_cache]\n$0" },
        ],
        [
            "security",
            {
                ...SECTION_METADATA.security,
                snippet:
                    "[security]\nhuman_in_the_loop = true\nrequire_write_tool_for_claims = true\n$0",
            },
        ],
        [
            "tools",
            {
                ...SECTION_METADATA.tools,
                snippet:
                    '[tools]\ndefault_policy = "prompt"\nmax_tool_loops = 20\n$0',
            },
        ],
        [
            "tools.policies",
            {
                ...SECTION_METADATA["tools.policies"],
                snippet: '[tools.policies]\napply_patch = "prompt"\n$0',
            },
        ],
        [
            "commands",
            {
                ...SECTION_METADATA.commands,
                snippet: '[commands]\nallow_list = ["git status"]\n$0',
            },
        ],
        ["mcp", { ...SECTION_METADATA.mcp, snippet: "[mcp]\n$0" }],
        [
            "mcp.providers",
            {
                ...SECTION_METADATA["mcp.providers"],
                snippet:
                    '[[mcp.providers]]\nname = "${1}"\ncommand = "${2}"\nargs = [${3}]\nenabled = ${4|true,false|}\n$0',
            },
        ],
        ["acp", { ...SECTION_METADATA.acp, snippet: "[acp]\n$0" }],
    ];

    return entries.map(([section, metadata]) => {
        const item = new vscode.CompletionItem(
            section,
            vscode.CompletionItemKind.Module
        );
        item.detail = metadata.label;
        item.documentation = new vscode.MarkdownString(metadata.description);
        item.insertText = new vscode.SnippetString(metadata.snippet);
        item.sortText = `0_${section}`;
        return item;
    });
}

function createKeyCompletion(
    key: string,
    metadata: KeyMetadata
): vscode.CompletionItem {
    const item = new vscode.CompletionItem(
        key,
        vscode.CompletionItemKind.Property
    );
    item.detail = metadata.detail;
    item.documentation = new vscode.MarkdownString(metadata.documentation);
    item.insertText = metadata.insertText
        ? new vscode.SnippetString(metadata.insertText)
        : undefined;
    item.sortText = `1_${key}`;
    return item;
}

function getCurrentSection(
    document: vscode.TextDocument,
    position: vscode.Position
): string | undefined {
    for (let lineNumber = position.line; lineNumber >= 0; lineNumber -= 1) {
        const text = document.lineAt(lineNumber).text;
        const match = text.match(/^\s*(\[\[|\[)\s*([^\]]+?)\s*\]{1,2}\s*$/);
        if (match) {
            return match[2];
        }
    }

    return undefined;
}

function getKeyMetadata(
    section: string
): Array<[string, KeyMetadata]> | undefined {
    if (SECTION_KEYS[section]) {
        return Object.entries(SECTION_KEYS[section]);
    }

    const parentSection = section.includes(".")
        ? section.split(".").slice(0, -1).join(".")
        : undefined;
    if (parentSection && SECTION_KEYS[parentSection]) {
        return Object.entries(SECTION_KEYS[parentSection]);
    }

    return undefined;
}

function getSectionHover(
    document: vscode.TextDocument,
    position: vscode.Position
): vscode.Hover | undefined {
    const range = document.getWordRangeAtPosition(position, /[A-Za-z0-9_.]+/);
    if (!range) {
        return undefined;
    }

    const lineText = document.lineAt(position.line).text.trim();
    if (!lineText.startsWith("[")) {
        return undefined;
    }

    const section = lineText.replace(/^\[+/, "").replace(/\]+$/, "");
    const metadata = SECTION_METADATA[section];
    if (!metadata) {
        return undefined;
    }

    const contents = new vscode.MarkdownString();
    contents.appendMarkdown(`**${metadata.label}**  \n${metadata.description}`);
    return new vscode.Hover(contents, range);
}

function getKeyHover(
    document: vscode.TextDocument,
    position: vscode.Position
): vscode.Hover | undefined {
    const range = document.getWordRangeAtPosition(
        position,
        /[A-Za-z_.][A-Za-z0-9_.]+/
    );
    if (!range) {
        return undefined;
    }

    const key = document.getText(range);
    const section = getCurrentSection(document, position);
    if (!section) {
        return undefined;
    }

    const keyEntries = getKeyMetadata(section);
    const metadata = keyEntries?.find(([candidate]) => candidate === key)?.[1];
    if (!metadata) {
        return undefined;
    }

    const contents = new vscode.MarkdownString();
    contents.appendMarkdown(
        `**${key}** — ${metadata.detail}\n\n${metadata.documentation}`
    );
    return new vscode.Hover(contents, range);
}

function createDocumentSymbols(
    document: vscode.TextDocument
): vscode.DocumentSymbol[] {
    const sectionLines: Array<{ section: string; startLine: number }> = [];

    for (let line = 0; line < document.lineCount; line += 1) {
        const text = document.lineAt(line).text;
        const match = text.match(/^\s*(\[\[|\[)\s*([^\]]+?)\s*\]{1,2}\s*$/);
        if (!match) {
            continue;
        }

        const section = match[2];
        if (!sectionLines.some((entry) => entry.section === section)) {
            sectionLines.push({ section, startLine: line });
        }
    }

    const rootSymbols: vscode.DocumentSymbol[] = [];
    const symbolMap = new Map<string, vscode.DocumentSymbol>();

    sectionLines.forEach((entry, index) => {
        const endLine =
            index + 1 < sectionLines.length
                ? sectionLines[index + 1].startLine - 1
                : document.lineCount - 1;
        const selectionRange = new vscode.Range(
            entry.startLine,
            0,
            entry.startLine,
            document.lineAt(entry.startLine).text.length
        );
        const range = new vscode.Range(
            entry.startLine,
            0,
            endLine,
            document.lineAt(endLine).text.length
        );
        const segments = entry.section.split(".");

        let currentPath = "";
        let parent: vscode.DocumentSymbol | undefined;

        segments.forEach((segment, segmentIndex) => {
            currentPath = currentPath ? `${currentPath}.${segment}` : segment;
            let symbol = symbolMap.get(currentPath);

            if (!symbol) {
                const metadata = SECTION_METADATA[currentPath];
                symbol = new vscode.DocumentSymbol(
                    metadata?.label ?? segment,
                    metadata?.description ?? "",
                    vscode.SymbolKind.Module,
                    range,
                    selectionRange
                );

                symbolMap.set(currentPath, symbol);

                if (parent) {
                    parent.children.push(symbol);
                } else {
                    rootSymbols.push(symbol);
                }
            }

            if (segmentIndex === segments.length - 1) {
                symbol.range = range;
                symbol.selectionRange = selectionRange;
            }

            parent = symbol;
        });
    });

    return rootSymbols;
}
