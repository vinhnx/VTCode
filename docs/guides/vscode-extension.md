# VS Code Extension for VT Code

This guide describes how to use the bundled VS Code extension located in `extensions/vscode-extension`.

## Features

- Displays an activation notification that can be customized with the `VT_EXTENSION_ACTIVATION_MESSAGE` environment variable.
- Adds the **VT Code: Show Greeting** command (command ID: `vtcode.showGreeting`) to the Command Palette.
- Allows the greeting message to be customized with the `VT_EXTENSION_GREETING_MESSAGE` environment variable.

## Setup

1. Install dependencies:

    ```bash
    npm install
    ```

2. Compile the extension:

    ```bash
    npm run compile
    ```

The above commands must be executed from the `extensions/vscode-extension` directory.

## Running in VS Code

1. Open VS Code and run **File > Open Folder...**, selecting the `extensions/vscode-extension` directory.
2. Run **View > Run**, choose **Launch Extension**, and press **F5** to open a new Extension Development Host window.
3. In the new window, open the Command Palette (`Ctrl+Shift+P` / `Cmd+Shift+P`) and run **VT Code: Show Greeting**.

## Environment Variables

| Variable | Purpose | Default |
| --- | --- | --- |
| `VT_EXTENSION_ACTIVATION_MESSAGE` | Notification displayed when the extension activates. | `VT Code extension activated.` |
| `VT_EXTENSION_GREETING_MESSAGE` | Message displayed when the greeting command runs. | `Welcome to the VT Code VS Code extension.` |

Set environment variables in your shell before launching VS Code or configure them inside `.vscode/launch.json` when developing the extension.
