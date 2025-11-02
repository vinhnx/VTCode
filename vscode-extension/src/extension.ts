import { spawn, type SpawnOptionsWithoutStdio } from "node:child_process";
import * as vscode from "vscode";
import { registerVtcodeLanguageFeatures } from "./languageFeatures";
import {
    appendMcpProvider,
    loadConfigSummaryFromUri,
    pickVtcodeConfigUri,
    registerVtcodeConfigWatcher,
    revealMcpSection,
    revealToolsPolicySection,
    setHumanInTheLoop,
    setMcpProviderEnabled,
    VtcodeConfigSummary,
} from "./vtcodeConfig";

type QuickActionItem = vscode.QuickPickItem & {
    run: () => Thenable<unknown> | void;
};

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
        this.iconPath = new vscode.ThemeIcon(action.icon ?? "rocket");
        this.command = {
            command: action.command,
            title: action.label,
            arguments: action.args,
        };
        this.contextValue = "vtcodeQuickAction";
    }
}

class QuickActionTreeDataProvider
    implements vscode.TreeDataProvider<QuickActionTreeItem>
{
    private readonly onDidChangeTreeDataEmitter =
        new vscode.EventEmitter<void>();
    readonly onDidChangeTreeData = this.onDidChangeTreeDataEmitter.event;

    constructor(private readonly getActions: () => QuickActionDescription[]) {}

    getTreeItem(element: QuickActionTreeItem): vscode.TreeItem {
        return element;
    }

    getChildren(): vscode.ProviderResult<QuickActionTreeItem[]> {
        return this.getActions().map(
            (action) => new QuickActionTreeItem(action)
        );
    }

    refresh(): void {
        this.onDidChangeTreeDataEmitter.fire();
    }
}

interface WorkspaceInsightDescription {
    readonly label: string;
    readonly description: string;
    readonly icon: string;
    readonly command?: vscode.Command;
    readonly tooltip?: string | vscode.MarkdownString;
}

class WorkspaceInsightTreeItem extends vscode.TreeItem {
    constructor(public readonly insight: WorkspaceInsightDescription) {
        super(insight.label, vscode.TreeItemCollapsibleState.None);
        this.description = insight.description;
        this.iconPath = new vscode.ThemeIcon(insight.icon);
        this.command = insight.command;
        if (insight.tooltip) {
            this.tooltip = insight.tooltip;
        }
        this.contextValue = "vtcodeWorkspaceInsight";
    }
}

class WorkspaceInsightsTreeDataProvider
    implements vscode.TreeDataProvider<WorkspaceInsightTreeItem>
{
    private readonly onDidChangeTreeDataEmitter =
        new vscode.EventEmitter<void>();
    readonly onDidChangeTreeData = this.onDidChangeTreeDataEmitter.event;

    constructor(
        private readonly getInsights: () => WorkspaceInsightDescription[]
    ) {}

    getTreeItem(element: WorkspaceInsightTreeItem): vscode.TreeItem {
        return element;
    }

    getChildren(): vscode.ProviderResult<WorkspaceInsightTreeItem[]> {
        return this.getInsights().map(
            (insight) => new WorkspaceInsightTreeItem(insight)
        );
    }

    refresh(): void {
        this.onDidChangeTreeDataEmitter.fire();
    }
}

let outputChannel: vscode.OutputChannel | undefined;
let statusBarItem: vscode.StatusBarItem | undefined;
let quickActionsProviderInstance: QuickActionTreeDataProvider | undefined;
let workspaceInsightsProvider:
    | WorkspaceInsightsTreeDataProvider
    | undefined;
let agentTerminal: vscode.Terminal | undefined;
let terminalCloseListener: vscode.Disposable | undefined;
let cliAvailable = false;
let missingCliWarningShown = false;
let cliAvailabilityCheck: Promise<void> | undefined;
let currentConfigSummary: VtcodeConfigSummary | undefined;
let lastConfigUri: string | undefined;
let lastConfigParseError: string | undefined;
let workspaceTrusted = vscode.workspace.isTrusted;

const CLI_DETECTION_TIMEOUT_MS = 4000;
const VT_CODE_CHAT_PARTICIPANT_ID = "vtcode.agent";
const VT_CODE_UPDATE_PLAN_TOOL = "vtcode.updatePlan";
const VT_CODE_MCP_PROVIDER_ID = "vtcode.workspaceMcp";

const mcpDefinitionsChanged = new vscode.EventEmitter<void>();

let chatLaunchHintShown = false;

let ideContextBridge: IdeContextFileBridge | undefined;

const IDE_CONTEXT_ENV_VARIABLE = "VT_VSCODE_CONTEXT_FILE";
const IDE_CONTEXT_HEADER = "## VS Code Context";
const MAX_IDE_CONTEXT_CHARS = 6000;
const MAX_FULL_DOCUMENT_CONTEXT_LINES = 400;
const ACTIVE_EDITOR_CONTEXT_WINDOW = 80;
const MAX_VISIBLE_EDITOR_CONTEXTS = 3;

