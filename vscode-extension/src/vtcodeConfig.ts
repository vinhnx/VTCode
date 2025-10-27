import * as vscode from 'vscode';
import { join } from 'node:path';
import * as TOML from '@iarna/toml';

export interface VtcodeMcpProviderSummary {
    readonly name: string;
    readonly enabled?: boolean;
    readonly command?: string;
    readonly args?: string[];
}

export interface VtcodeConfigSummary {
    readonly hasConfig: boolean;
    readonly uri?: vscode.Uri;
    readonly humanInTheLoop?: boolean;
    readonly toolDefaultPolicy?: string;
    readonly toolPoliciesCount?: number;
    readonly mcpEnabled?: boolean;
    readonly mcpProviders: VtcodeMcpProviderSummary[];
    readonly parseError?: string;
}

export type VtcodeConfigUpdateHandler = (summary: VtcodeConfigSummary) => void;

function asRecord(value: unknown): Record<string, unknown> | undefined {
    if (value && typeof value === 'object' && !Array.isArray(value)) {
        return value as Record<string, unknown>;
    }

    return undefined;
}

function asBoolean(value: unknown): boolean | undefined {
    return typeof value === 'boolean' ? value : undefined;
}

function asString(value: unknown): string | undefined {
    return typeof value === 'string' ? value : undefined;
}

function asArray(value: unknown): unknown[] | undefined {
    return Array.isArray(value) ? value : undefined;
}

export async function registerVtcodeConfigWatcher(
    context: vscode.ExtensionContext,
    onUpdate: VtcodeConfigUpdateHandler
): Promise<void> {
    if (!vscode.workspace.workspaceFolders || vscode.workspace.workspaceFolders.length === 0) {
        onUpdate({ hasConfig: false, mcpProviders: [] });
        return;
    }

    const watcher = vscode.workspace.createFileSystemWatcher('**/vtcode.toml');
    context.subscriptions.push(watcher);

    const refresh = async () => {
        const summary = await loadPrimaryConfigSummary();
        onUpdate(summary);
    };

    const scheduleRefresh = () => {
        void refresh();
    };

    watcher.onDidCreate(scheduleRefresh, undefined, context.subscriptions);
    watcher.onDidChange(scheduleRefresh, undefined, context.subscriptions);
    watcher.onDidDelete(scheduleRefresh, undefined, context.subscriptions);

    await refresh();
}

export async function pickVtcodeConfigUri(preferred?: vscode.Uri): Promise<vscode.Uri | undefined> {
    const matches = await vscode.workspace.findFiles('**/vtcode.toml', '**/{node_modules,dist,out,.git,target}/**', 10);
    if (matches.length === 0) {
        return undefined;
    }

    if (preferred && matches.some((candidate) => candidate.toString() === preferred.toString())) {
        return preferred;
    }

    if (matches.length === 1) {
        return matches[0];
    }

    const items = matches.map((uri) => ({
        label: vscode.workspace.asRelativePath(uri, false),
        uri
    }));

    const selection = await vscode.window.showQuickPick(items, {
        placeHolder: 'Select the vtcode.toml to use'
    });

    return selection?.uri;
}

export async function revealToolsPolicySection(uri: vscode.Uri): Promise<void> {
    const document = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(document, { preview: false });
    const text = document.getText();
    const toolsPoliciesIndex = text.indexOf('[tools.policies]');
    if (toolsPoliciesIndex >= 0) {
        const position = document.positionAt(toolsPoliciesIndex);
        editor.selection = new vscode.Selection(position, position);
        editor.revealRange(new vscode.Range(position, position), vscode.TextEditorRevealType.InCenter);
        return;
    }

    const toolsIndex = text.indexOf('[tools]');
    if (toolsIndex >= 0) {
        const position = document.positionAt(toolsIndex);
        editor.selection = new vscode.Selection(position, position);
        editor.revealRange(new vscode.Range(position, position), vscode.TextEditorRevealType.InCenter);
        return;
    }

    const endPosition = document.positionAt(text.length);
    editor.selection = new vscode.Selection(endPosition, endPosition);
    editor.revealRange(new vscode.Range(endPosition, endPosition), vscode.TextEditorRevealType.InCenter);
}

export async function revealMcpSection(uri: vscode.Uri): Promise<void> {
    const document = await vscode.workspace.openTextDocument(uri);
    const editor = await vscode.window.showTextDocument(document, { preview: false });
    const text = document.getText();
    const mcpIndex = text.indexOf('[mcp]');
    if (mcpIndex >= 0) {
        const position = document.positionAt(mcpIndex);
        editor.selection = new vscode.Selection(position, position);
        editor.revealRange(new vscode.Range(position, position), vscode.TextEditorRevealType.InCenter);
    } else {
        const endPosition = document.positionAt(text.length);
        editor.selection = new vscode.Selection(endPosition, endPosition);
        editor.revealRange(new vscode.Range(endPosition, endPosition), vscode.TextEditorRevealType.InCenter);
    }
}

