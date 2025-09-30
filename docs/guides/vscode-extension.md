# VS Code Extension for VT Code

This guide covers the bundled VS Code extension in `extensions/vscode-extension`, which launches the
Rust VT Code agent (via the `vtcode` binary) directly inside an integrated terminal. The extension
uses the same chat loop that powers the CLI experience and wires the workspace context through the
`WORKSPACE_DIR` environment variable so the agent operates on the open folder.

## Features

- Registers the **VT Code: Start Chat Session** command (`vtcode.startChat`).
- Opens a dedicated integrated terminal that runs `vtcode chat` for the active workspace.
- Reuses the Rust core agent loop, including MCP tools, configuration loading, and confirmation
  prompts.
- Captures the currently active editor tab and forwards its file path plus language ID to the agent
  for richer context.
- Enriches the chat environment with VS Code metadata (workspace name, folder count, app host,
  version, UI kind, and remote target) so the agent understands the editor surroundings.
- Allows customization of the terminal name, activation banner, and binary path via environment
  variables.
- Emits structured logs using the shared Pino logger so extension activity is observable when
  debugging.

## Prerequisites

1. Install Node.js 18+ and npm. Verify with `node --version` and `npm --version`.
2. Ensure the `vtcode` CLI is available on your PATH **or** export `VT_EXTENSION_VTCODE_BINARY`
   pointing at the compiled binary.
3. Open a VT Code-compatible workspace folder in VS Code so the extension can resolve the
   workspace path.

## Setup

```bash
cd extensions/vscode-extension
npm install
npm run compile
```

The compile step produces the `dist/` JavaScript consumed by VS Code. Leave `npm run watch` running
(if desired) to rebuild on every edit:

```bash
npm run watch
```

## Launching the Agent

1. Export any desired environment variables in the same shell that will start VS Code (examples are
   listed below).
2. Open the `extensions/vscode-extension` folder in VS Code.
3. Press **F5** (or use the Run and Debug panel) to start an Extension Development Host with the
   **Launch Extension** configuration.
4. In the development host window, open the Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`) and run
   **VT Code: Start Chat Session**.
5. A terminal named according to `VT_EXTENSION_TERMINAL_NAME` (default: `VT Code Chat`) appears and
   launches the real `vtcode chat` loop, scoped to the open workspace.

If a terminal with the configured name already exists, the extension disposes it before spawning a
fresh chat session. This keeps the integrated terminal output aligned with the current workspace
context.

## launch.json Example

Configure `.vscode/launch.json` with environment variables and the pre-launch compilation task:

```json
{
    "version": "0.2.0",
    "configurations": [
        {
            "name": "Launch Extension",
            "type": "extensionHost",
            "request": "launch",
            "runtimeExecutable": "${execPath}",
            "args": [
                "--extensionDevelopmentPath=${workspaceFolder}"
            ],
            "env": {
                "VT_EXTENSION_ACTIVATION_MESSAGE": "VT Code extension ready for chat.",
                "VT_EXTENSION_VTCODE_BINARY": "vtcode",
                "VT_EXTENSION_TERMINAL_NAME": "VT Code Chat"
            },
            "preLaunchTask": "npm: compile"
        }
    ]
}
```

## Environment Variables

| Variable | Purpose | Default |
| --- | --- | --- |
| `VT_EXTENSION_ACTIVATION_MESSAGE` | Notification displayed when the extension activates. | `VT Code extension activated.` |
| `VT_EXTENSION_VTCODE_BINARY` | Path to the `vtcode` binary executed inside the terminal. | `vtcode` |
| `VT_EXTENSION_TERMINAL_NAME` | Integrated terminal name reused between sessions. | `VT Code Chat` |
| `VT_EXTENSION_LOG_LEVEL` | Pino log level for extension diagnostics. | `info` |
| `VT_EXTENSION_ACTIVE_DOCUMENT` | Set automatically to the active editor's file path for agent context. | _(set by extension)_ |
| `VT_EXTENSION_ACTIVE_DOCUMENT_LANGUAGE` | Set automatically to the active editor's language identifier. | _(set by extension)_ |
| `VT_EXTENSION_WORKSPACE_NAME` | Workspace folder name backing the chat session. | _(set by extension)_ |
| `VT_EXTENSION_WORKSPACE_FOLDER_COUNT` | Number of workspace folders currently open. | _(set by extension)_ |
| `VT_EXTENSION_VSCODE_APP_NAME` | Host application name reported by VS Code. | _(set by extension)_ |
| `VT_EXTENSION_VSCODE_APP_HOST` | VS Code application host type (desktop, web, etc.). | _(set by extension)_ |
| `VT_EXTENSION_VSCODE_UI_KIND` | UI kind reported by VS Code (desktop or web). | _(set by extension)_ |
| `VT_EXTENSION_VSCODE_REMOTE_NAME` | Remote target identifier if running in a remote workspace. | _(set by extension)_ |
| `VT_EXTENSION_VSCODE_VERSION` | Running VS Code version. | _(set by extension)_ |
| `VT_EXTENSION_VSCODE_PLATFORM` | Platform detected by the host Node.js runtime. | _(set by extension)_ |

The extension always sets `WORKSPACE_DIR` for the spawned process so VT Code loads the correct
configuration and workspace metadata. If you need to pass additional CLI arguments, configure the
binary itself (e.g., via a wrapper script) so that environment variables remain the single source of
truth.

## Troubleshooting

- **`vtcode` command not found**: Either install the CLI globally (`cargo install --path .`) or set
  `VT_EXTENSION_VTCODE_BINARY` to the absolute path of the compiled binary.
- **No workspace detected**: Open a folder in VS Code before running the command. The extension logs
  `MissingWorkspace` events through the shared logger when no folder is available.
- **Logs too noisy**: Lower the verbosity by exporting `VT_EXTENSION_LOG_LEVEL=warn` before starting
  VS Code.