export function activate(context: vscode.ExtensionContext) {
    outputChannel = vscode.window.createOutputChannel("VTCode");
    context.subscriptions.push(outputChannel);

    ensureStableApi(context);
    logExtensionHostContext(context);
    registerVtcodeLanguageFeatures(context);

    void initializeIdeContextBridge(context);

    statusBarItem = vscode.window.createStatusBarItem(
        vscode.StatusBarAlignment.Left,
        100
    );
    statusBarItem.name = "VTCode Quick Actions";
    statusBarItem.accessibilityInformation = {
        role: "button",
        label: "Open VTCode quick actions or installation guide",
    };
    context.subscriptions.push(statusBarItem);

    workspaceInsightsProvider = new WorkspaceInsightsTreeDataProvider(() =>
        createWorkspaceInsights(
            workspaceTrusted,
            cliAvailable,
            currentConfigSummary
        )
    );
    context.subscriptions.push(
        vscode.window.registerTreeDataProvider(
            "vtcodeWorkspaceStatusView",
            workspaceInsightsProvider
        )
    );

    quickActionsProviderInstance = new QuickActionTreeDataProvider(() =>
        createQuickActions(
            cliAvailable,
            currentConfigSummary,
            workspaceTrusted
        )
    );
    context.subscriptions.push(
        vscode.window.registerTreeDataProvider(
            "vtcodeQuickActionsView",
            quickActionsProviderInstance
        )
    );

    initializeContextKeys();

    void registerVtcodeConfigWatcher(context, handleConfigUpdate);

    updateWorkspaceTrustState(workspaceTrusted);

    if (workspaceTrusted) {
        setStatusBarChecking(getConfiguredCommandPath());
    }

    context.subscriptions.push(
        vscode.workspace.onDidGrantWorkspaceTrust(async () => {
            updateWorkspaceTrustState(true);
            setStatusBarChecking(getConfiguredCommandPath());
            await refreshCliAvailability("manual");
        })
    );

    const workspaceFolderWatcher = vscode.workspace.onDidChangeWorkspaceFolders(
        () => {
            quickActionsProviderInstance?.refresh();
            workspaceInsightsProvider?.refresh();
            void refreshCliAvailability("manual");
        }
    );

    if (vscode.env.uiKind === vscode.UIKind.Web) {
        void vscode.window.showWarningMessage(
            "VTCode Companion is running in VS Code for the Web. Command execution features are disabled, but documentation and configuration helpers remain available."
        );
    }

    const quickActionsCommand = vscode.commands.registerCommand(
        "vtcode.openQuickActions",
        async () => {
            const actions = createQuickActions(
                cliAvailable,
                currentConfigSummary,
                workspaceTrusted
            );
            const pickItems: QuickActionItem[] = actions.map((action) => ({
                label: `${action.icon ? `$(${action.icon}) ` : ""}${
                    action.label
                }`,
                description: action.description,
                run: () =>
                    vscode.commands.executeCommand(
                        action.command,
                        ...(action.args ?? [])
                    ),
            }));

            const selection = await vscode.window.showQuickPick(pickItems, {
                placeHolder: "Choose a VTCode action to run",
            });

            if (selection) {
                await selection.run();
            }
        }
    );

    const askAgent = vscode.commands.registerCommand(
        "vtcode.askAgent",
        async () => {
            if (!(await ensureCliAvailableForCommand())) {
                return;
            }

            const question = await vscode.window.showInputBox({
                prompt: "What would you like the VTCode agent to help with?",
                placeHolder: "Summarize src/main.rs",
                ignoreFocusOut: true,
            });

            if (!question || !question.trim()) {
                return;
            }

            try {
                const promptWithContext = await appendIdeContextToPrompt(
                    question,
                    { includeActiveEditor: true }
                );

                await runVtcodeCommand(["ask", promptWithContext], {
                    title: "Asking VTCode…",
                });
                void vscode.window.showInformationMessage(
                    "VTCode finished processing your request. Check the VTCode output channel for details."
                );
            } catch (error) {
                handleCommandError("ask", error);
            }
        }
    );

    const askSelection = vscode.commands.registerCommand(
        "vtcode.askSelection",
        async () => {
            const editor = vscode.window.activeTextEditor;
            if (!editor) {
                void vscode.window.showWarningMessage(
                    "Open a text editor to ask VTCode about the current selection."
                );
                return;
            }

            const selection = editor.selection;
            if (selection.isEmpty) {
                void vscode.window.showWarningMessage(
                    "Highlight text first, then run “Ask About Selection with VTCode.”"
                );
                return;
            }

            const selectedText = editor.document.getText(selection);
            if (!selectedText.trim()) {
                void vscode.window.showWarningMessage(
                    "The selected text is empty. Select code or text for VTCode to inspect."
                );
                return;
            }

            if (!(await ensureCliAvailableForCommand())) {
                return;
            }

            const defaultQuestion = "Explain the highlighted selection.";
            const question = await vscode.window.showInputBox({
                prompt: "How should VTCode help with the highlighted selection?",
                value: defaultQuestion,
                valueSelection: [0, defaultQuestion.length],
                ignoreFocusOut: true,
            });

            if (question === undefined) {
                return;
            }

            const trimmedQuestion = question.trim() || defaultQuestion;
            const languageId = editor.document.languageId || "text";
            const rangeLabel = `${selection.start.line + 1}-${
                selection.end.line + 1
            }`;
            const workspaceFolder = vscode.workspace.getWorkspaceFolder(
                editor.document.uri
            );
            const relativePath = workspaceFolder
                ? vscode.workspace.asRelativePath(editor.document.uri, false)
                : editor.document.fileName;
            const normalizedSelection = selectedText.replace(/\r\n/g, "\n");
            const prompt = `${trimmedQuestion}\n\nFile: ${relativePath}\nLines: ${rangeLabel}\n\n\
\u0060\u0060\u0060${languageId}\n${normalizedSelection}\n\u0060\u0060\u0060`;

            try {
                await runVtcodeCommand(["ask", prompt], {
                    title: "Asking VTCode about the selection…",
                });
                void vscode.window.showInformationMessage(
                    "VTCode processed the highlighted selection. Review the output channel for the response."
                );
            } catch (error) {
                handleCommandError("ask about the selection", error);
            }
        }
    );

    const openConfig = vscode.commands.registerCommand(
        "vtcode.openConfig",
        async () => {
            try {
                const configUri = await pickVtcodeConfigUri(
                    currentConfigSummary?.uri
                );
                if (!configUri) {
                    void vscode.window.showWarningMessage(
                        "No vtcode.toml file was found in this workspace."
                    );
                    return;
                }

                const document = await vscode.workspace.openTextDocument(
                    configUri
                );
                await vscode.window.showTextDocument(document, {
                    preview: false,
                });
            } catch (error) {
                handleCommandError("open configuration", error);
            }
        }
    );

    const openDocumentation = vscode.commands.registerCommand(
        "vtcode.openDocumentation",
        async () => {
            await vscode.env.openExternal(
                vscode.Uri.parse("https://github.com/vinhnx/vtcode#readme")
            );
        }
    );

    const openDeepWiki = vscode.commands.registerCommand(
        "vtcode.openDeepWiki",
        async () => {
            await vscode.env.openExternal(
                vscode.Uri.parse("https://deepwiki.com/vinhnx/vtcode")
            );
        }
    );

    const openWalkthrough = vscode.commands.registerCommand(
        "vtcode.openWalkthrough",
        async () => {
            await vscode.commands.executeCommand(
                "workbench.action.openWalkthrough",
                "vtcode.walkthrough"
            );
        }
    );

    const openInstallGuide = vscode.commands.registerCommand(
        "vtcode.openInstallGuide",
        async () => {
            await vscode.env.openExternal(
                vscode.Uri.parse(
                    "https://github.com/vinhnx/vtcode#installation"
                )
            );
        }
    );

    const openChatCommand = vscode.commands.registerCommand(
        "vtcode.openChat",
        async () => {
            if (
                !(await ensureWorkspaceTrustedForCommand(
                    "chat with VTCode in VS Code"
                ))
            ) {
                return;
            }

            const chatCommands: Array<{ id: string; args?: unknown[] }> = [
                { id: "workbench.action.chat.open" },
                { id: "workbench.panel.chatSidebar.view.focus" },
                { id: "workbench.panel.chat.view.focus" },
            ];

            let opened = false;
            for (const entry of chatCommands) {
                try {
                    await vscode.commands.executeCommand(
                        entry.id,
                        ...(entry.args ?? [])
                    );
                    opened = true;
                    break;
                } catch (error) {
                    // Try the next known command if this one is unavailable.
                }
            }

            if (!opened) {
                void vscode.window.showInformationMessage(
                    "Open the Chat view and mention @vtcode.agent to start a conversation with VTCode."
                );
                return;
            }

            if (!chatLaunchHintShown) {
                chatLaunchHintShown = true;
                void vscode.window.showInformationMessage(
                    "Mention @vtcode.agent in the Chat view to start a conversation with VTCode."
                );
            }
        }
    );

    const toggleHumanInTheLoopCommand = vscode.commands.registerCommand(
        "vtcode.toggleHumanInTheLoop",
        async () => {
            if (
                !(await ensureWorkspaceTrustedForCommand(
                    "change VTCode human-in-the-loop settings"
                ))
            ) {
                return;
            }

            try {
                const configUri = await pickVtcodeConfigUri(
                    currentConfigSummary?.uri
                );
                if (!configUri) {
                    void vscode.window.showWarningMessage(
                        "No vtcode.toml file was found in this workspace."
                    );
                    return;
                }

                const activeSummary =
                    currentConfigSummary &&
                    currentConfigSummary.uri?.toString() ===
                        configUri.toString()
                        ? currentConfigSummary
                        : await loadConfigSummaryFromUri(configUri);

                const newValue = activeSummary.humanInTheLoop === false;
                const updated = await setHumanInTheLoop(configUri, newValue);
                if (!updated) {
                    void vscode.window.showWarningMessage(
                        "Failed to update human_in_the_loop in vtcode.toml."
                    );
                    return;
                }

                const relativePath = vscode.workspace.asRelativePath(
                    configUri,
                    false
                );
                const channel = getOutputChannel();
                channel.appendLine(
                    `[info] human_in_the_loop set to ${newValue} in ${relativePath}.`
                );
                void vscode.window.showInformationMessage(
                    `Human-in-the-loop safeguards are now ${
                        newValue ? "enabled" : "disabled"
                    } in vtcode.toml.`
                );
            } catch (error) {
                handleCommandError("toggle human-in-the-loop mode", error);
            }
        }
    );

    const openToolsPolicyGuideCommand = vscode.commands.registerCommand(
        "vtcode.openToolsPolicyGuide",
        async () => {
            try {
                await openToolsPolicyGuide();
            } catch (error) {
                handleCommandError("open tool policy guide", error);
            }
        }
    );

    const openToolsPolicyConfigCommand = vscode.commands.registerCommand(
        "vtcode.openToolsPolicyConfig",
        async () => {
            try {
                const configUri = await pickVtcodeConfigUri(
                    currentConfigSummary?.uri
                );
                if (!configUri) {
                    void vscode.window.showWarningMessage(
                        "No vtcode.toml file was found in this workspace."
                    );
                    return;
                }

                await revealToolsPolicySection(configUri);
            } catch (error) {
                handleCommandError("open tool policy configuration", error);
            }
        }
    );

    const configureMcpProvidersCommand = vscode.commands.registerCommand(
        "vtcode.configureMcpProviders",
        async () => {
            if (
                !(await ensureWorkspaceTrustedForCommand(
                    "edit VTCode MCP provider settings"
                ))
            ) {
                return;
            }

            try {
                const configUri = await pickVtcodeConfigUri(
                    currentConfigSummary?.uri
                );
                if (!configUri) {
                    void vscode.window.showWarningMessage(
                        "No vtcode.toml file was found in this workspace."
                    );
                    return;
                }

                const activeSummary =
                    currentConfigSummary &&
                    currentConfigSummary.uri?.toString() ===
                        configUri.toString()
                        ? currentConfigSummary
                        : await loadConfigSummaryFromUri(configUri);

                const providers = activeSummary.mcpProviders;
                const enabledCount = providers.filter(
                    (provider) => provider.enabled !== false
                ).length;

                const quickItems: Array<
                    vscode.QuickPickItem & {
                        action: "toggle" | "add" | "guide" | "open";
                        providerName?: string;
                    }
                > = providers.map((provider) => ({
                    label: `${
                        provider.enabled === false
                            ? "$(circle-slash)"
                            : "$(check)"
                    } ${provider.name}`,
                    description: provider.command ?? "No command configured",
                    detail:
                        provider.args && provider.args.length > 0
                            ? `Args: ${provider.args.join(" ")}`
                            : provider.enabled === false
                            ? "Provider disabled"
                            : undefined,
                    action: "toggle",
                    providerName: provider.name,
                }));

                quickItems.push(
                    {
                        label: "$(add) Add MCP provider",
                        description:
                            "Define a new Model Context Protocol provider entry.",
                        action: "add",
                    },
                    {
                        label: "$(gear) Open MCP configuration",
                        description: "Edit the MCP section in vtcode.toml.",
                        action: "open",
                    },
                    {
                        label: "$(book) Open MCP integration guide",
                        description:
                            "Read the VTCode MCP configuration walkthrough.",
                        action: "guide",
                    }
                );

                const selection = await vscode.window.showQuickPick(
                    quickItems,
                    {
                        placeHolder:
                            providers.length > 0
                                ? `Manage ${providers.length} MCP provider${
                                      providers.length === 1 ? "" : "s"
                                  } (${enabledCount} enabled)`
                                : "No MCP providers defined. Add one to enable external tools.",
                    }
                );

                if (!selection) {
                    return;
                }

                switch (selection.action) {
                    case "toggle": {
                        if (!selection.providerName) {
                            return;
                        }

                        const provider = providers.find(
                            (candidate) =>
                                candidate.name === selection.providerName
                        );
                        if (!provider) {
                            void vscode.window.showWarningMessage(
                                `Provider “${selection.providerName}” is no longer available.`
                            );
                            return;
                        }

                        const newState = provider.enabled === false;
                        const result = await setMcpProviderEnabled(
                            configUri,
                            selection.providerName,
                            newState
                        );
                        if (result === "notfound") {
                            void vscode.window.showWarningMessage(
                                `Provider “${selection.providerName}” was not found in vtcode.toml.`
                            );
                            return;
                        }

                        if (result === "updated") {
                            const channel = getOutputChannel();
                            const relativePath =
                                vscode.workspace.asRelativePath(
                                    configUri,
                                    false
                                );
                            channel.appendLine(
                                `[info] MCP provider "${selection.providerName}" enabled=${newState} in ${relativePath}.`
                            );
                            void vscode.window.showInformationMessage(
                                `MCP provider “${
                                    selection.providerName
                                }” is now ${newState ? "enabled" : "disabled"}.`
                            );
                        }
                        break;
                    }
                    case "add": {
                        const name = await vscode.window.showInputBox({
                            prompt: "Provider name",
                            ignoreFocusOut: true,
                        });

                        if (!name || !name.trim()) {
                            return;
                        }

                        if (
                            providers.some(
                                (provider) =>
                                    provider.name.toLowerCase() ===
                                    name.trim().toLowerCase()
                            )
                        ) {
                            void vscode.window.showWarningMessage(
                                `An MCP provider named “${name.trim()}” already exists.`
                            );
                            return;
                        }

                        const command = await vscode.window.showInputBox({
                            prompt: "Command used to launch the provider",
                            value: "uvx",
                            ignoreFocusOut: true,
                        });

                        if (!command || !command.trim()) {
                            return;
                        }

                        const argsInput = await vscode.window.showInputBox({
                            prompt: "Arguments (separate with spaces, leave blank for none)",
                            ignoreFocusOut: true,
                        });

                        const args = argsInput
                            ? argsInput
                                  .split(" ")
                                  .map((value) => value.trim())
                                  .filter((value) => value.length > 0)
                            : [];

                        const enableChoice = await vscode.window.showQuickPick(
                            ["Enable provider", "Keep disabled"],
                            {
                                placeHolder:
                                    "Should the provider start enabled?",
                            }
                        );

                        if (!enableChoice) {
                            return;
                        }

                        const appended = await appendMcpProvider(configUri, {
                            name: name.trim(),
                            command: command.trim(),
                            args,
                            enabled: enableChoice === "Enable provider",
                        });

                        if (appended) {
                            const channel = getOutputChannel();
                            const relativePath =
                                vscode.workspace.asRelativePath(
                                    configUri,
                                    false
                                );
                            channel.appendLine(
                                `[info] Added MCP provider "${name.trim()}" to ${relativePath}.`
                            );
                            void vscode.window.showInformationMessage(
                                `Added MCP provider “${name.trim()}” to vtcode.toml.`
                            );
                        } else {
                            void vscode.window.showWarningMessage(
                                `Provider “${name.trim()}” already exists in vtcode.toml.`
                            );
                        }
                        break;
                    }
                    case "guide": {
                        await openMcpGuide();
                        break;
                    }
                    case "open": {
                        await revealMcpSection(configUri);
                        break;
                    }
                }
            } catch (error) {
                handleCommandError("configure MCP providers", error);
            }
        }
    );

    const launchAgentTerminal = vscode.commands.registerCommand(
        "vtcode.launchAgentTerminal",
        async () => {
            if (!(await ensureCliAvailableForCommand())) {
                return;
            }

            const cwd = getWorkspaceRoot();
            if (!cwd) {
                void vscode.window.showWarningMessage(
                    "Open a workspace folder before launching the VTCode agent terminal."
                );
                return;
            }

            const commandPath = getConfiguredCommandPath();
            const { terminal, created } = ensureAgentTerminal(commandPath, cwd);
            terminal.show(true);
            if (created) {
                const channel = getOutputChannel();
                channel.appendLine(
                    `[info] Launching VTCode agent terminal with "${commandPath} chat" in ${cwd}.`
                );
            }
        }
    );

    const runAnalyze = vscode.commands.registerCommand(
        "vtcode.runAnalyze",
        async () => {
            if (!(await ensureCliAvailableForCommand())) {
                return;
            }

            try {
                await runVtcodeCommand(["analyze"], {
                    title: "Analyzing workspace with VTCode…",
                });
                void vscode.window.showInformationMessage(
                    "VTCode finished analyzing the workspace. Review the VTCode output channel for results."
                );
            } catch (error) {
                handleCommandError("analyze the workspace", error);
            }
        }
    );

    const runUpdatePlanTaskCommand = vscode.commands.registerCommand(
        "vtcode.runUpdatePlanTask",
        async () => {
            if (
                !(await ensureWorkspaceTrustedForCommand(
                    "update the VTCode task plan"
                ))
            ) {
                return;
            }

            if (!(await ensureCliAvailableForCommand())) {
                return;
            }

            const tasks = await vscode.tasks.fetchTasks({ type: "vtcode" });
            const updatePlanTasks = tasks.filter(
                (task) =>
                    (task.definition as VtcodeTaskDefinition).command ===
                    "update-plan"
            );

            if (updatePlanTasks.length === 0) {
                void vscode.window.showWarningMessage(
                    "No VTCode update plan tasks are available. Define a VTCode task in tasks.json to customize the workflow."
                );
                return;
            }

            let taskToRun: vscode.Task | undefined;
            if (updatePlanTasks.length === 1) {
                taskToRun = updatePlanTasks[0];
            } else {
                const pickItems = updatePlanTasks.map((task) => ({
                    label: task.name,
                    task,
                }));
                const selection = await vscode.window.showQuickPick(pickItems, {
                    placeHolder: "Select the VTCode plan task to run",
                });
                taskToRun = selection?.task;
            }

            if (!taskToRun) {
                return;
            }

            if (ideContextBridge) {
                await ideContextBridge.flush();
            }

            await vscode.tasks.executeTask(taskToRun);
        }
    );

    const refreshQuickActions = vscode.commands.registerCommand(
        "vtcode.refreshQuickActions",
        async () => {
            quickActionsProviderInstance?.refresh();
            await refreshCliAvailability("manual");
        }
    );

    const taskProvider = vscode.tasks.registerTaskProvider("vtcode", {
        provideTasks: provideVtcodeTasks,
        resolveTask: resolveVtcodeTask,
    });

    const configurationWatcher = vscode.workspace.onDidChangeConfiguration(
        (event) => {
            if (event.affectsConfiguration("vtcode.commandPath")) {
                void refreshCliAvailability("configuration");
            }
        }
    );

    context.subscriptions.push(
        quickActionsCommand,
        askAgent,
        askSelection,
        openConfig,
        openDocumentation,
        openDeepWiki,
        openWalkthrough,
        openInstallGuide,
        openChatCommand,
        toggleHumanInTheLoopCommand,
        openToolsPolicyGuideCommand,
        openToolsPolicyConfigCommand,
        configureMcpProvidersCommand,
        launchAgentTerminal,
        runAnalyze,
        runUpdatePlanTaskCommand,
        refreshQuickActions,
        taskProvider,
        configurationWatcher,
        workspaceFolderWatcher
    );

    registerVtcodeAiIntegrations(context);

    void refreshCliAvailability("activation");
}