export async function setHumanInTheLoop(uri: vscode.Uri, enabled: boolean): Promise<boolean> {
    const document = await vscode.workspace.openTextDocument(uri);
    const text = document.getText();
    const match = text.match(/^(\s*human_in_the_loop\s*=\s*)(true|false)/m);
    const edit = new vscode.WorkspaceEdit();

    if (match && match.index !== undefined) {
        const start = document.positionAt(match.index);
        const end = document.positionAt(match.index + match[0].length);
        edit.replace(uri, new vscode.Range(start, end), `${match[1]}${enabled}`);
    } else {
        const securityMatch = text.match(/^\s*\[security\]\s*$/m);
        if (securityMatch && securityMatch.index !== undefined) {
            const securityLine = document.positionAt(securityMatch.index).line;
            const insertPosition = new vscode.Position(securityLine + 1, 0);
            edit.insert(uri, insertPosition, `human_in_the_loop = ${enabled}\n`);
        } else {
            const appendText = `${text.endsWith('\n') ? '' : '\n'}[security]\nhuman_in_the_loop = ${enabled}\n`;
            const endPosition = document.positionAt(text.length);
            edit.insert(uri, endPosition, appendText);
        }
    }

    const applied = await vscode.workspace.applyEdit(edit);
    if (applied) {
        await document.save();
    }
    return applied;
}

export async function setMcpProviderEnabled(
    uri: vscode.Uri,
    providerName: string,
    enabled: boolean
): Promise<'updated' | 'nochange' | 'notfound'> {
    const document = await vscode.workspace.openTextDocument(uri);
    const text = document.getText();
    const block = findMcpProviderBlock(document, text, providerName);
    if (!block) {
        return 'notfound';
    }

    const currentValue = block.enabledValue;
    if (currentValue === enabled) {
        return 'nochange';
    }

    const edit = new vscode.WorkspaceEdit();
    if (block.enabledRange) {
        edit.replace(uri, block.enabledRange, `enabled = ${enabled}`);
    } else {
        const insertPosition = new vscode.Position(block.insertLine, 0);
        const indentation = block.indentation ?? '';
        edit.insert(uri, insertPosition, `${indentation}enabled = ${enabled}\n`);
    }

    const applied = await vscode.workspace.applyEdit(edit);
    if (applied) {
        await document.save();
        return 'updated';
    }

    return 'nochange';
}

export async function appendMcpProvider(
    uri: vscode.Uri,
    provider: { name: string; command: string; args: string[]; enabled: boolean }
): Promise<boolean> {
    const document = await vscode.workspace.openTextDocument(uri);
    const text = document.getText();
    const existing = findMcpProviderBlock(document, text, provider.name);
    if (existing) {
        return false;
    }

    const snippetLines = [
        '',
        '[[mcp.providers]]',
        `name = "${provider.name}"`,
        `command = "${provider.command}"`
    ];

    if (provider.args.length > 0) {
        const serializedArgs = provider.args.map((arg) => `"${arg.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`).join(', ');
        snippetLines.push(`args = [${serializedArgs}]`);
    }

    snippetLines.push(`enabled = ${provider.enabled}`);
    snippetLines.push('');

    const appendText = snippetLines.join('\n');
    const endPosition = document.positionAt(text.length);
    const edit = new vscode.WorkspaceEdit();
    edit.insert(uri, endPosition, `${text.endsWith('\n') ? '' : '\n'}${appendText}`);

    const applied = await vscode.workspace.applyEdit(edit);
    if (applied) {
        await document.save();
    }
    return applied;
}

export async function loadConfigSummaryFromUri(uri: vscode.Uri): Promise<VtcodeConfigSummary> {
    const bytes = await vscode.workspace.fs.readFile(uri);
    const text = Buffer.from(bytes).toString('utf8');
    try {
        const parsed = TOML.parse(text) as unknown;
        return buildSummaryFromParsed(uri, parsed);
    } catch (error) {
        return { hasConfig: true, uri, mcpProviders: [], parseError: error instanceof Error ? error.message : String(error) };
    }
}

async function loadPrimaryConfigSummary(): Promise<VtcodeConfigSummary> {
    const primary = await guessPrimaryConfigUri();
    if (!primary) {
        return { hasConfig: false, mcpProviders: [] };
    }

    return loadConfigSummaryFromUri(primary);
}

