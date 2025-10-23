import * as vscode from 'vscode';
import { spawn, type SpawnOptionsWithoutStdio } from 'node:child_process';
import { registerVtcodeLanguageFeatures } from './languageFeatures';
import {
    appendMcpProvider,
    loadConfigSummaryFromUri,
    pickVtcodeConfigUri,
    registerVtcodeConfigWatcher,
    revealMcpSection,
    revealToolsPolicySection,
    setHumanInTheLoop,
    setMcpProviderEnabled,
    VtcodeConfigSummary
} from './vtcodeConfig';

type QuickActionItem = vscode.QuickPickItem & { run: () => Thenable<unknown> | void };

interface QuickActionDescription {
    readonly label: string;
    readonly description: string;
    readonly command: string;
    readonly icon?: string;
    readonly args?: unknown[];
}

class QuickActionTreeItem extends vscode.TreeItem {
    constructor(public readonly action: QuickActionDescription) {
        super(action.label, vscode.TreeItemCollapsibleState.None);
        this.description = action.description;
        this.iconPath = new vscode.ThemeIcon(action.icon ?? 'rocket');
        this.command = {
            command: action.command,
            title: action.label,
            arguments: action.args
        };
        this.contextValue = 'vtcodeQuickAction';
    }
}

class QuickActionTreeDataProvider implements vscode.TreeDataProvider<QuickActionTreeItem> {
    private readonly onDidChangeTreeDataEmitter = new vscode.EventEmitter<void>();
    readonly onDidChangeTreeData = this.onDidChangeTreeDataEmitter.event;

    constructor(private readonly getActions: () => QuickActionDescription[]) {}

    getTreeItem(element: QuickActionTreeItem): vscode.TreeItem {
        return element;
    }

    getChildren(): vscode.ProviderResult<QuickActionTreeItem[]> {
        return this.getActions().map((action) => new QuickActionTreeItem(action));
    }

    refresh(): void {
        this.onDidChangeTreeDataEmitter.fire();
    }
}

let outputChannel: vscode.OutputChannel | undefined;
let statusBarItem: vscode.StatusBarItem | undefined;
let quickActionsProviderInstance: QuickActionTreeDataProvider | undefined;
let agentTerminal: vscode.Terminal | undefined;
let terminalCloseListener: vscode.Disposable | undefined;
let cliAvailable = false;
let missingCliWarningShown = false;
let cliAvailabilityCheck: Promise<void> | undefined;
let currentConfigSummary: VtcodeConfigSummary | undefined;
let lastConfigUri: string | undefined;
let lastConfigParseError: string | undefined;

const CLI_DETECTION_TIMEOUT_MS = 4000;