async function initializeIdeContextBridge(
    context: vscode.ExtensionContext
): Promise<void> {
    const storageRoot = context.globalStorageUri ?? context.storageUri;
    if (!storageRoot || storageRoot.scheme !== "file") {
        return;
    }

    try {
        await vscode.workspace.fs.createDirectory(storageRoot);
    } catch (error) {
        const message = error instanceof Error ? error.message : String(error);
        getOutputChannel().appendLine(
            `[warn] Failed to prepare IDE context storage: ${message}`
        );
    }

    const fileUri = vscode.Uri.joinPath(storageRoot, "vtcode-ide-context.md");
    const bridge = new IdeContextFileBridge(fileUri);
    ideContextBridge = bridge;
    context.subscriptions.push(bridge);

    await bridge.flush();

    const scheduleRefresh = () => ideContextBridge?.scheduleRefresh();
    context.subscriptions.push(
        vscode.window.onDidChangeActiveTextEditor(() => scheduleRefresh()),
        vscode.window.onDidChangeVisibleTextEditors(() => scheduleRefresh()),
        vscode.window.onDidChangeTextEditorSelection(() => scheduleRefresh()),
        vscode.workspace.onDidChangeTextDocument((event) => {
            if (isDocumentVisible(event.document)) {
                scheduleRefresh();
            }
        }),
        vscode.workspace.onDidSaveTextDocument((document) => {
            if (isDocumentVisible(document)) {
                scheduleRefresh();
            }
        }),
        vscode.workspace.onDidCloseTextDocument(() => scheduleRefresh()),
        vscode.workspace.onDidChangeWorkspaceFolders(() => scheduleRefresh())
    );
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
    if (
        !(await ensureWorkspaceTrustedForCommand("run VTCode commands"))
    ) {
        return false;
    }

    await refreshCliAvailability("manual");

    if (cliAvailable) {
        return true;
    }

    const commandPath = getConfiguredCommandPath();
    const selection = await vscode.window.showWarningMessage(
        `The VTCode CLI ("${commandPath}") is not available. Install the CLI or update the "vtcode.commandPath" setting to run this command.`,
        "Open Installation Guide"
    );

    if (selection === "Open Installation Guide") {
        await vscode.commands.executeCommand("vtcode.openInstallGuide");
    }

    return false;
}

function createQuickActions(
    cliAvailableState: boolean,
    summary: VtcodeConfigSummary | undefined,
    trusted: boolean
): QuickActionDescription[] {
    const actions: QuickActionDescription[] = [];

    if (!trusted) {
        actions.push(
            {
                label: "Trust this workspace for VTCode",
                description:
                    "Grant workspace trust to enable VTCode automation and CLI access.",
                command: "workbench.action.manageTrust",
                icon: "shield",
            },
            {
                label: "Review VTCode CLI requirements",
                description:
                    "Learn how the VTCode CLI integrates once the workspace is trusted.",
                command: "vtcode.openInstallGuide",
                icon: "tools",
            }
        );
    }

    if (trusted && cliAvailableState) {
        actions.push(
            {
                label: "Ask the VTCode agent…",
                description:
                    "Send a one-off question and stream the answer in VS Code.",
                command: "vtcode.askAgent",
                icon: "comment-discussion",
            },
            {
                label: "Open a VTCode chat session",
                description:
                    "Launch the VS Code Chat view and collaborate with the VTCode participant.",
                command: "vtcode.openChat",
                icon: "comment-quote",
            },
            {
                label: "Ask about highlighted selection",
                description:
                    "Right-click or trigger VTCode to explain the selected text.",
                command: "vtcode.askSelection",
                icon: "comment",
            },
            {
                label: "Update VTCode task plan",
                description:
                    "Run the predefined VS Code task that drives the update_plan tool.",
                command: "vtcode.runUpdatePlanTask",
                icon: "checklist",
            },
            {
                label: "Launch interactive VTCode terminal",
                description:
                    "Open an integrated terminal session running vtcode chat.",
                command: "vtcode.launchAgentTerminal",
                icon: "terminal",
            },
            {
                label: "Analyze workspace with VTCode",
                description:
                    "Run vtcode analyze and stream the report to the VTCode output channel.",
                command: "vtcode.runAnalyze",
                icon: "pulse",
            }
        );
    } else if (trusted) {
        actions.push({
            label: "Review VTCode CLI installation",
            description:
                "Open the VTCode CLI installation instructions required for automation.",
            command: "vtcode.openInstallGuide",
            icon: "tools",
        });
    }

    if (trusted && summary?.hasConfig) {
        const hitlEnabled = summary.humanInTheLoop !== false;
        actions.push({
            label: hitlEnabled
                ? "Disable human-in-the-loop safeguards"
                : "Enable human-in-the-loop safeguards",
            description: hitlEnabled
                ? "Allow VTCode to automate tool execution without manual approval."
                : "Require confirmation before VTCode executes high-impact tools.",
            command: "vtcode.toggleHumanInTheLoop",
            icon: "shield",
        });

        const providerCount = summary.mcpProviders.length;
        const enabledCount = summary.mcpProviders.filter(
            (provider) => provider.enabled !== false
        ).length;
        actions.push({
            label:
                providerCount > 0
                    ? "Manage MCP providers"
                    : "Configure MCP providers",
            description:
                providerCount > 0
                    ? `Adjust ${enabledCount}/${providerCount} enabled Model Context Protocol providers.`
                    : "Connect VTCode to external Model Context Protocol tools.",
            command: "vtcode.configureMcpProviders",
            icon: "plug",
        });

        const toolPoliciesCount = summary.toolPoliciesCount ?? 0;
        actions.push({
            label: "Review tool policy configuration",
            description:
                toolPoliciesCount > 0
                    ? `Inspect ${toolPoliciesCount} explicit tool policy overrides.`
                    : "Define allow/prompt/deny rules for VTCode tools.",
            command: "vtcode.openToolsPolicyConfig",
            icon: "law",
        });
    }

    const toolGuideDescription = summary?.hasConfig && trusted
        ? "Read documentation covering VTCode tool governance and HITL flows."
        : "Learn how VTCode enforces tool governance and human-in-the-loop safeguards.";
    actions.push({
        label: "Open VTCode tool policy guide",
        description: toolGuideDescription,
        command: "vtcode.openToolsPolicyGuide",
        icon: "book",
    });

    const configDescription = summary?.uri
        ? `Open ${vscode.workspace.asRelativePath(
              summary.uri,
              false
          )} to adjust VTCode settings.`
        : "Jump directly to the workspace VTCode configuration file.";

    actions.push(
        {
            label: "Open vtcode.toml",
            description: configDescription,
            command: "vtcode.openConfig",
            icon: "gear",
        },
        {
            label: "View VTCode documentation",
            description: "Open the VTCode README in your browser.",
            command: "vtcode.openDocumentation",
            icon: "book",
        },
        {
            label: "Review VTCode DeepWiki overview",
            description: "Open the DeepWiki page for VTCode capabilities.",
            command: "vtcode.openDeepWiki",
            icon: "globe",
        },
        {
            label: "Explore the VTCode walkthrough",
            description:
                "Open the getting-started walkthrough to learn about VTCode features.",
            command: "vtcode.openWalkthrough",
            icon: "rocket",
        }
    );

    return actions;
}

