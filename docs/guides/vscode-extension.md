# VS Code Extension for VT Code

This guide describes how to use the bundled VS Code extension located in `extensions/vscode-extension`.

## Features

- Displays an activation notification that can be customized with the `VT_EXTENSION_ACTIVATION_MESSAGE` environment variable.
- Adds the **VT Code: Show Greeting** command (command ID: `vtcode.showGreeting`) to the Command Palette.
- Allows the greeting message to be customized with the `VT_EXTENSION_GREETING_MESSAGE` environment variable.

## Setup

1. Ensure Node.js 18+ and npm are installed by running `node --version` and `npm --version` in your shell.
2. Open a terminal in `extensions/vscode-extension` and install dependencies:

    ```bash
    npm install
    ```

3. Build the TypeScript sources once so VS Code can load the compiled JavaScript:

    ```bash
    npm run compile
    ```

4. (Optional) Start the TypeScript compiler in watch mode while iterating:

    ```bash
    npm run watch
    ```

    Leave this terminal open so the `dist/` output stays in sync with your edits.

## Running in VS Code

1. Export any desired environment variables in the same shell that launches VS Code:

    ```bash
    export VT_EXTENSION_ACTIVATION_MESSAGE="Ready to build with VT Code"
    export VT_EXTENSION_GREETING_MESSAGE="Hello from the development host"
    ```

    Alternatively, define them in a `.vscode/launch.json` `env` block (see below) so they are scoped to
    the debugger session.

2. Open VS Code, then choose **File > Open Folder...** and select `extensions/vscode-extension`.
3. When prompted, install the recommended extensions (ESLint, TypeScript tools) to match the project setup.
4. Open the **Run and Debug** view (`Ctrl+Shift+D` / `Cmd+Shift+D`), pick **Launch Extension**, and press **F5**.
5. A new **Extension Development Host** window launches. In that window, open the Command Palette
   (`Ctrl+Shift+P` / `Cmd+Shift+P`) and run **VT Code: Show Greeting** to verify the command and activation
   messages.

If you modify TypeScript files while the development host is running, stop the debug session, wait for
`npm run watch` to finish rebuilding (or run `npm run compile` again), and then press **F5** to reload the
extension.

### launch.json Example

Add the following snippet to `.vscode/launch.json` inside the `extensions/vscode-extension` workspace to run
with explicit environment variables and pre-launch compilation:

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
                "VT_EXTENSION_ACTIVATION_MESSAGE": "Ready to build with VT Code",
                "VT_EXTENSION_GREETING_MESSAGE": "Hello from the development host"
            },
            "preLaunchTask": "npm: compile"
        }
    ]
}
```

The `preLaunchTask` ensures the compiled output is refreshed before VS Code spins up the extension host.

## Environment Variables

| Variable | Purpose | Default |
| --- | --- | --- |
| `VT_EXTENSION_ACTIVATION_MESSAGE` | Notification displayed when the extension activates. | `VT Code extension activated.` |
| `VT_EXTENSION_GREETING_MESSAGE` | Message displayed when the greeting command runs. | `Welcome to the VT Code VS Code extension.` |

Set environment variables in your shell before launching VS Code or configure them inside `.vscode/launch.json` when developing the extension.