export function activate(context: vscode.ExtensionContext) {
    outputChannel = vscode.window.createOutputChannel('VTCode');
    context.subscriptions.push(outputChannel);

    ensureStableApi(context);
    logExtensionHostContext(context);
    registerVtcodeLanguageFeatures(context);

    statusBarItem = vscode.window.createStatusBarItem(vscode.StatusBarAlignment.Left, 100);
    statusBarItem.name = 'VTCode Quick Actions';
    statusBarItem.accessibilityInformation = { role: 'button', label: 'Open VTCode quick actions or installation guide' };
    context.subscriptions.push(statusBarItem);

    quickActionsProviderInstance = new QuickActionTreeDataProvider(() => createQuickActions(cliAvailable, currentConfigSummary));
    context.subscriptions.push(vscode.window.registerTreeDataProvider('vtcodeQuickActionsView', quickActionsProviderInstance));

    void registerVtcodeConfigWatcher(context, handleConfigUpdate);

    setStatusBarChecking(getConfiguredCommandPath());

    if (vscode.env.uiKind === vscode.UIKind.Web) {
        void vscode.window.showWarningMessage(
            'VTCode Companion is running in VS Code for the Web. Command execution features are disabled, but documentation and configuration helpers remain available.'
        );
    }

    const quickActionsCommand = vscode.commands.registerCommand('vtcode.openQuickActions', async () => {
        const actions = createQuickActions(cliAvailable, currentConfigSummary);
        const pickItems: QuickActionItem[] = actions.map((action) => ({
            label: `${action.icon ? `$(${action.icon}) ` : ''}${action.label}`,
            description: action.description,
            run: () => vscode.commands.executeCommand(action.command, ...(action.args ?? []))
        }));

        const selection = await vscode.window.showQuickPick(pickItems, {
            placeHolder: 'Choose a VTCode action to run'
        });

        if (selection) {
            await selection.run();
        }
    });

    const askAgent = vscode.commands.registerCommand('vtcode.askAgent', async () => {
        if (!(await ensureCliAvailableForCommand())) {
            return;
        }

        const question = await vscode.window.showInputBox({
            prompt: 'What would you like the VTCode agent to help with?',
            placeHolder: 'Summarize src/main.rs',
            ignoreFocusOut: true
        });

        if (!question || !question.trim()) {
            return;
        }

        try {
            await runVtcodeCommand(['ask', question], { title: 'Asking VTCode…' });
            void vscode.window.showInformationMessage('VTCode finished processing your request. Check the VTCode output channel for details.');
        } catch (error) {
            handleCommandError('ask', error);
        }
    });

    const askSelection = vscode.commands.registerCommand('vtcode.askSelection', async () => {
        const editor = vscode.window.activeTextEditor;
        if (!editor) {
            void vscode.window.showWarningMessage('Open a text editor to ask VTCode about the current selection.');
            return;
        }

        const selection = editor.selection;
        if (selection.isEmpty) {
            void vscode.window.showWarningMessage('Highlight text first, then run “Ask About Selection with VTCode.”');
            return;
        }

        const selectedText = editor.document.getText(selection);
        if (!selectedText.trim()) {
            void vscode.window.showWarningMessage('The selected text is empty. Select code or text for VTCode to inspect.');
            return;
        }

        if (!(await ensureCliAvailableForCommand())) {
            return;
        }

        const defaultQuestion = 'Explain the highlighted selection.';
        const question = await vscode.window.showInputBox({
            prompt: 'How should VTCode help with the highlighted selection?',
            value: defaultQuestion,
            valueSelection: [0, defaultQuestion.length],
            ignoreFocusOut: true
        });

        if (question === undefined) {
            return;
        }

        const trimmedQuestion = question.trim() || defaultQuestion;
        const languageId = editor.document.languageId || 'text';
        const rangeLabel = `${selection.start.line + 1}-${selection.end.line + 1}`;
        const workspaceFolder = vscode.workspace.getWorkspaceFolder(editor.document.uri);
        const relativePath = workspaceFolder
            ? vscode.workspace.asRelativePath(editor.document.uri, false)
            : editor.document.fileName;
        const normalizedSelection = selectedText.replace(/\r\n/g, '\n');
        const prompt = `${trimmedQuestion}\n\nFile: ${relativePath}\nLines: ${rangeLabel}\n\n\
\u0060\u0060\u0060${languageId}\n${normalizedSelection}\n\u0060\u0060\u0060`;

        try {
            await runVtcodeCommand(['ask', prompt], { title: 'Asking VTCode about the selection…' });
            void vscode.window.showInformationMessage('VTCode processed the highlighted selection. Review the output channel for the response.');
        } catch (error) {
            handleCommandError('ask about the selection', error);
        }
    });

    const openConfig = vscode.commands.registerCommand('vtcode.openConfig', async () => {
        try {
            const configUri = await pickVtcodeConfigUri(currentConfigSummary?.uri);
            if (!configUri) {
                void vscode.window.showWarningMessage('No vtcode.toml file was found in this workspace.');
                return;
            }

            const document = await vscode.workspace.openTextDocument(configUri);
            await vscode.window.showTextDocument(document, { preview: false });
        } catch (error) {
            handleCommandError('open configuration', error);
        }
    });

    const openDocumentation = vscode.commands.registerCommand('vtcode.openDocumentation', async () => {
        await vscode.env.openExternal(vscode.Uri.parse('https://github.com/vinhnx/vtcode#readme'));
    });

    const openDeepWiki = vscode.commands.registerCommand('vtcode.openDeepWiki', async () => {
        await vscode.env.openExternal(vscode.Uri.parse('https://deepwiki.com/vinhnx/vtcode'));
    });

    const openWalkthrough = vscode.commands.registerCommand('vtcode.openWalkthrough', async () => {
        await vscode.commands.executeCommand('workbench.action.openWalkthrough', 'vtcode.walkthrough');
    });

    const openInstallGuide = vscode.commands.registerCommand('vtcode.openInstallGuide', async () => {
        await vscode.env.openExternal(vscode.Uri.parse('https://github.com/vinhnx/vtcode#installation'));
    });

    const toggleHumanInTheLoopCommand = vscode.commands.registerCommand('vtcode.toggleHumanInTheLoop', async () => {
        try {
            const configUri = await pickVtcodeConfigUri(currentConfigSummary?.uri);
            if (!configUri) {
                void vscode.window.showWarningMessage('No vtcode.toml file was found in this workspace.');
                return;
            }

            const activeSummary =
                currentConfigSummary && currentConfigSummary.uri?.toString() === configUri.toString()
                    ? currentConfigSummary
                    : await loadConfigSummaryFromUri(configUri);

            const newValue = activeSummary.humanInTheLoop === false;
            const updated = await setHumanInTheLoop(configUri, newValue);
            if (!updated) {
                void vscode.window.showWarningMessage('Failed to update human_in_the_loop in vtcode.toml.');
                return;
            }

            const relativePath = vscode.workspace.asRelativePath(configUri, false);
            const channel = getOutputChannel();
            channel.appendLine(`[info] human_in_the_loop set to ${newValue} in ${relativePath}.`);
            void vscode.window.showInformationMessage(
                `Human-in-the-loop safeguards are now ${newValue ? 'enabled' : 'disabled'} in vtcode.toml.`
            );
        } catch (error) {
            handleCommandError('toggle human-in-the-loop mode', error);
        }
    });

    const openToolsPolicyGuideCommand = vscode.commands.registerCommand('vtcode.openToolsPolicyGuide', async () => {
        try {
            await openToolsPolicyGuide();
        } catch (error) {
            handleCommandError('open tool policy guide', error);
        }
    });

    const openToolsPolicyConfigCommand = vscode.commands.registerCommand('vtcode.openToolsPolicyConfig', async () => {
        try {
            const configUri = await pickVtcodeConfigUri(currentConfigSummary?.uri);
            if (!configUri) {
                void vscode.window.showWarningMessage('No vtcode.toml file was found in this workspace.');
                return;
            }

            await revealToolsPolicySection(configUri);
        } catch (error) {
            handleCommandError('open tool policy configuration', error);
        }
    });

    const configureMcpProvidersCommand = vscode.commands.registerCommand('vtcode.configureMcpProviders', async () => {
        try {
            const configUri = await pickVtcodeConfigUri(currentConfigSummary?.uri);
            if (!configUri) {
                void vscode.window.showWarningMessage('No vtcode.toml file was found in this workspace.');
                return;
            }

            const activeSummary =
                currentConfigSummary && currentConfigSummary.uri?.toString() === configUri.toString()
                    ? currentConfigSummary
                    : await loadConfigSummaryFromUri(configUri);

            const providers = activeSummary.mcpProviders;
            const enabledCount = providers.filter((provider) => provider.enabled !== false).length;

            const quickItems: Array<
                vscode.QuickPickItem & {
                    action: 'toggle' | 'add' | 'guide' | 'open';
                    providerName?: string;
                }
            > = providers.map((provider) => ({
                label: `${provider.enabled === false ? '$(circle-slash)' : '$(check)'} ${provider.name}`,
                description: provider.command ?? 'No command configured',
                detail:
                    provider.args && provider.args.length > 0
                        ? `Args: ${provider.args.join(' ')}`
                        : provider.enabled === false
                        ? 'Provider disabled'
                        : undefined,
                action: 'toggle',
                providerName: provider.name
            }));

            quickItems.push(
                {
                    label: '$(add) Add MCP provider',
                    description: 'Define a new Model Context Protocol provider entry.',
                    action: 'add'
                },
                {
                    label: '$(gear) Open MCP configuration',
                    description: 'Edit the MCP section in vtcode.toml.',
                    action: 'open'
                },
                {
                    label: '$(book) Open MCP integration guide',
                    description: 'Read the VTCode MCP configuration walkthrough.',
                    action: 'guide'
                }
            );

            const selection = await vscode.window.showQuickPick(quickItems, {
                placeHolder:
                    providers.length > 0
                        ? `Manage ${providers.length} MCP provider${providers.length === 1 ? '' : 's'} (${enabledCount} enabled)`
                        : 'No MCP providers defined. Add one to enable external tools.'
            });

            if (!selection) {
                return;
            }

            switch (selection.action) {
                case 'toggle': {
                    if (!selection.providerName) {
                        return;
                    }

                    const provider = providers.find((candidate) => candidate.name === selection.providerName);
                    if (!provider) {
                        void vscode.window.showWarningMessage(`Provider “${selection.providerName}” is no longer available.`);
                        return;
                    }

                    const newState = provider.enabled === false;
                    const result = await setMcpProviderEnabled(configUri, selection.providerName, newState);
                    if (result === 'notfound') {
                        void vscode.window.showWarningMessage(`Provider “${selection.providerName}” was not found in vtcode.toml.`);
                        return;
                    }

                    if (result === 'updated') {
                        const channel = getOutputChannel();
                        const relativePath = vscode.workspace.asRelativePath(configUri, false);
                        channel.appendLine(`[info] MCP provider "${selection.providerName}" enabled=${newState} in ${relativePath}.`);
                        void vscode.window.showInformationMessage(
                            `MCP provider “${selection.providerName}” is now ${newState ? 'enabled' : 'disabled'}.`
                        );
                    }
                    break;
                }
                case 'add': {
                    const name = await vscode.window.showInputBox({
                        prompt: 'Provider name',
                        ignoreFocusOut: true
                    });

                    if (!name || !name.trim()) {
                        return;
                    }

                    if (providers.some((provider) => provider.name.toLowerCase() === name.trim().toLowerCase())) {
                        void vscode.window.showWarningMessage(`An MCP provider named “${name.trim()}” already exists.`);
                        return;
                    }

                    const command = await vscode.window.showInputBox({
                        prompt: 'Command used to launch the provider',
                        value: 'uvx',
                        ignoreFocusOut: true
                    });

                    if (!command || !command.trim()) {
                        return;
                    }

                    const argsInput = await vscode.window.showInputBox({
                        prompt: 'Arguments (separate with spaces, leave blank for none)',
                        ignoreFocusOut: true
                    });

                    const args = argsInput
                        ? argsInput
                              .split(' ')
                              .map((value) => value.trim())
                              .filter((value) => value.length > 0)
                        : [];

                    const enableChoice = await vscode.window.showQuickPick(['Enable provider', 'Keep disabled'], {
                        placeHolder: 'Should the provider start enabled?'
                    });

                    if (!enableChoice) {
                        return;
                    }

                    const appended = await appendMcpProvider(configUri, {
                        name: name.trim(),
                        command: command.trim(),
                        args,
                        enabled: enableChoice === 'Enable provider'
                    });

                    if (appended) {
                        const channel = getOutputChannel();
                        const relativePath = vscode.workspace.asRelativePath(configUri, false);
                        channel.appendLine(`[info] Added MCP provider "${name.trim()}" to ${relativePath}.`);
                        void vscode.window.showInformationMessage(`Added MCP provider “${name.trim()}” to vtcode.toml.`);
                    } else {
                        void vscode.window.showWarningMessage(`Provider “${name.trim()}” already exists in vtcode.toml.`);
                    }
                    break;
                }
                case 'guide': {
                    await openMcpGuide();
                    break;
                }
                case 'open': {
                    await revealMcpSection(configUri);
                    break;
                }
            }
        } catch (error) {
            handleCommandError('configure MCP providers', error);
        }
    });

    const launchAgentTerminal = vscode.commands.registerCommand('vtcode.launchAgentTerminal', async () => {
        if (!(await ensureCliAvailableForCommand())) {
            return;
        }

        const cwd = getWorkspaceRoot();
        if (!cwd) {
            void vscode.window.showWarningMessage('Open a workspace folder before launching the VTCode agent terminal.');
            return;
        }

        const commandPath = getConfiguredCommandPath();
        const { terminal, created } = ensureAgentTerminal(commandPath, cwd);
        terminal.show(true);
        if (created) {
            const channel = getOutputChannel();
            channel.appendLine(`[info] Launching VTCode agent terminal with "${commandPath} chat" in ${cwd}.`);
        }
    });

    const runAnalyze = vscode.commands.registerCommand('vtcode.runAnalyze', async () => {
        if (!(await ensureCliAvailableForCommand())) {
            return;
        }

        try {
            await runVtcodeCommand(['analyze'], { title: 'Analyzing workspace with VTCode…' });
            void vscode.window.showInformationMessage('VTCode finished analyzing the workspace. Review the VTCode output channel for results.');
        } catch (error) {
            handleCommandError('analyze the workspace', error);
        }
    });

    const refreshQuickActions = vscode.commands.registerCommand('vtcode.refreshQuickActions', async () => {
        quickActionsProviderInstance?.refresh();
        await refreshCliAvailability('manual');
    });

    const configurationWatcher = vscode.workspace.onDidChangeConfiguration((event) => {
        if (event.affectsConfiguration('vtcode.commandPath')) {
            void refreshCliAvailability('configuration');
        }
    });

    context.subscriptions.push(
        quickActionsCommand,
        askAgent,
        askSelection,
        openConfig,
        openDocumentation,
        openDeepWiki,
        openWalkthrough,
        openInstallGuide,
        toggleHumanInTheLoopCommand,
        openToolsPolicyGuideCommand,
        openToolsPolicyConfigCommand,
        configureMcpProvidersCommand,
        launchAgentTerminal,
        runAnalyze,
        refreshQuickActions,
        configurationWatcher
    );

    void refreshCliAvailability('activation');
}