function createWorkspaceInsights(
    trusted: boolean,
    cliAvailableState: boolean,
    summary: VtcodeConfigSummary | undefined
): WorkspaceInsightDescription[] {
    const insights: WorkspaceInsightDescription[] = [];

    insights.push({
        label: trusted
            ? "Workspace trust granted"
            : "Workspace trust required",
        description: trusted
            ? "VTCode can run CLI automation in this workspace."
            : "Grant trust to enable VTCode CLI commands and automation features.",
        icon: trusted ? "shield" : "shield-off",
        command: trusted
            ? undefined
            : {
                  command: "workbench.action.manageTrust",
                  title: "Manage Workspace Trust",
              },
        tooltip: trusted
            ? "Workspace trust allows VTCode to spawn CLI processes."
            : "Security-sensitive features are disabled until this workspace is trusted.",
    });

    if (!trusted) {
        insights.push({
            label: "CLI access blocked",
            description:
                "Trust the workspace to allow VTCode to detect and launch the CLI.",
            icon: "circle-slash",
            command: {
                command: "vtcode.openInstallGuide",
                title: "Review CLI Installation",
            },
        });
    } else {
        const commandPath = getConfiguredCommandPath();
        insights.push({
            label: cliAvailableState
                ? "VTCode CLI detected"
                : "VTCode CLI unavailable",
            description: cliAvailableState
                ? `Using ${commandPath}`
                : `Check ${commandPath} or adjust vtcode.commandPath`,
            icon: cliAvailableState ? "check" : "warning",
            command: cliAvailableState
                ? { command: "vtcode.openQuickActions", title: "Open Quick Actions" }
                : { command: "vtcode.openInstallGuide", title: "Open Installation Guide" },
            tooltip: createStatusBarTooltip(
                commandPath,
                cliAvailableState,
                trusted
            ),
        });
    }

    if (summary?.hasConfig) {
        const configPath = summary.uri
            ? vscode.workspace.asRelativePath(summary.uri, false)
            : "vtcode.toml";
        insights.push({
            label: "VTCode configuration detected",
            description: configPath,
            icon: "gear",
            command: { command: "vtcode.openConfig", title: "Open vtcode.toml" },
        });

        const hitlStatus = summary.humanInTheLoop === false
            ? "Disabled (manual approvals required)"
            : "Enabled";
        insights.push({
            label: "Human-in-the-loop safeguards",
            description: hitlStatus,
            icon: summary.humanInTheLoop === false ? "person" : "shield",
            command:
                trusted && summary.uri
                    ? {
                          command: "vtcode.toggleHumanInTheLoop",
                          title: "Toggle human-in-the-loop safeguards",
                      }
                    : undefined,
        });

        const providerCount = summary.mcpProviders.length;
        const enabledCount = summary.mcpProviders.filter(
            (provider) => provider.enabled !== false
        ).length;
        insights.push({
            label: "MCP providers",
            description:
                providerCount > 0
                    ? `${enabledCount}/${providerCount} enabled`
                    : "No providers configured",
            icon: "plug",
            command:
                trusted && summary.uri
                    ? {
                          command: "vtcode.configureMcpProviders",
                          title: "Configure MCP providers",
                      }
                    : undefined,
        });

        const toolPoliciesCount = summary.toolPoliciesCount ?? 0;
        const toolPolicyLabel = summary.toolDefaultPolicy
            ? `Default: ${summary.toolDefaultPolicy}`
            : "No default policy set";
        insights.push({
            label: "Tool policy coverage",
            description:
                toolPoliciesCount > 0
                    ? `${toolPoliciesCount} overrides · ${toolPolicyLabel}`
                    : `No overrides · ${toolPolicyLabel}`,
            icon: "law",
            command: {
                command: "vtcode.openToolsPolicyConfig",
                title: "Review tool policy configuration",
            },
        });

        if (summary.parseError) {
            insights.push({
                label: "Configuration parsing error",
                description: summary.parseError,
                icon: "error",
                command: summary.uri
                    ? { command: "vtcode.openConfig", title: "Open vtcode.toml" }
                    : undefined,
            });
        }
    } else {
        insights.push({
            label: "No vtcode.toml detected",
            description:
                "Use VTCode: Open Configuration to create a workspace configuration.",
            icon: "file",
            command: { command: "vtcode.openConfig", title: "Create vtcode.toml" },
        });
    }

    return insights;
}

function handleConfigUpdate(summary: VtcodeConfigSummary) {
    currentConfigSummary = summary;

    void vscode.commands.executeCommand(
        "setContext",
        "vtcode.configAvailable",
        summary.hasConfig
    );
    void vscode.commands.executeCommand(
        "setContext",
        "vtcode.hitlEnabled",
        summary.humanInTheLoop === true
    );
    void vscode.commands.executeCommand(
        "setContext",
        "vtcode.toolPoliciesConfigured",
        (summary.toolPoliciesCount ?? 0) > 0
    );
    void vscode.commands.executeCommand(
        "setContext",
        "vtcode.mcpConfigured",
        summary.mcpProviders.length > 0
    );
    void vscode.commands.executeCommand(
        "setContext",
        "vtcode.mcpEnabled",
        summary.mcpEnabled === true
    );

    const configUriString = summary.uri?.toString();
    if (summary.parseError && summary.parseError !== lastConfigParseError) {
        const channel = getOutputChannel();
        channel.appendLine(
            `[warn] Failed to parse vtcode.toml: ${summary.parseError}`
        );
    } else if (
        !summary.parseError &&
        configUriString &&
        configUriString !== lastConfigUri
    ) {
        const channel = getOutputChannel();
        const label = summary.uri
            ? vscode.workspace.asRelativePath(summary.uri, false)
            : "vtcode.toml";
        channel.appendLine(`[info] Using VTCode configuration from ${label}.`);
    }

    lastConfigUri = configUriString;
    lastConfigParseError = summary.parseError;

    updateStatusBarItem(getConfiguredCommandPath(), cliAvailable);
    quickActionsProviderInstance?.refresh();
    workspaceInsightsProvider?.refresh();
    mcpDefinitionsChanged.fire();
}