async function guessPrimaryConfigUri(): Promise<vscode.Uri | undefined> {
    const matches = await vscode.workspace.findFiles('**/vtcode.toml', '**/{node_modules,dist,out,.git,target}/**', 10);
    if (matches.length === 0) {
        return undefined;
    }

    if (matches.length === 1) {
        return matches[0];
    }

    const workspaceFolders = vscode.workspace.workspaceFolders;
    if (workspaceFolders) {
        for (const folder of workspaceFolders) {
            const rootCandidate = matches.find((uri) => uri.fsPath === join(folder.uri.fsPath, 'vtcode.toml'));
            if (rootCandidate) {
                return rootCandidate;
            }
        }
    }

    return matches.sort((a, b) => a.fsPath.length - b.fsPath.length)[0];
}

function buildSummaryFromParsed(uri: vscode.Uri, parsed: unknown): VtcodeConfigSummary {
    const root = asRecord(parsed) ?? {};
    const security = asRecord(root['security']);
    const tools = asRecord(root['tools']);
    const mcp = asRecord(root['mcp']);

    const humanInTheLoop = asBoolean(security?.['human_in_the_loop']);
    const toolDefaultPolicy = asString(tools?.['default_policy']);
    const toolPoliciesRecord = tools ? asRecord(tools['policies']) : undefined;
    const toolPoliciesCount = toolPoliciesRecord ? Object.keys(toolPoliciesRecord).length : 0;

    const providersRaw = mcp ? asArray(mcp['providers']) ?? [] : [];
    const mcpProviders: VtcodeMcpProviderSummary[] = [];
    providersRaw.forEach((entry) => {
        const provider = asRecord(entry);
        if (!provider) {
            return;
        }

        const name = asString(provider['name']);
        if (!name) {
            return;
        }

        const command = asString(provider['command']);
        const enabledValue = asBoolean(provider['enabled']);
        const argsCandidates = asArray(provider['args']);
        const args = argsCandidates?.filter((value): value is string => typeof value === 'string');

        mcpProviders.push({
            name,
            command,
            enabled: enabledValue,
            args
        });
    });

    return {
        hasConfig: true,
        uri,
        humanInTheLoop,
        toolDefaultPolicy,
        toolPoliciesCount,
        mcpEnabled: asBoolean(mcp?.['enabled']),
        mcpProviders
    };
}

interface McpProviderBlock {
    readonly enabledRange?: vscode.Range;
    readonly enabledValue?: boolean;
    readonly indentation?: string;
    readonly insertLine: number;
}

function findMcpProviderBlock(
    document: vscode.TextDocument,
    text: string,
    providerName: string
): McpProviderBlock | undefined {
    const lineOffsets: number[] = [];
    let offset = 0;
    const lines = text.split(/\r?\n/);
    for (const line of lines) {
        lineOffsets.push(offset);
        offset += line.length + 1;
    }

    const normalizedProvider = providerName.trim().toLowerCase();

    for (let index = 0; index < lines.length; index += 1) {
        const line = lines[index];
        if (line.trim() !== '[[mcp.providers]]') {
            continue;
        }

        let name: string | undefined;
        let enabledRange: vscode.Range | undefined;
        let enabledValue: boolean | undefined;
        let indentation: string | undefined;
        let insertLine = index + 1;

        for (let inner = index + 1; inner < lines.length; inner += 1) {
            const candidate = lines[inner];
            const trimmed = candidate.trim();
            if (trimmed.startsWith('[') && !trimmed.startsWith('#')) {
                insertLine = inner;
                break;
            }

            if (trimmed.length === 0) {
                insertLine = inner + 1;
                continue;
            }

            const nameMatch = trimmed.match(/^name\s*=\s*"(.+?)"/);
            if (nameMatch) {
                name = nameMatch[1].trim();
            }

            const enabledMatch = trimmed.match(/^enabled\s*=\s*(true|false)/);
            if (enabledMatch) {
                const matchOffset = candidate.indexOf(enabledMatch[0]);
                if (matchOffset >= 0) {
                    const startOffset = lineOffsets[inner] + matchOffset;
                    const start = document.positionAt(startOffset);
                    const end = document.positionAt(startOffset + enabledMatch[0].length);
                    enabledRange = new vscode.Range(start, end);
                    enabledValue = enabledMatch[1] === 'true';
                    indentation = candidate.slice(0, matchOffset);
                }
            }

            if (!indentation && trimmed && !trimmed.startsWith('#')) {
                indentation = candidate.slice(0, candidate.indexOf(trimmed));
            }
        }

        if (name && name.trim().toLowerCase() === normalizedProvider) {
            return {
                enabledRange,
                enabledValue,
                indentation,
                insertLine
            };
        }
    }

    return undefined;
}