export function deactivate() {
    if (outputChannel) {
        outputChannel.dispose();
        outputChannel = undefined;
    }

    if (statusBarItem) {
        statusBarItem.dispose();
        statusBarItem = undefined;
    }

    if (agentTerminal) {
        agentTerminal.dispose();
        agentTerminal = undefined;
    }

    if (terminalCloseListener) {
        terminalCloseListener.dispose();
        terminalCloseListener = undefined;
    }

    quickActionsProviderInstance = undefined;
    cliAvailabilityCheck = undefined;
}

async function ensureCliAvailableForCommand(): Promise<boolean> {
    await refreshCliAvailability('manual');

    if (cliAvailable) {
        return true;
    }

    const commandPath = getConfiguredCommandPath();
    const selection = await vscode.window.showWarningMessage(
        `The VTCode CLI ("${commandPath}") is not available. Install the CLI or update the "vtcode.commandPath" setting to run this command.`,
        'Open Installation Guide'
    );

    if (selection === 'Open Installation Guide') {
        await vscode.commands.executeCommand('vtcode.openInstallGuide');
    }

    return false;
}

function createQuickActions(cliAvailableState: boolean, summary?: VtcodeConfigSummary): QuickActionDescription[] {
    const actions: QuickActionDescription[] = [];

    if (cliAvailableState) {
        actions.push(
            {
                label: 'Ask the VTCode agent…',
                description: 'Send a one-off question and stream the answer in VS Code.',
                command: 'vtcode.askAgent',
                icon: 'comment-discussion'
            },
            {
                label: 'Ask about highlighted selection',
                description: 'Right-click or trigger VTCode to explain the selected text.',
                command: 'vtcode.askSelection',
                icon: 'comment'
            },
            {
                label: 'Launch interactive VTCode terminal',
                description: 'Open an integrated terminal session running vtcode chat.',
                command: 'vtcode.launchAgentTerminal',
                icon: 'terminal'
            },
            {
                label: 'Analyze workspace with VTCode',
                description: 'Run vtcode analyze and stream the report to the VTCode output channel.',
                command: 'vtcode.runAnalyze',
                icon: 'pulse'
            }
        );
    } else {
        actions.push({
            label: 'Review VTCode CLI installation',
            description: 'Open the VTCode CLI installation instructions required for ask commands.',
            command: 'vtcode.openInstallGuide',
            icon: 'tools'
        });
    }

    if (summary?.hasConfig) {
        const hitlEnabled = summary.humanInTheLoop !== false;
        actions.push({
            label: hitlEnabled ? 'Disable human-in-the-loop safeguards' : 'Enable human-in-the-loop safeguards',
            description: hitlEnabled
                ? 'Allow VTCode to automate tool execution without manual approval.'
                : 'Require confirmation before VTCode executes high-impact tools.',
            command: 'vtcode.toggleHumanInTheLoop',
            icon: 'shield'
        });

        const providerCount = summary.mcpProviders.length;
        const enabledCount = summary.mcpProviders.filter((provider) => provider.enabled !== false).length;
        actions.push({
            label: providerCount > 0 ? 'Manage MCP providers' : 'Configure MCP providers',
            description:
                providerCount > 0
                    ? `Adjust ${enabledCount}/${providerCount} enabled Model Context Protocol providers.`
                    : 'Connect VTCode to external Model Context Protocol tools.',
            command: 'vtcode.configureMcpProviders',
            icon: 'plug'
        });

        const toolPoliciesCount = summary.toolPoliciesCount ?? 0;
        actions.push({
            label: 'Review tool policy configuration',
            description:
                toolPoliciesCount > 0
                    ? `Inspect ${toolPoliciesCount} explicit tool policy overrides.`
                    : 'Define allow/prompt/deny rules for VTCode tools.',
            command: 'vtcode.openToolsPolicyConfig',
            icon: 'law'
        });

        actions.push({
            label: 'Open VTCode tool policy guide',
            description: 'Read documentation covering VTCode tool governance and HITL flows.',
            command: 'vtcode.openToolsPolicyGuide',
            icon: 'book'
        });
    } else {
        actions.push({
            label: 'Open VTCode tool policy guide',
            description: 'Learn how VTCode enforces tool governance and human-in-the-loop safeguards.',
            command: 'vtcode.openToolsPolicyGuide',
            icon: 'book'
        });
    }

    const configDescription = summary?.uri
        ? `Open ${vscode.workspace.asRelativePath(summary.uri, false)} to adjust VTCode settings.`
        : 'Jump directly to the workspace VTCode configuration file.';

    actions.push(
        {
            label: 'Open vtcode.toml',
            description: configDescription,
            command: 'vtcode.openConfig',
            icon: 'gear'
        },
        {
            label: 'View VTCode documentation',
            description: 'Open the VTCode README in your browser.',
            command: 'vtcode.openDocumentation',
            icon: 'book'
        },
        {
            label: 'Review VTCode DeepWiki overview',
            description: 'Open the DeepWiki page for VTCode capabilities.',
            command: 'vtcode.openDeepWiki',
            icon: 'globe'
        },
        {
            label: 'Explore the VTCode walkthrough',
            description: 'Open the getting-started walkthrough to learn about VTCode features.',
            command: 'vtcode.openWalkthrough',
            icon: 'rocket'
        }
    );

    return actions;
}