async function refreshCliAvailability(
    trigger: "activation" | "configuration" | "manual"
): Promise<void> {
    if (cliAvailabilityCheck) {
        await cliAvailabilityCheck;
        return;
    }

    const commandPath = getConfiguredCommandPath();

    if (!workspaceTrusted) {
        updateCliAvailabilityState(false, commandPath, "untrusted");
        return;
    }

    setStatusBarChecking(commandPath);

    cliAvailabilityCheck = (async () => {
        if (vscode.env.uiKind === vscode.UIKind.Web) {
            updateCliAvailabilityState(false, commandPath);
            return;
        }

        const available = await detectCliAvailability(commandPath);
        updateCliAvailabilityState(available, commandPath);

        if (!available && trigger === "activation" && workspaceTrusted) {
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
    const spawnOptions = cwd
        ? createSpawnOptions({ cwd })
        : createSpawnOptions();

    return new Promise((resolve) => {
        let resolved = false;

        const complete = (value: boolean) => {
            if (!resolved) {
                resolved = true;
                resolve(value);
            }
        };

        try {
            const child = spawn(commandPath, ["--version"], spawnOptions);

            const timer = setTimeout(() => {
                child.kill();
                complete(false);
            }, CLI_DETECTION_TIMEOUT_MS);

            child.on("error", () => {
                clearTimeout(timer);
                complete(false);
            });

            child.on("close", (code) => {
                clearTimeout(timer);
                complete(code === 0);
            });
        } catch (error) {
            complete(false);
        }
    });
}

function updateCliAvailabilityState(
    available: boolean,
    commandPath: string,
    reason?: "untrusted"
) {
    const normalizedAvailable = available && workspaceTrusted;
    const previous = cliAvailable;
    cliAvailable = normalizedAvailable;

    void vscode.commands.executeCommand(
        "setContext",
        "vtcode.cliAvailable",
        normalizedAvailable && workspaceTrusted
    );
    updateStatusBarItem(commandPath, normalizedAvailable);

    if (normalizedAvailable) {
        missingCliWarningShown = false;
    }

    if (previous !== normalizedAvailable) {
        const channel = getOutputChannel();
        if (!workspaceTrusted && reason === "untrusted") {
            channel.appendLine(
                "[info] VTCode CLI checks are paused until the workspace is trusted."
            );
        } else if (normalizedAvailable) {
            channel.appendLine(
                `[info] Detected VTCode CLI using "${commandPath}".`
            );
        } else {
            channel.appendLine(
                `[warn] VTCode CLI not found using "${commandPath}".`
            );
        }
    }

    quickActionsProviderInstance?.refresh();
    workspaceInsightsProvider?.refresh();
}

function setStatusBarChecking(commandPath: string) {
    if (!statusBarItem) {
        return;
    }

    if (!workspaceTrusted) {
        updateStatusBarItem(commandPath, false);
        return;
    }

    statusBarItem.text = "$(sync~spin) VTCode";
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

    if (!workspaceTrusted) {
        statusBarItem.text = "$(shield) Trust VTCode Workspace";
        statusBarItem.tooltip = createStatusBarTooltip(
            commandPath,
            available,
            false
        );
        statusBarItem.command = "workbench.action.manageTrust";
        statusBarItem.backgroundColor = new vscode.ThemeColor(
            "statusBarItem.prominentBackground"
        );
        statusBarItem.color = new vscode.ThemeColor(
            "statusBarItem.prominentForeground"
        );
        statusBarItem.show();
        return;
    }

    if (available) {
        const hitlEnabled = currentConfigSummary?.humanInTheLoop !== false;
        const suffix = currentConfigSummary?.hasConfig
            ? hitlEnabled
                ? " $(person)"
                : " $(run-all)"
            : "";
        statusBarItem.text = `$(chevron-right) VTCode${suffix}`; // Using chevron-right icon as requested

        statusBarItem.tooltip = createStatusBarTooltip(
            commandPath,
            true,
            true
        );
        statusBarItem.command = "vtcode.openQuickActions";
        statusBarItem.backgroundColor = new vscode.ThemeColor(
            "vtcode.statusBarBackground"
        );
        statusBarItem.color = new vscode.ThemeColor(
            "vtcode.statusBarForeground"
        );
    } else {
        statusBarItem.text = "$(warning) VTCode CLI Missing";
        statusBarItem.tooltip = createStatusBarTooltip(
            commandPath,
            false,
            true
        );
        statusBarItem.command = "vtcode.openInstallGuide";
        statusBarItem.backgroundColor = new vscode.ThemeColor(
            "statusBarItem.warningBackground"
        );
        statusBarItem.color = new vscode.ThemeColor(
            "statusBarItem.warningForeground"
        );
    }

    statusBarItem.show();
}

function createStatusBarTooltip(
    commandPath: string,
    available: boolean,
    trusted: boolean
): vscode.MarkdownString {
    const tooltip = new vscode.MarkdownString(undefined, true);
    tooltip.appendMarkdown("**VTCode Workspace**\n\n");
    tooltip.appendMarkdown(
        `• Workspace trust: ${trusted ? "Trusted" : "Restricted"}\n`
    );

    tooltip.appendMarkdown("\n**VTCode CLI**\n\n");
    tooltip.appendMarkdown(`• Path: \`${commandPath}\`\n`);
    const cliStatus = !trusted
        ? "Blocked by workspace trust"
        : available
        ? "Available"
        : "Missing";
    tooltip.appendMarkdown(`• Status: ${cliStatus}\n`);

    if (!trusted) {
        tooltip.appendMarkdown(
            "\nGrant workspace trust to enable VTCode CLI automation.\n"
        );
        tooltip.isTrusted = true;
        return tooltip;
    }

    if (currentConfigSummary?.hasConfig) {
        tooltip.appendMarkdown("\n**Configuration**\n");
        if (currentConfigSummary.uri) {
            const relative = vscode.workspace.asRelativePath(
                currentConfigSummary.uri,
                false
            );
            tooltip.appendMarkdown(`• File: \`${relative}\`\n`);
        }

        if (currentConfigSummary.humanInTheLoop !== undefined) {
            tooltip.appendMarkdown(
                `• Human-in-the-loop: ${
                    currentConfigSummary.humanInTheLoop ? "Enabled" : "Disabled"
                }\n`
            );
        }

        if (currentConfigSummary.toolDefaultPolicy) {
            tooltip.appendMarkdown(
                `• Default tool policy: \`${currentConfigSummary.toolDefaultPolicy}\`\n`
            );
        }

        const providerCount = currentConfigSummary.mcpProviders.length;
        const enabledCount = currentConfigSummary.mcpProviders.filter(
            (provider) => provider.enabled !== false
        ).length;
        if (providerCount > 0) {
            tooltip.appendMarkdown(
                `• MCP providers: ${enabledCount}/${providerCount} enabled\n`
            );
        } else {
            tooltip.appendMarkdown("• MCP providers: none configured\n");
        }
    } else {
        tooltip.appendMarkdown(
            "\nNo vtcode.toml detected in this workspace.\n"
        );
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
        "Open Installation Guide"
    );

    if (selection === "Open Installation Guide") {
        await vscode.commands.executeCommand("vtcode.openInstallGuide");
    }
}

function updateWorkspaceTrustState(trusted: boolean): void {
    workspaceTrusted = trusted;
    void vscode.commands.executeCommand(
        "setContext",
        "vtcode.workspaceTrusted",
        trusted
    );

    const commandPath = getConfiguredCommandPath();
    updateCliAvailabilityState(
        cliAvailable,
        commandPath,
        trusted ? undefined : "untrusted"
    );
    mcpDefinitionsChanged.fire();
}

function initializeContextKeys(): void {
    const contextDefaults: Array<[string, boolean]> = [
        ["vtcode.workspaceTrusted", workspaceTrusted],
        ["vtcode.cliAvailable", false],
        ["vtcode.configAvailable", false],
        ["vtcode.hitlEnabled", false],
        ["vtcode.toolPoliciesConfigured", false],
        ["vtcode.mcpConfigured", false],
        ["vtcode.mcpEnabled", false],
    ];

    for (const [key, value] of contextDefaults) {
        void vscode.commands.executeCommand("setContext", key, value);
    }
}

async function ensureWorkspaceTrustedForCommand(
    action: string
): Promise<boolean> {
    if (workspaceTrusted) {
        return true;
    }

    const selection = await vscode.window.showWarningMessage(
        `VTCode requires a trusted workspace to ${action}.`,
        "Manage Workspace Trust"
    );

    if (selection === "Manage Workspace Trust") {
        await vscode.commands.executeCommand("workbench.action.manageTrust");
    }

    return false;
}

function getConfiguredCommandPath(): string {
    return (
        vscode.workspace
            .getConfiguration("vtcode")
            .get<string>("commandPath", "vtcode")
            .trim() || "vtcode"
    );
}

interface RunVtcodeCommandOptions {
    readonly title?: string;
    readonly revealOutput?: boolean;
    readonly showProgress?: boolean;
    readonly onStdout?: (text: string) => void;
    readonly onStderr?: (text: string) => void;
    readonly cancellationToken?: vscode.CancellationToken;
}

interface UpdatePlanToolInput {
    readonly summary?: string;
    readonly steps?: string[];
}

async function runVtcodeCommand(
    args: string[],
    options: RunVtcodeCommandOptions = {}
): Promise<void> {
    if (vscode.env.uiKind === vscode.UIKind.Web) {
        throw new Error(
            "VTCode commands that spawn the CLI are not available in the web extension host."
        );
    }

    if (!workspaceTrusted) {
        throw new Error(
            "Trust this workspace to run VTCode CLI commands from VS Code."
        );
    }

    const commandPath = getConfiguredCommandPath();
    const cwd = getWorkspaceRoot();
    if (!cwd) {
        throw new Error(
            "Open a workspace folder before running VTCode commands."
        );
    }

    if (ideContextBridge) {
        await ideContextBridge.flush();
    }

    if (options.cancellationToken?.isCancellationRequested) {
        throw new vscode.CancellationError();
    }

    const channel = getOutputChannel();
    const configArgs = getConfigArguments();
    const normalizedArgs = args.map((arg) => String(arg));
    const finalArgs = [...configArgs, ...normalizedArgs];
    const displayArgs = formatArgsForLogging(finalArgs);
    const revealOutput = options.revealOutput ?? true;

    if (revealOutput) {
        channel.show(true);
    }
    channel.appendLine(`$ ${commandPath} ${displayArgs}`);

    const runCommand = async () =>
        new Promise<void>((resolve, reject) => {
            const child = spawn(
                commandPath,
                finalArgs,
                createSpawnOptions({ cwd })
            );

            let cancellationRegistration: vscode.Disposable | undefined;
            let cancelled = false;
            if (options.cancellationToken) {
                cancellationRegistration = options.cancellationToken.onCancellationRequested(
                    () => {
                        cancelled = true;
                        if (!child.killed) {
                            child.kill();
                        }
                    }
                );
            }

            child.stdout.on("data", (data: Buffer) => {
                const text = data.toString();
                channel.append(text);
                options.onStdout?.(text);
            });

            child.stderr.on("data", (data: Buffer) => {
                const text = data.toString();
                channel.append(text);
                options.onStderr?.(text);
            });

            child.on("error", (error: Error) => {
                cancellationRegistration?.dispose();
                reject(error);
            });

            child.on("close", (code) => {
                cancellationRegistration?.dispose();
                if (cancelled) {
                    reject(new vscode.CancellationError());
                    return;
                }

                if (code === 0) {
                    resolve();
                } else {
                    reject(
                        new Error(
                            `VTCode exited with code ${code ?? "unknown"}`
                        )
                    );
                }
            });
        });

    if (options.showProgress === false) {
        await runCommand();
        return;
    }

    await vscode.window.withProgress(
        {
            location: vscode.ProgressLocation.Notification,
            title: options.title ?? "Running VTCode…",
        },
        runCommand
    );
}

function getConfigArguments(): string[] {
    const uri = currentConfigSummary?.uri;
    if (!uri) {
        return [];
    }

    return ["--config", uri.fsPath];
}

interface VtcodeTaskDefinition extends vscode.TaskDefinition {
    type: "vtcode";
    command: "update-plan";
    summary?: string;
    steps?: string[];
    label?: string;
}

async function provideVtcodeTasks(): Promise<vscode.Task[]> {
    if (vscode.env.uiKind === vscode.UIKind.Web) {
        return [];
    }

    if (!workspaceTrusted) {
        return [];
    }

    const folder = getPrimaryWorkspaceFolder();
    if (!folder) {
        return [];
    }

    if (ideContextBridge) {
        await ideContextBridge.flush();
    }

    const definition: VtcodeTaskDefinition = {
        type: "vtcode",
        command: "update-plan",
        label: "Update plan with VTCode",
    };

    return [createUpdatePlanTask(folder, definition)];
}

function resolveVtcodeTask(task: vscode.Task): vscode.Task | undefined {
    const definition = task.definition as VtcodeTaskDefinition;
    if (definition.command !== "update-plan") {
        return undefined;
    }

    const scope = task.scope;
    let folder: vscode.WorkspaceFolder | undefined;
    if (scope && typeof scope !== "number") {
        folder = scope as vscode.WorkspaceFolder;
    } else {
        folder = getPrimaryWorkspaceFolder();
    }

    if (!folder) {
        return undefined;
    }

    ideContextBridge?.scheduleRefresh();

    return createUpdatePlanTask(folder, definition);
}

function createUpdatePlanTask(
    folder: vscode.WorkspaceFolder,
    definition: VtcodeTaskDefinition
): vscode.Task {
    const resolvedDefinition: VtcodeTaskDefinition = {
        type: "vtcode",
        command: "update-plan",
        summary: definition.summary,
        steps: definition.steps,
        label: definition.label,
    };

    const label = definition.label ?? "Update plan with VTCode";
    const prompt = buildUpdatePlanPrompt(resolvedDefinition);
    const args = [...getConfigArguments(), "exec", prompt];

    const execution = new vscode.ProcessExecution(
        getConfiguredCommandPath(),
        args,
        {
            cwd: folder.uri.fsPath,
            env: getVtcodeEnvironment(),
        }
    );

    const task = new vscode.Task(
        resolvedDefinition,
        folder,
        label,
        "VTCode",
        execution
    );
    task.detail =
        "Runs `vtcode exec` to synchronize the workspace TODO plan via the update_plan tool.";
    task.presentationOptions = {
        reveal: vscode.TaskRevealKind.Always,
        echo: true,
        focus: false,
        panel: vscode.TaskPanelKind.Shared,
    };

    return task;
}

function buildUpdatePlanPrompt(definition: VtcodeTaskDefinition): string {
    const summary =
        definition.summary?.trim() ??
        "Refresh the current TODO plan using the VTCode update_plan tool.";
    const steps = (definition.steps ?? [])
        .map((step) => step.trim())
        .filter((step) => step.length > 0);

    const lines = [
        summary,
        "Use the update_plan tool to synchronize tasks and mark step status accurately.",
    ];

    if (steps.length > 0) {
        lines.push("", "Plan inputs:");
        steps.forEach((step, index) => {
            lines.push(`${index + 1}. ${step}`);
        });
    }

    return lines.join("\n");
}

function registerVtcodeAiIntegrations(
    context: vscode.ExtensionContext
): void {
    context.subscriptions.push(mcpDefinitionsChanged);

    if ("lm" in vscode && typeof vscode.lm?.registerTool === "function") {
        const toolDisposable = vscode.lm.registerTool<UpdatePlanToolInput>(
            VT_CODE_UPDATE_PLAN_TOOL,
            {
                prepareInvocation: async (options) => {
                    const summary = options.input.summary?.trim();
                    const invocationMessage = summary
                        ? `Updating VTCode plan: ${summary}`
                        : "Updating VTCode plan with vtcode exec.";
                    return { invocationMessage };
                },
                invoke: async (options, token) => {
                    if (vscode.env.uiKind === vscode.UIKind.Web) {
                        throw new Error(
                            "VTCode CLI commands are not available in VS Code for the Web."
                        );
                    }

                    if (!workspaceTrusted) {
                        throw new Error(
                            "Trust this workspace to allow VTCode to update the TODO plan."
                        );
                    }

                    await refreshCliAvailability("manual");
                    if (!cliAvailable) {
                        const commandPath = getConfiguredCommandPath();
                        throw new Error(
                            `The VTCode CLI ("${commandPath}") is not available. Install the CLI or update the vtcode.commandPath setting.`
                        );
                    }

                    const input = options.input ?? {};
                    const summary =
                        typeof input.summary === "string"
                            ? input.summary.trim()
                            : undefined;
                    const steps = Array.isArray(input.steps)
                        ? input.steps
                              .map((value) => String(value).trim())
                              .filter((value) => value.length > 0)
                        : undefined;

                    const definition: VtcodeTaskDefinition = {
                        type: "vtcode",
                        command: "update-plan",
                        summary,
                        steps,
                    };
                    const prompt = buildUpdatePlanPrompt(definition);
                    const outputChunks: string[] = [];

                    try {
                        await runVtcodeCommand([
                            "exec",
                            prompt,
                        ], {
                            title: "Updating VTCode plan…",
                            revealOutput: false,
                            showProgress: false,
                            cancellationToken: token,
                            onStdout: (text) => outputChunks.push(text),
                            onStderr: (text) => outputChunks.push(text),
                        });
                    } catch (error) {
                        if (error instanceof vscode.CancellationError) {
                            throw error;
                        }

                        throw error;
                    }

                    const combined = outputChunks.join("");
                    const normalized = combined.replace(/\r\n/g, "\n").trim();
                    const content =
                        normalized.length > 0
                            ? `\`\`\`\n${normalized}\n\`\`\``
                            : "VTCode completed the update_plan request but did not emit any output.";

                    return [new vscode.LanguageModelTextPart(content)];
                },
            }
        );
        context.subscriptions.push(toolDisposable);
    }

    if (
        "lm" in vscode &&
        typeof vscode.lm?.registerMcpServerDefinitionProvider === "function"
    ) {
        const mcpProvider: vscode.McpServerDefinitionProvider<
            vscode.McpStdioServerDefinition
        > = {
            onDidChangeMcpServerDefinitions: mcpDefinitionsChanged.event,
            provideMcpServerDefinitions: async () => {
                if (!workspaceTrusted) {
                    return [];
                }

                const providers = currentConfigSummary?.mcpProviders ?? [];
                return providers
                    .filter((provider) => provider.command)
                    .map(
                        (provider) =>
                            new vscode.McpStdioServerDefinition(
                                provider.name,
                                provider.command ?? "",
                                provider.args ?? []
                            )
                    );
            },
            resolveMcpServerDefinition: async (server) => {
                if (!workspaceTrusted) {
                    throw new Error(
                        "Trust this workspace before starting VTCode MCP servers."
                    );
                }

                return server;
            },
        };

        const disposable = vscode.lm.registerMcpServerDefinitionProvider(
            VT_CODE_MCP_PROVIDER_ID,
            mcpProvider
        );
        context.subscriptions.push(disposable);
    }

    if (
        "chat" in vscode &&
        typeof vscode.chat?.createChatParticipant === "function"
    ) {
        const participant = vscode.chat.createChatParticipant(
            VT_CODE_CHAT_PARTICIPANT_ID,
            async (request, _context, response, token) => {
                const basePrompt = request.prompt.trim();
                if (!basePrompt) {
                    response.markdown(
                        "Ask a question or describe a task for the VTCode agent to begin."
                    );
                    return;
                }

                if (vscode.env.uiKind === vscode.UIKind.Web) {
                    const message =
                        "The VTCode CLI is not available in VS Code for the Web. Open a desktop workspace to chat with the CLI-driven agent.";
                    response.markdown(message);
                    return {
                        errorDetails: { message },
                    };
                }

                if (!workspaceTrusted) {
                    const message =
                        "Trust this workspace to allow VTCode to run CLI commands from chat.";
                    response.markdown(message);
                    return {
                        errorDetails: { message },
                    };
                }

                await refreshCliAvailability("manual");
                if (!cliAvailable) {
                    const commandPath = getConfiguredCommandPath();
                    const message = `The VTCode CLI (\`${commandPath}\`) is not available. Install it or update the \`vtcode.commandPath\` setting.`;
                    response.markdown(message);
                    return {
                        errorDetails: { message },
                    };
                }

                const promptWithContext = await appendIdeContextToPrompt(
                    basePrompt,
                    {
                        includeActiveEditor: true,
                        chatRequest: request,
                        cancellationToken: token,
                    }
                );

                response.progress("Running `vtcode ask`…");

                const collected: string[] = [];
                try {
                    await runVtcodeCommand([
                        "ask",
                        promptWithContext,
                    ], {
                        title: "Asking VTCode…",
                        revealOutput: false,
                        showProgress: false,
                        cancellationToken: token,
                        onStdout: (text) => collected.push(text),
                        onStderr: (text) => collected.push(text),
                    });
                } catch (error) {
                    if (error instanceof vscode.CancellationError) {
                        response.progress("VTCode chat request cancelled.");
                        return;
                    }

                    const message =
                        error instanceof Error ? error.message : String(error);
                    response.markdown(
                        `VTCode encountered an error while running the CLI: ${message}`
                    );
                    return {
                        errorDetails: { message },
                    };
                }

                const combined = collected.join("");
                const normalized = combined.replace(/\r\n/g, "\n").trim();
                if (normalized.length > 0) {
                    response.markdown(`\`\`\`\n${normalized}\n\`\`\``);
                } else {
                    response.markdown(
                        "VTCode completed the request but did not emit any output."
                    );
                }

                return {
                    metadata: {
                        command: "ask",
                    },
                };
            }
        );
        participant.iconPath = new vscode.ThemeIcon("rocket");
        participant.followupProvider = {
            provideFollowups: async () => [
                {
                    prompt: "Summarize the current TODO plan.",
                    label: "Summarize TODO plan",
                },
                {
                    prompt:
                        "Review configured MCP providers and highlight anything that needs attention.",
                    label: "Audit MCP providers",
                },
                {
                    prompt:
                        "Suggest the next high-priority tasks VTCode should tackle in this workspace.",
                    label: "Suggest next tasks",
                },
            ],
        };
        context.subscriptions.push(participant);
    }
}

interface AppendIdeContextOptions {
    readonly includeActiveEditor?: boolean;
    readonly includeVisibleEditors?: boolean;
    readonly chatRequest?: vscode.ChatRequest;
    readonly cancellationToken?: vscode.CancellationToken;
}

async function appendIdeContextToPrompt(
    prompt: string,
    options: AppendIdeContextOptions = {}
): Promise<string> {
    const contextBlock = await buildIdeContextBlock(options);
    if (!contextBlock) {
        return prompt;
    }

    const trimmedPrompt = prompt.trimEnd();
    const basePrompt = trimmedPrompt.length > 0 ? trimmedPrompt : prompt;

    if (basePrompt.length === 0) {
        return contextBlock;
    }

    return `${basePrompt}\n\n${contextBlock}`;
}

async function buildIdeContextBlock(
    options: AppendIdeContextOptions = {}
): Promise<string | undefined> {
    const sections = await collectIdeContextSections(options);
    if (sections.length === 0) {
        return undefined;
    }

    return [IDE_CONTEXT_HEADER, ...sections].join("\n\n");
}

async function collectIdeContextSections(
    options: AppendIdeContextOptions = {}
): Promise<string[]> {
    const sections: string[] = [];
    const seenKeys = new Set<string>();
    const token = options.cancellationToken;

    if (token?.isCancellationRequested) {
        throw new vscode.CancellationError();
    }

    if (options.includeActiveEditor !== false) {
        const activeSection = await buildActiveEditorContextSection(
            seenKeys,
            token
        );
        if (activeSection) {
            sections.push(activeSection);
        }
    }

    if (options.includeVisibleEditors) {
        const visibleSections = await buildVisibleEditorContextSections(
            seenKeys,
            token
        );
        if (visibleSections.length > 0) {
            sections.push(...visibleSections);
        }
    }

    if (options.chatRequest) {
        const referenceSections = await buildReferenceContextSections(
            options.chatRequest,
            seenKeys,
            token
        );
        if (referenceSections.length > 0) {
            sections.push(...referenceSections);
        }
    }

    return sections;
}

async function buildActiveEditorContextSection(
    seen: Set<string>,
    token?: vscode.CancellationToken
): Promise<string | undefined> {
    const editor = vscode.window.activeTextEditor;
    if (!editor) {
        return undefined;
    }

    if (token?.isCancellationRequested) {
        throw new vscode.CancellationError();
    }

    const document = editor.document;
    const preferredRange = computeActiveEditorRange(editor);
    const context = extractDocumentContext(document, preferredRange);
    if (!context) {
        return undefined;
    }

    const key = createContextKey(document.uri, context.range, "active-editor");
    if (!registerContextKey(seen, key)) {
        return undefined;
    }

    const label = getPathLabel(document.uri);
    const detailParts: string[] = [];
    if (context.range) {
        detailParts.push(`lines ${formatRangeLabel(context.range)}`);
    }
    if (document.isDirty) {
        detailParts.push("unsaved changes");
    }
    if (context.truncated) {
        detailParts.push("truncated");
    }

    const headingDetails = detailParts.length > 0 ? ` (${detailParts.join(" • ")})` : "";
    const heading = `### Active Editor: ${label}${headingDetails}`;
    const codeBlock = formatCodeBlock(document.languageId, context.text);
    const notes = context.truncated
        ? "_Context truncated to fit VS Code chat limits._"
        : undefined;

    return [heading, codeBlock, notes].filter(Boolean).join("\n\n");
}

async function buildVisibleEditorContextSections(
    seen: Set<string>,
    token?: vscode.CancellationToken
): Promise<string[]> {
    const sections: string[] = [];
    const activeEditor = vscode.window.activeTextEditor;
    const activeUri = activeEditor?.document.uri.toString();

    for (const editor of vscode.window.visibleTextEditors) {
        if (sections.length >= MAX_VISIBLE_EDITOR_CONTEXTS) {
            break;
        }

        if (token?.isCancellationRequested) {
            throw new vscode.CancellationError();
        }

        const document = editor.document;
        if (document.uri.toString() === activeUri) {
            continue;
        }

        const context = extractDocumentContext(document, undefined);
        if (!context) {
            continue;
        }

        const key = createContextKey(document.uri, context.range, "visible-editor");
        if (!registerContextKey(seen, key)) {
            continue;
        }

        const label = getPathLabel(document.uri);
        const detailParts: string[] = [];
        if (context.range) {
            detailParts.push(`lines ${formatRangeLabel(context.range)}`);
        }
        if (document.isDirty) {
            detailParts.push("unsaved changes");
        }
        if (context.truncated) {
            detailParts.push("truncated");
        }

        const headingDetails = detailParts.length > 0 ? ` (${detailParts.join(" • ")})` : "";
        const heading = `### Editor: ${label}${headingDetails}`;
        const codeBlock = formatCodeBlock(document.languageId, context.text);
        const notes = context.truncated
            ? "_Context truncated to fit VS Code chat limits._"
            : undefined;

        sections.push([heading, codeBlock, notes].filter(Boolean).join("\n\n"));
    }

    return sections;
}

async function buildReferenceContextSections(
    request: vscode.ChatRequest,
    seen: Set<string>,
    token?: vscode.CancellationToken
): Promise<string[]> {
    const sections: string[] = [];

    for (const reference of request.references ?? []) {
        if (token?.isCancellationRequested) {
            throw new vscode.CancellationError();
        }

        const section = await buildReferenceContextSection(
            reference,
            seen,
            token
        );
        if (section) {
            sections.push(section);
        }
    }

    return sections;
}

async function buildReferenceContextSection(
    reference: vscode.ChatPromptReference,
    seen: Set<string>,
    token?: vscode.CancellationToken
): Promise<string | undefined> {
    const value = reference.value;

    if (typeof value === "string") {
        const trimmed = value.trim();
        if (!trimmed) {
            return undefined;
        }

        const key = createContextKey(undefined, undefined, `string:${trimmed}`);
        if (!registerContextKey(seen, key)) {
            return undefined;
        }

        const description = reference.modelDescription?.trim();
        const headingLabel = description && description.length > 0
            ? description
            : `Reference ${reference.id}`;
        const heading = `### Reference: ${headingLabel}`;
        const block = formatCodeBlock("text", trimmed);
        return `${heading}\n\n${block}`;
    }

    if (value instanceof vscode.Location) {
        const document = await vscode.workspace.openTextDocument(value.uri);
        if (token?.isCancellationRequested) {
            throw new vscode.CancellationError();
        }

        const context = extractDocumentContext(document, value.range);
        if (!context) {
            return undefined;
        }

        const key = createContextKey(value.uri, context.range, reference.id);
        if (!registerContextKey(seen, key)) {
            return undefined;
        }

        const label = getPathLabel(value.uri);
        const description = reference.modelDescription?.trim();
        const headingLabel = description && description.length > 0 ? description : label;
        const details: string[] = [`lines ${formatRangeLabel(context.range)}`];
        if (context.truncated) {
            details.push("truncated");
        }
        const detailText = details.length > 0 ? ` (${details.join(" • ")})` : "";
        const heading = `### Reference: ${headingLabel}${detailText}`;
        const block = formatCodeBlock(document.languageId, context.text);
        const notes = context.truncated
            ? "_Context truncated to fit VS Code chat limits._"
            : undefined;
        return [heading, block, notes].filter(Boolean).join("\n\n");
    }

    if (value instanceof vscode.Uri) {
        const document = await vscode.workspace.openTextDocument(value);
        if (token?.isCancellationRequested) {
            throw new vscode.CancellationError();
        }

        const context = extractDocumentContext(document, undefined);
        if (!context) {
            return undefined;
        }

        const key = createContextKey(value, context.range, reference.id);
        if (!registerContextKey(seen, key)) {
            return undefined;
        }

        const label = getPathLabel(value);
        const description = reference.modelDescription?.trim();
        const headingLabel = description && description.length > 0 ? description : label;
        const details: string[] = [];
        if (context.range) {
            details.push(`lines ${formatRangeLabel(context.range)}`);
        }
        if (context.truncated) {
            details.push("truncated");
        }
        const detailText = details.length > 0 ? ` (${details.join(" • ")})` : "";
        const heading = `### Reference: ${headingLabel}${detailText}`;
        const block = formatCodeBlock(document.languageId, context.text);
        const notes = context.truncated
            ? "_Context truncated to fit VS Code chat limits._"
            : undefined;
        return [heading, block, notes].filter(Boolean).join("\n\n");
    }

    return undefined;
}

function computeActiveEditorRange(
    editor: vscode.TextEditor
): vscode.Range | undefined {
    const document = editor.document;
    if (!editor.selection.isEmpty) {
        return editor.selection;
    }

    const visibleRanges = editor.visibleRanges.filter((range) => !range.isEmpty);
    if (visibleRanges.length > 0) {
        const first = visibleRanges[0];
        const last = visibleRanges[visibleRanges.length - 1];
        return new vscode.Range(first.start, last.end);
    }

    if (document.lineCount === 0) {
        return undefined;
    }

    if (document.lineCount <= MAX_FULL_DOCUMENT_CONTEXT_LINES) {
        const lastLineIndex = Math.max(0, document.lineCount - 1);
        const endPosition = document.lineAt(lastLineIndex).range.end;
        return new vscode.Range(new vscode.Position(0, 0), endPosition);
    }

    const activeLine = editor.selection.active.line;
    const halfWindow = Math.max(1, Math.floor(ACTIVE_EDITOR_CONTEXT_WINDOW / 2));
    const startLine = Math.max(0, activeLine - halfWindow);
    const endLine = Math.min(document.lineCount - 1, activeLine + halfWindow);
    const endPosition = document.lineAt(endLine).range.end;
    return new vscode.Range(new vscode.Position(startLine, 0), endPosition);
}

interface DocumentContext {
    readonly text: string;
    readonly range: vscode.Range;
    readonly truncated: boolean;
}

function extractDocumentContext(
    document: vscode.TextDocument,
    range: vscode.Range | undefined
): DocumentContext | undefined {
    if (document.lineCount === 0) {
        return undefined;
    }

    let targetRange = range;
    let truncated = false;

    if (!targetRange) {
        const totalLines = document.lineCount;
        const endLineIndex = Math.min(totalLines, MAX_FULL_DOCUMENT_CONTEXT_LINES) - 1;
        const endPosition = document.lineAt(Math.max(0, endLineIndex)).range.end;
        targetRange = new vscode.Range(new vscode.Position(0, 0), endPosition);
        if (totalLines > MAX_FULL_DOCUMENT_CONTEXT_LINES) {
            truncated = true;
        }
    }

    const rawText = document.getText(targetRange);
    const normalized = normalizeForPrompt(rawText);
    if (!normalized.trim()) {
        return undefined;
    }

    const limited = truncateForPrompt(normalized, MAX_IDE_CONTEXT_CHARS);
    return {
        text: limited.text,
        range: targetRange,
        truncated: truncated || limited.truncated,
    };
}

function formatCodeBlock(languageId: string | undefined, content: string): string {
    const language = languageId && languageId.trim().length > 0 ? languageId : "text";
    return `\`\`\`${language}\n${content}\n\`\`\``;
}

function getPathLabel(uri: vscode.Uri): string {
    if (uri.scheme === "untitled") {
        const segments = uri.path.split("/");
        const name = segments[segments.length - 1] || "untitled";
        return `untitled:${name}`;
    }

    const relative = vscode.workspace.asRelativePath(uri, false);
    if (relative && relative !== uri.toString()) {
        return relative;
    }

    if (uri.scheme === "file") {
        return uri.fsPath;
    }

    return uri.toString(true);
}

function formatRangeLabel(range: vscode.Range): string {
    const startLine = range.start.line + 1;
    const endLine = range.end.line + 1;
    return startLine === endLine ? `${startLine}` : `${startLine}-${endLine}`;
}

function createContextKey(
    uri: vscode.Uri | undefined,
    range: vscode.Range | undefined,
    fallback: string
): string {
    const base = uri ? uri.toString() : fallback;
    if (range) {
        return `${base}:${range.start.line}:${range.start.character}-${range.end.line}:${range.end.character}`;
    }
    return base;
}

function registerContextKey(seen: Set<string>, key: string): boolean {
    if (seen.has(key)) {
        return false;
    }
    seen.add(key);
    return true;
}

function normalizeForPrompt(text: string): string {
    return text.replace(/\r\n/g, "\n");
}

function truncateForPrompt(
    text: string,
    limit: number
): { text: string; truncated: boolean } {
    if (text.length <= limit) {
        return { text, truncated: false };
    }

    return { text: text.slice(0, limit), truncated: true };
}

function getIdeContextFilePath(): string | undefined {
    return ideContextBridge?.filePath;
}

function isDocumentVisible(document: vscode.TextDocument): boolean {
    if (vscode.window.activeTextEditor?.document === document) {
        return true;
    }

    return vscode.window.visibleTextEditors.some(
        (editor) => editor.document === document
    );
}

class IdeContextFileBridge implements vscode.Disposable {
    private pendingTimer: NodeJS.Timeout | undefined;
    private currentRefresh: Promise<void> | undefined;
    private disposed = false;

    constructor(private readonly fileUri: vscode.Uri) {}

    dispose(): void {
        this.disposed = true;
        if (this.pendingTimer) {
            clearTimeout(this.pendingTimer);
            this.pendingTimer = undefined;
        }
    }

    scheduleRefresh(delay = 200): void {
        if (this.disposed) {
            return;
        }
        if (this.pendingTimer) {
            clearTimeout(this.pendingTimer);
        }
        this.pendingTimer = setTimeout(() => {
            this.pendingTimer = undefined;
            void this.performRefresh();
        }, delay);
    }

    async flush(): Promise<void> {
        if (this.disposed) {
            return;
        }
        if (this.pendingTimer) {
            clearTimeout(this.pendingTimer);
            this.pendingTimer = undefined;
        }
        await this.performRefresh();
    }

    get filePath(): string | undefined {
        if (this.fileUri.scheme !== "file") {
            return undefined;
        }
        return this.fileUri.fsPath;
    }

    private async performRefresh(): Promise<void> {
        if (this.disposed) {
            return;
        }

        if (this.currentRefresh) {
            await this.currentRefresh;
            return;
        }

        const task = (async () => {
            try {
                const block = await buildIdeContextBlock({
                    includeActiveEditor: true,
                    includeVisibleEditors: true,
                });
                const content = block ? `${block}\n` : "";
                await vscode.workspace.fs.writeFile(
                    this.fileUri,
                    Buffer.from(content, "utf8")
                );
            } catch (error) {
                const message =
                    error instanceof Error ? error.message : String(error);
                getOutputChannel().appendLine(
                    `[warn] Failed to update IDE context snapshot: ${message}`
                );
            }
        })();

        this.currentRefresh = task;
        try {
            await task;
        } finally {
            if (this.currentRefresh === task) {
                this.currentRefresh = undefined;
            }
        }
    }
}

function getVtcodeEnvironment(
    overrides: NodeJS.ProcessEnv = {}
): NodeJS.ProcessEnv {
    const env = { ...process.env, ...overrides };
    const contextPath = getIdeContextFilePath();
    if (contextPath) {
        env[IDE_CONTEXT_ENV_VARIABLE] = contextPath;
    } else {
        delete env[IDE_CONTEXT_ENV_VARIABLE];
    }
    return env;
}

function getPrimaryWorkspaceFolder(): vscode.WorkspaceFolder | undefined {
    const activeEditor = vscode.window.activeTextEditor;
    if (activeEditor) {
        const folder = vscode.workspace.getWorkspaceFolder(
            activeEditor.document.uri
        );
        if (folder) {
            return folder;
        }
    }

    const [firstWorkspace] = vscode.workspace.workspaceFolders ?? [];
    return firstWorkspace;
}

function createSpawnOptions(
    overrides: Partial<SpawnOptionsWithoutStdio> = {}
): SpawnOptionsWithoutStdio {
    const { env: overrideEnv, ...rest } = overrides;
    return {
        env: getVtcodeEnvironment(overrideEnv ?? {}),
        ...rest,
    };
}

function formatArgsForLogging(args: string[]): string {
    return args
        .map((arg) => {
            const value = String(arg);
            return /(\s|"|')/.test(value) ? JSON.stringify(value) : value;
        })
        .join(" ");
}

function formatArgsForShell(args: string[]): string {
    return args
        .map((arg) => {
            const value = String(arg);
            return quoteForShell(value);
        })
        .filter((value) => value.length > 0)
        .join(" ");
}

function quoteForShell(value: string): string {
    if (!/[\s"'\\$`]/.test(value)) {
        return value;
    }

    return `"${value.replace(/(["\\$`])/g, "\\$1")}"`;
}

function getWorkspaceRoot(): string | undefined {
    const activeEditor = vscode.window.activeTextEditor;
    if (activeEditor) {
        const workspaceFolder = vscode.workspace.getWorkspaceFolder(
            activeEditor.document.uri
        );
        if (workspaceFolder) {
            return workspaceFolder.uri.fsPath;
        }
    }

    const [firstWorkspace] = vscode.workspace.workspaceFolders ?? [];
    return firstWorkspace?.uri.fsPath;
}

function handleCommandError(contextLabel: string, error: unknown) {
    const message = error instanceof Error ? error.message : String(error);
    void vscode.window.showErrorMessage(
        `Failed to ${contextLabel} with VTCode: ${message}`
    );
}

function getOutputChannel(): vscode.OutputChannel {
    if (!outputChannel) {
        outputChannel = vscode.window.createOutputChannel("VTCode");
    }

    return outputChannel;
}

async function openToolsPolicyGuide(): Promise<void> {
    const [guide] = await vscode.workspace.findFiles(
        "docs/vtcode_tools_policy.md",
        "**/{node_modules,dist,out,.git,target}/**",
        1
    );
    if (guide) {
        const document = await vscode.workspace.openTextDocument(guide);
        await vscode.window.showTextDocument(document, { preview: false });
        return;
    }

    await vscode.env.openExternal(
        vscode.Uri.parse(
            "https://github.com/vinhnx/vtcode/blob/main/docs/vtcode_tools_policy.md"
        )
    );
}

async function openMcpGuide(): Promise<void> {
    const [guide] = await vscode.workspace.findFiles(
        "docs/guides/mcp-integration.md",
        "**/{node_modules,dist,out,.git,target}/**",
        1
    );
    if (guide) {
        const document = await vscode.workspace.openTextDocument(guide);
        await vscode.window.showTextDocument(document, { preview: false });
        return;
    }

    await vscode.env.openExternal(
        vscode.Uri.parse(
            "https://github.com/vinhnx/vtcode/blob/main/docs/guides/mcp-integration.md"
        )
    );
}

function ensureAgentTerminal(
    commandPath: string,
    cwd: string
): { terminal: vscode.Terminal; created: boolean } {
    if (agentTerminal) {
        return { terminal: agentTerminal, created: false };
    }

    const terminal = vscode.window.createTerminal({
        name: "VTCode Agent",
        cwd,
        env: getVtcodeEnvironment(),
        iconPath: new vscode.ThemeIcon("comment-discussion"),
    });

    // Instead of immediate execution, use a slight delay to allow any auto-activation to complete
    setTimeout(() => {
        void (async () => {
            if (ideContextBridge) {
                await ideContextBridge.flush();
            }
            const quotedCommandPath = /\s/.test(commandPath)
                ? `"${commandPath.replace(/\\/g, '\\\\').replace(/"/g, '\\"')}"`
                : commandPath;
            const configArgs = getConfigArguments();
            const terminalArgs = ["chat", ...configArgs];
            const argsText = formatArgsForShell(terminalArgs);
            const commandText =
                argsText.length > 0
                    ? `${quotedCommandPath} ${argsText}`
                    : quotedCommandPath;
            // Send the VTCode command after a brief delay to allow any environment activation to complete
            terminal.sendText(commandText, true);
        })();
    }, 800); // 800ms delay to allow environment activation if it happens

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
    const manifest = context.extension.packageJSON as
        | { enabledApiProposals?: string[] }
        | undefined;
    const proposals = manifest?.enabledApiProposals ?? [];

    if (proposals.length > 0) {
        const channel = getOutputChannel();
        channel.appendLine(
            `[warn] Proposed VS Code APIs enabled: ${proposals.join(", ")}.`
        );
    }
}

function logExtensionHostContext(context: vscode.ExtensionContext) {
    const channel = getOutputChannel();
    const remoteName = vscode.env.remoteName
        ? `remote (${vscode.env.remoteName})`
        : "local";
    const hostKind =
        vscode.env.uiKind === vscode.UIKind.Web ? "web" : "desktop";
    const modeLabel = getExtensionModeLabel(context.extensionMode);
    channel.appendLine(
        `[info] VTCode Companion activated in ${remoteName} ${hostKind} host (${modeLabel} mode).`
    );
}

function getExtensionModeLabel(mode: vscode.ExtensionMode): string {
    switch (mode) {
        case vscode.ExtensionMode.Development:
            return "development";
        case vscode.ExtensionMode.Test:
            return "test";
        case vscode.ExtensionMode.Production:
        default:
            return "production";
    }
}
