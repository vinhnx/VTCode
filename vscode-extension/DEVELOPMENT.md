# VTCode Companion Extension Development Guide

This guide provides instructions for setting up, developing, and running the VTCode Companion VSCode extension.

## Prerequisites

Before you begin, ensure you have the following installed:

-   [Node.js](https://nodejs.org/) (version 18 or higher)
-   [Visual Studio Code](https://code.visualstudio.com/)
-   [VSCode Extension Development tools](https://code.visualstudio.com/api/get-started/your-first-extension)

## Getting Started

### 1. Clone and Setup

```bash
# Navigate to your extension directory
cd /path/to/vtcode/vscode-extension

# Install dependencies
npm install
```

### 2. Build the Extension

To compile the TypeScript code and bundle the extension:

```bash
npm run compile
```

This will:

-   Compile TypeScript files to JavaScript
-   Bundle the extension using esbuild
-   Output the compiled code to the `dist` directory

## Running the Extension

### Method 1: Using VSCode Debugger (Recommended for Development)

1. Open this directory in VSCode:

    ```bash
    code .
    ```

2. Press `F5` or go to the "Run and Debug" view (Ctrl+Shift+D or Cmd+Shift+D)

3. Select "Run Extension" from the launch configuration dropdown

4. Click the green "Run" button or press `F5` again

5. A new VSCode window titled "Extension Development Host" will open with your extension installed

### Method 2: Manual Build and Install

1. Build the extension:

    ```bash
    npm run compile
    ```

2. Package the extension:

    ```bash
    npm run package
    ```

3. Install the packaged `.vsix` file in VSCode:
    - Open VSCode
    - Go to Extensions view (Ctrl+Shift+X or Cmd+Shift+X)
    - Click the "..." menu button and select "Install from VSIX..."
    - Select your packaged `.vsix` file

## Development Workflow

### Watching for Changes

To automatically rebuild when you make changes, use the watch command:

```bash
npm run watch
```

### Running Tests

To run extension tests:

```bash
npm test
```

### Linting Code

To lint your TypeScript code:

```bash
npm run lint
```

## Extension Structure

```
vscode-extension/
├── package.json          # Extension manifest and configuration
├── src/
│   └── extension.ts      # Main extension entry point
├── tsconfig.json         # TypeScript configuration
├── .vscode/
│   └── launch.json       # Debug launch configurations
├── syntaxes/             # Language syntax definitions
└── dist/                 # Compiled output directory
```

## Debugging the Extension

When using the "Run Extension" launch configuration:

1. Set breakpoints in your TypeScript files in the original VSCode window
2. Interact with the extension in the "Extension Development Host" window
3. Debug output will appear in the Debug Console of the original window
4. Extension logs can be viewed in the Output panel (select "VTCode" from the dropdown)

## Commands Available

The extension contributes the following commands:

-   `vtcode.openQuickActions` - Open the quick actions panel
-   `vtcode.askAgent` - Send a question to the VTCode agent
-   `vtcode.askSelection` - Ask about the selected text
-   `vtcode.openConfig` - Open the vtcode.toml configuration file
-   `vtcode.launchAgentTerminal` - Launch an integrated VTCode terminal
-   And more...

Access these commands via the Command Palette (Ctrl+Shift+P or Cmd+Shift+P).

## Troubleshooting Terminal Activation

If you're seeing activation commands like `source /path/to/venv/bin/activate` when launching the VTCode agent terminal, this might be due to VSCode's terminal profile settings automatically activating Python environments. The extension simply sends the `vtcode chat` command to the terminal; if you're seeing environment activation, it's likely due to your VSCode configuration.

To resolve this:

1. Check your VSCode settings for any Python virtual environment auto-activation settings:

    - Look for "python.terminal.activateEnvironment" in your settings
    - Check your terminal profile settings in VSCode preferences

2. The extension itself does not activate Python environments - it launches a terminal with the appropriate working directory and sends the `vtcode chat` command.

## CLI Installation Requirements

The VTCode extension requires the VTCode CLI to be installed separately on your system. The extension cannot install the CLI automatically for security and policy reasons.

The CLI can be installed via:

-   Cargo: `cargo install vtcode` (if available)
-   Homebrew: `brew install vtcode` (if available)
-   npm: `npm install -g vtcode` (if available)
-   Or by following the manual installation instructions

The extension checks for the CLI availability when activated and will show appropriate warnings if it's not found. Users can update the `vtcode.commandPath` setting in VSCode to specify a custom path if the CLI is installed in a non-standard location.

## Adding a Status Bar Icon

The extension already includes functionality to add a status bar icon that indicates VTCode status. In the `extension.ts` file, you'll find:

1. A status bar item is created in the `activate` function
2. The status bar item has a tooltip and command associated with it
3. The `vtcode.launchAgentTerminal` command already exists to open the agent terminal

To customize the icon or add additional functionality to the status bar item:

1. Look for the status bar creation code in `extension.ts`
2. Modify the icon, text, or behavior as needed
3. The status bar already shows different states based on CLI availability

### Modifying Status Bar Click Behavior

By default, the status bar item opens the quick actions when clicked if the CLI is available. If you want to change this behavior to launch the agent terminal instead:

1. In `extension.ts`, locate the `updateStatusBarItem` function
2. Find this line in the available (true) section:
    ```typescript
    statusBarItem.command = "vtcode.openQuickActions";
    ```
3. Change it to:
    ```typescript
    statusBarItem.command = "vtcode.launchAgentTerminal";
    ```

### Customizing Status Bar Icon

To show a dedicated VTCode icon in the status bar (similar to other extensions in VSCode):

1. The status bar item is created in the `activate` function in `extension.ts`:

    ```typescript
    statusBarItem = vscode.window.createStatusBarItem(
        vscode.StatusBarAlignment.Left,
        100
    );
    ```

2. The current implementation uses the "$(hubot)" icon in the text, which displays a robot icon:

    ```typescript
    statusBarItem.text = `$(hubot) VTCode${suffix}`;
    ```

    The `$(hubot)` part is a VSCode codicon that displays a robot icon, appropriate for an AI agent extension. You can use other VSCode codicons like:

    - `$(comment-discussion)` for a discussion/chat icon
    - `$(terminal)` for a terminal icon (appropriate for VTCode chat)
    - `$(rocket)` for a rocket icon
    - `$(zap)` for a lightning bolt icon
    - `$(tools)` for a tools icon
    - And many others - you can browse all available codicons at https://microsoft.github.io/vscode-codicons/dist/codicon.html

3. To change to a different icon, modify the text assignment in the `updateStatusBarItem` function:

    ```typescript
    statusBarItem.text = `$(hubot) VTCode`; // Using a robot icon
    ```

4. For the VTCode chat functionality specifically, you might consider using the `$(terminal)` icon since it launches a terminal with the VTCode chat interface.

5. VSCode status bar items do not directly support custom SVG/PNG images. They use built-in codicons. However, the extension icon (shown in the Extensions view and marketplace) can be a custom SVG image stored in the media folder.

    - VSCode status bar items don't directly support custom images via `iconPath`. Instead, they use built-in codicons.
    - You can use any of the many available VSCode codicons in your status bar text:
        - `$(hubot)` - Robot icon (used in the current implementation)
        - `$(comment-discussion)` - Discussion/chat icon
        - `$(terminal)` - Terminal icon
        - `$(rocket)` - Rocket icon
        - `$(zap)` - Lightning bolt icon
        - And many others - you can browse all available codicons at https://microsoft.github.io/vscode-codicons/dist/codicon.html
    - To change the icon, modify the status bar text in the `updateStatusBarItem` function in `extension.ts`:
        ```typescript
        statusBarItem.text = `$(hubot) VTCode${suffix}`; // Using a robot icon
        ```
    - The media folder and custom icon files are still useful for other extension elements like the extension icon in the VSCode marketplace:
        ```json
        {
            "icon": "media/vtcode-icon.svg",
            "files": [
                "dist/**/*",
                "media/**/*",
                "syntaxes/**/*",
                "language-configuration.json"
            ]
        }
        ```

6. For a cleaner look with just an icon (no text), you can set:
    ```typescript
    statusBarItem.text = "$(hubot)"; // Just the icon
    ```

This will make the status bar icon launch the VTCode agent terminal when clicked instead of opening the quick actions panel.

You can also customize the appearance of the status bar, including text and tooltip, in the same `updateStatusBarItem` function.

The existing status bar item already has a command to open the agent terminal, and you can customize its appearance and behavior by modifying the code in `extension.ts`.

## Common Issues

### Missing VTCode CLI

If you see messages about the VTCode CLI being missing:

1. Install the VTCode CLI according to the [official installation guide](https://github.com/vinhnx/vtcode#installation)
2. Or update the `vtcode.commandPath` setting in VSCode preferences

### Extension Not Loading

If the extension doesn't appear to be loading:

1. Check that you're running the "Run Extension" launch configuration
2. Verify the extension appears in the Extensions view in the "Extension Development Host" window
3. Check the Developer Tools console (Help > Toggle Developer Tools) for errors

### PreLaunchTask 'watch' Issue

If you see the message "Waiting for preLaunchTask 'watch'...", this means VSCode is trying to run the watch task but it's not completing properly:

1. **Manual Solution**:

    - Open a terminal in VSCode
    - Run `npm run watch` manually in one terminal
    - Then start debugging the extension in another terminal

2. **Alternative Launch**:

    - Modify `.vscode/launch.json` to use the "compile" task instead of "watch"
    - Or temporarily change the launch configuration to not use a preLaunchTask:

    ```json
    {
        "name": "Run Extension (without watch)",
        "type": "extensionHost",
        "request": "launch",
        "args": ["--extensionDevelopmentPath=${workspaceFolder}"],
        "outFiles": ["${workspaceFolder}/dist/**/*.js"]
        // Remove the "preLaunchTask" line
    }
    ```

3. **Task Configuration**:
    - Ensure the watch task is properly defined in `.vscode/tasks.json`
    - The watch task should run `npm run watch` which executes the esbuild watch command

## Building for Distribution

To package the extension for distribution:

```bash
npm run package
```

This will create a `.vsix` file that can be installed in VSCode.

## Releasing the Extension

The extension includes an automated release script that handles version bumping, building, packaging, and publishing to both VSCode Marketplace and Open VSX Registry.

### Quick Release

To release a new version, use the `release.sh` script:

```bash
# Patch release (0.1.1 -> 0.1.2)
./release.sh patch

# Minor release (0.1.1 -> 0.2.0)
./release.sh minor

# Major release (0.1.1 -> 1.0.0)
./release.sh major
```

### What the Release Script Does

The automated release script performs the following steps:

1. **Checks dependencies** - Verifies that all required tools are installed (node, npm, git, jq, vsce, ovsx)
2. **Bumps version** - Updates the version in `package.json` according to semver
3. **Updates CHANGELOG** - Adds a new version entry with the current date
4. **Builds extension** - Compiles and bundles the TypeScript code
5. **Packages extension** - Creates a `.vsix` file
6. **Commits changes** - Commits the version bump to git
7. **Creates git tag** - Creates a tag with the format `vscode-v{version}` (e.g., `vscode-v0.1.2`)
8. **Pushes to GitHub** - Pushes commits and tags (with confirmation prompt)
9. **Publishes to VSCode Marketplace** - Publishes to the official marketplace (with confirmation prompt)
10. **Publishes to Open VSX** - Publishes to Open VSX Registry for VSCodium and alternatives (with confirmation prompt)
11. **Cleans up** - Removes old `.vsix` files

### Tag Naming Convention

The extension uses a **different naming convention** from the main VTCode binary to avoid version conflicts:

-   **Main VTCode binary tags**: `v0.39.0`, `v0.39.1`, etc.
-   **VSCode extension tags**: `vscode-v0.1.0`, `vscode-v0.1.1`, etc.

This ensures that extension releases don't conflict with the core VTCode CLI releases in the same repository.

### Manual Release Steps

If you prefer to release manually without the script:

1. **Bump the version**:

    ```bash
    # Update version in package.json manually or with npm
    npm version patch  # or minor, or major
    ```

2. **Update CHANGELOG.md**:

    ```markdown
    ## [0.1.2] - 2025-11-03

    ### Added

    -   New feature description

    ### Fixed

    -   Bug fix description
    ```

3. **Build and package**:

    ```bash
    npm run bundle
    npm run package
    ```

4. **Commit and tag**:

    ```bash
    git add package.json CHANGELOG.md
    git commit -m "chore: release vscode extension v0.1.2"
    git tag -a vscode-v0.1.2 -m "VSCode Extension Release v0.1.2"
    git push origin main --tags
    ```

5. **Publish to VSCode Marketplace**:

    ```bash
    vsce publish
    ```

6. **Publish to Open VSX Registry**:

    ```bash
    ovsx publish vtcode-companion-0.1.2.vsix
    ```

7. **Create GitHub Release**:
    - Go to https://github.com/vinhnx/vtcode/releases/new
    - Select tag: `vscode-v0.1.2`
    - Add release notes from CHANGELOG
    - Attach the `.vsix` file

### Prerequisites for Publishing

Before you can publish extensions, you need:

1. **VSCode Marketplace**:

    - A [Visual Studio Marketplace publisher account](https://marketplace.visualstudio.com/manage)
    - A Personal Access Token (PAT) with Marketplace permissions
    - Login with: `vsce login <publisher-name>`

2. **Open VSX Registry**:
    - An [Open VSX account](https://open-vsx.org/)
    - A Personal Access Token from your Open VSX account settings
    - The token will be requested when you run `ovsx publish`

### Testing a Release

After publishing, test the extension:

```bash
# Install from marketplace
code --install-extension nguyenxuanvinh.vtcode-companion

# Or install from local .vsix file
code --install-extension vtcode-companion-0.1.2.vsix
```

## Useful Links

-   [VSCode Extension API Documentation](https://code.visualstudio.com/api)
-   [Extension Development Tutorial](https://code.visualstudio.com/api/get-started/your-first-extension)
-   [VTCode Companion GitHub Repository](https://github.com/vinhnx/vtcode)
-   [Publishing Extensions](https://code.visualstudio.com/api/working-with-extensions/publishing-extension)
-   [Open VSX Registry](https://open-vsx.org/)