function handleConfigUpdate(summary: VtcodeConfigSummary) {
    currentConfigSummary = summary;

    void vscode.commands.executeCommand('setContext', 'vtcode.configAvailable', summary.hasConfig);
    void vscode.commands.executeCommand('setContext', 'vtcode.hitlEnabled', summary.humanInTheLoop === true);
    void vscode.commands.executeCommand('setContext', 'vtcode.toolPoliciesConfigured', (summary.toolPoliciesCount ?? 0) > 0);
    void vscode.commands.executeCommand('setContext', 'vtcode.mcpConfigured', summary.mcpProviders.length > 0);
    void vscode.commands.executeCommand('setContext', 'vtcode.mcpEnabled', summary.mcpEnabled === true);

    const configUriString = summary.uri?.toString();
    if (summary.parseError && summary.parseError !== lastConfigParseError) {
        const channel = getOutputChannel();
        channel.appendLine(`[warn] Failed to parse vtcode.toml: ${summary.parseError}`);
    } else if (!summary.parseError && configUriString && configUriString !== lastConfigUri) {
        const channel = getOutputChannel();
        const label = summary.uri ? vscode.workspace.asRelativePath(summary.uri, false) : 'vtcode.toml';
        channel.appendLine(`[info] Using VTCode configuration from ${label}.`);
    }

    lastConfigUri = configUriString;
    lastConfigParseError = summary.parseError;

    updateStatusBarItem(getConfiguredCommandPath(), cliAvailable);
    quickActionsProviderInstance?.refresh();
}

async function refreshCliAvailability(trigger: 'activation' | 'configuration' | 'manual'): Promise<void> {
    if (cliAvailabilityCheck) {
        await cliAvailabilityCheck;
        return;
    }

    const commandPath = getConfiguredCommandPath();
    setStatusBarChecking(commandPath);

    cliAvailabilityCheck = (async () => {
        if (vscode.env.uiKind === vscode.UIKind.Web) {
            updateCliAvailabilityState(false, commandPath);
            return;
        }

        const available = await detectCliAvailability(commandPath);
        updateCliAvailabilityState(available, commandPath);

        if (!available && trigger === 'activation') {
            await maybeShowMissingCliWarning(commandPath);
        }
    })();

    try {
        await cliAvailabilityCheck;
    } finally {
        cliAvailabilityCheck = undefined;
    }
}

async function detectCliAvailability(commandPath: string): Promise<boolean> {
    if (!commandPath) {
        return false;
    }

    const cwd = getWorkspaceRoot();
    const spawnOptions = cwd ? createSpawnOptions({ cwd }) : createSpawnOptions();

    return new Promise((resolve) => {
        let resolved = false;

        const complete = (value: boolean) => {
            if (!resolved) {
                resolved = true;
                resolve(value);
            }
        };

        try {
            const child = spawn(commandPath, ['--version'], spawnOptions);

            const timer = setTimeout(() => {
                child.kill();
                complete(false);
            }, CLI_DETECTION_TIMEOUT_MS);

            child.on('error', () => {
                clearTimeout(timer);
                complete(false);
            });

            child.on('close', (code) => {
                clearTimeout(timer);
                complete(code === 0);
            });
        } catch (error) {
            complete(false);
        }
    });
}

function updateCliAvailabilityState(available: boolean, commandPath: string) {
    const previous = cliAvailable;
    cliAvailable = available;

    void vscode.commands.executeCommand('setContext', 'vtcode.cliAvailable', available);
    updateStatusBarItem(commandPath, available);

    if (available) {
        missingCliWarningShown = false;
    }

    if (previous !== available) {
        const channel = getOutputChannel();
        if (available) {
            channel.appendLine(`[info] Detected VTCode CLI using "${commandPath}".`);
        } else {
            channel.appendLine(`[warn] VTCode CLI not found using "${commandPath}".`);
        }
    }

    quickActionsProviderInstance?.refresh();
}

function setStatusBarChecking(commandPath: string) {
    if (!statusBarItem) {
        return;
    }

    statusBarItem.text = '$(sync~spin) VTCode';
    statusBarItem.tooltip = `Checking availability of "${commandPath}"`;
    statusBarItem.command = undefined;
    statusBarItem.backgroundColor = undefined;
    statusBarItem.color = undefined;
    statusBarItem.show();
}

function updateStatusBarItem(commandPath: string, available: boolean) {
    if (!statusBarItem) {
        return;
    }

    if (available) {
        const hitlEnabled = currentConfigSummary?.humanInTheLoop !== false;
        const suffix = currentConfigSummary?.hasConfig ? (hitlEnabled ? ' $(person)' : ' $(run-all)') : '';
        statusBarItem.text = `$(tools) VTCode Ready${suffix}`;
        statusBarItem.tooltip = createStatusBarTooltip(commandPath, true);
        statusBarItem.command = 'vtcode.openQuickActions';
        statusBarItem.backgroundColor = new vscode.ThemeColor('vtcode.statusBarBackground');
        statusBarItem.color = new vscode.ThemeColor('vtcode.statusBarForeground');
    } else {
        statusBarItem.text = '$(warning) VTCode CLI Missing';
        statusBarItem.tooltip = createStatusBarTooltip(commandPath, false);
        statusBarItem.command = 'vtcode.openInstallGuide';
        statusBarItem.backgroundColor = new vscode.ThemeColor('statusBarItem.warningBackground');
        statusBarItem.color = new vscode.ThemeColor('statusBarItem.warningForeground');
    }

    statusBarItem.show();
}

function createStatusBarTooltip(commandPath: string, available: boolean): vscode.MarkdownString {
    const tooltip = new vscode.MarkdownString(undefined, true);
    tooltip.appendMarkdown('**VTCode CLI**\n\n');
    tooltip.appendMarkdown(`• Path: \`${commandPath}\`\n`);
    tooltip.appendMarkdown(`• Status: ${available ? 'Available' : 'Missing'}\n`);

    if (currentConfigSummary?.hasConfig) {
        tooltip.appendMarkdown('\n**Configuration**\n');
        if (currentConfigSummary.uri) {
            const relative = vscode.workspace.asRelativePath(currentConfigSummary.uri, false);
            tooltip.appendMarkdown(`• File: \`${relative}\`\n`);
        }

        if (currentConfigSummary.humanInTheLoop !== undefined) {
            tooltip.appendMarkdown(
                `• Human-in-the-loop: ${currentConfigSummary.humanInTheLoop ? 'Enabled' : 'Disabled'}\n`
            );
        }

        if (currentConfigSummary.toolDefaultPolicy) {
            tooltip.appendMarkdown(`• Default tool policy: \`${currentConfigSummary.toolDefaultPolicy}\`\n`);
        }

        const providerCount = currentConfigSummary.mcpProviders.length;
        const enabledCount = currentConfigSummary.mcpProviders.filter((provider) => provider.enabled !== false).length;
        if (providerCount > 0) {
            tooltip.appendMarkdown(`• MCP providers: ${enabledCount}/${providerCount} enabled\n`);
        } else {
            tooltip.appendMarkdown('• MCP providers: none configured\n');
        }
    } else {
        tooltip.appendMarkdown('\nNo vtcode.toml detected in this workspace.\n');
    }

    tooltip.isTrusted = true;
    return tooltip;
}

async function maybeShowMissingCliWarning(commandPath: string): Promise<void> {
    if (missingCliWarningShown || vscode.env.uiKind === vscode.UIKind.Web) {
        return;
    }

    missingCliWarningShown = true;
    const selection = await vscode.window.showWarningMessage(
        `VTCode CLI was not found on PATH as "${commandPath}".`,
        'Open Installation Guide'
    );

    if (selection === 'Open Installation Guide') {
        await vscode.commands.executeCommand('vtcode.openInstallGuide');
    }
}

function getConfiguredCommandPath(): string {
    return vscode.workspace.getConfiguration('vtcode').get<string>('commandPath', 'vtcode').trim() || 'vtcode';
}

async function runVtcodeCommand(args: string[], options?: { title?: string }): Promise<void> {
    if (vscode.env.uiKind === vscode.UIKind.Web) {
        throw new Error('VTCode commands that spawn the CLI are not available in the web extension host.');
    }

    const commandPath = getConfiguredCommandPath();
    const cwd = getWorkspaceRoot();
    if (!cwd) {
        throw new Error('Open a workspace folder before running VTCode commands.');
    }

    const channel = getOutputChannel();
    const configArgs = getConfigArguments();
    const normalizedArgs = args.map((arg) => String(arg));
    const finalArgs = [...configArgs, ...normalizedArgs];
    const displayArgs = formatArgsForLogging(finalArgs);

    channel.show(true);
    channel.appendLine(`$ ${commandPath} ${displayArgs}`);

    await vscode.window.withProgress(
        {
            location: vscode.ProgressLocation.Notification,
            title: options?.title ?? 'Running VTCode…'
        },
        async () =>
            new Promise<void>((resolve, reject) => {
                const child = spawn(commandPath, finalArgs, createSpawnOptions({ cwd }));

                child.stdout.on('data', (data: Buffer) => {
                    channel.append(data.toString());
                });

                child.stderr.on('data', (data: Buffer) => {
                    channel.append(data.toString());
                });

                child.on('error', (error: Error) => {
                    reject(error);
                });

                child.on('close', (code) => {
                    if (code === 0) {
                        resolve();
                    } else {
                        reject(new Error(`VTCode exited with code ${code ?? 'unknown'}`));
                    }
                });
            })
    );
}

function getConfigArguments(): string[] {
    const uri = currentConfigSummary?.uri;
    if (!uri) {
        return [];
    }

    return ['--config', uri.fsPath];
}

function createSpawnOptions(
    overrides: Partial<SpawnOptionsWithoutStdio> = {}
): SpawnOptionsWithoutStdio {
    return {
        env: process.env,
        ...overrides
    };
}

function formatArgsForLogging(args: string[]): string {
    return args
        .map((arg) => {
            const value = String(arg);
            return /(\s|"|')/.test(value) ? JSON.stringify(value) : value;
        })
        .join(' ');
}

function formatArgsForShell(args: string[]): string {
    return args
        .map((arg) => {
            const value = String(arg);
            return quoteForShell(value);
        })
        .filter((value) => value.length > 0)
        .join(' ');
}

function quoteForShell(value: string): string {
    if (!/[\s"'\\$`]/.test(value)) {
        return value;
    }

    return `"${value.replace(/(["\\$`])/g, '\\$1')}"`;
}

function getWorkspaceRoot(): string | undefined {
    const activeEditor = vscode.window.activeTextEditor;
    if (activeEditor) {
        const workspaceFolder = vscode.workspace.getWorkspaceFolder(activeEditor.document.uri);
        if (workspaceFolder) {
            return workspaceFolder.uri.fsPath;
        }
    }

    const [firstWorkspace] = vscode.workspace.workspaceFolders ?? [];
    return firstWorkspace?.uri.fsPath;
}

function handleCommandError(contextLabel: string, error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    void vscode.window.showErrorMessage(`Failed to ${contextLabel} with VTCode: ${message}`);
}

function getOutputChannel(): vscode.OutputChannel {
    if (!outputChannel) {
        outputChannel = vscode.window.createOutputChannel('VTCode');
    }

    return outputChannel;
}

async function openToolsPolicyGuide(): Promise<void> {
    const [guide] = await vscode.workspace.findFiles('docs/vtcode_tools_policy.md', '**/{node_modules,dist,out,.git,target}/**', 1);
    if (guide) {
        const document = await vscode.workspace.openTextDocument(guide);
        await vscode.window.showTextDocument(document, { preview: false });
        return;
    }

    await vscode.env.openExternal(
        vscode.Uri.parse('https://github.com/vinhnx/vtcode/blob/main/docs/vtcode_tools_policy.md')
    );
}

async function openMcpGuide(): Promise<void> {
    const [guide] = await vscode.workspace.findFiles('docs/guides/mcp-integration.md', '**/{node_modules,dist,out,.git,target}/**', 1);
    if (guide) {
        const document = await vscode.workspace.openTextDocument(guide);
        await vscode.window.showTextDocument(document, { preview: false });
        return;
    }

    await vscode.env.openExternal(
        vscode.Uri.parse('https://github.com/vinhnx/vtcode/blob/main/docs/guides/mcp-integration.md')
    );
}

function ensureAgentTerminal(commandPath: string, cwd: string): { terminal: vscode.Terminal; created: boolean } {
    if (agentTerminal) {
        return { terminal: agentTerminal, created: false };
    }

    const terminal = vscode.window.createTerminal({
        name: 'VTCode Agent',
        cwd,
        env: process.env,
        iconPath: new vscode.ThemeIcon('comment-discussion')
    });
    const quotedCommandPath = /\s/.test(commandPath) ? `"${commandPath.replace(/"/g, '\\"')}"` : commandPath;
    const configArgs = getConfigArguments();
    const terminalArgs = ['chat', ...configArgs];
    const argsText = formatArgsForShell(terminalArgs);
    const commandText = argsText.length > 0 ? `${quotedCommandPath} ${argsText}` : quotedCommandPath;
    terminal.sendText(commandText, true);

    agentTerminal = terminal;

    if (!terminalCloseListener) {
        terminalCloseListener = vscode.window.onDidCloseTerminal((closed) => {
            if (closed === agentTerminal) {
                agentTerminal = undefined;
                terminalCloseListener?.dispose();
                terminalCloseListener = undefined;
            }
        });
    }

    return { terminal, created: true };
}

function ensureStableApi(context: vscode.ExtensionContext) {
    const manifest = context.extension.packageJSON as { enabledApiProposals?: string[] } | undefined;
    const proposals = manifest?.enabledApiProposals ?? [];

    if (proposals.length > 0) {
        const channel = getOutputChannel();
        channel.appendLine(`[warn] Proposed VS Code APIs enabled: ${proposals.join(', ')}.`);
    }
}

function logExtensionHostContext(context: vscode.ExtensionContext) {
    const channel = getOutputChannel();
    const remoteName = vscode.env.remoteName ? `remote (${vscode.env.remoteName})` : 'local';
    const hostKind = vscode.env.uiKind === vscode.UIKind.Web ? 'web' : 'desktop';
    const modeLabel = getExtensionModeLabel(context.extensionMode);
    channel.appendLine(`[info] VTCode Companion activated in ${remoteName} ${hostKind} host (${modeLabel} mode).`);
}

function getExtensionModeLabel(mode: vscode.ExtensionMode): string {
    switch (mode) {
        case vscode.ExtensionMode.Development:
            return 'development';
        case vscode.ExtensionMode.Test:
            return 'test';
        case vscode.ExtensionMode.Production:
        default:
            return 'production';
    }
}
