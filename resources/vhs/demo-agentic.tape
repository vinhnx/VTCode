Output demo-agentic.gif

Require vtcode
Require rustc

Set Shell bash
Set FontSize 15
Set Width 1200
Set Height 800
Set Theme '{ "name": "CustomTheme", "background": "#262626", "foreground": "#BFB38F", "selection": "#D99A4E", "cursor": "#BF4545", "black": "#262626", "red": "#BF4545", "green": "#BFB38F", "yellow": "#D99A4E", "blue": "#BFB38F", "magenta": "#D99A4E", "cyan": "#BFB38F", "white": "#BFB38F", "brightBlack": "#262626", "brightRed": "#BF4545", "brightGreen": "#BFB38F", "brightYellow": "#D99A4E", "brightBlue": "#BFB38F", "brightMagenta": "#D99A4E", "brightCyan": "#BFB38F", "brightWhite": "#BFB38F" }'
Set Framerate 60
Set TypingSpeed 30ms


# Introduction to VT Code Agentic Coding
Type "# VT Code - Agentic Coding Demo"
Enter
Sleep 1s

# Show vtcode version
Type "vtcode --version"
Enter
Sleep 2s

# Clear the screen
Type "clear"
Enter
Sleep 1s

# Enter vtcode interactive chat mode
Hide
Type "vtcode"
Enter
Show
Sleep 3s

# Agentic coding use case: Ask agent to check current directory
Type "What directory are we in?"
Enter
Sleep 3s

# Agent will likely use pwd tool to answer
Sleep 5s

# Ask agent to list files in current directory
Type "Can you list the files in this directory?"
Enter
Sleep 3s

# Agent will likely use ls tool to answer
Sleep 5s

# Ask agent to analyze the project structure
Type "Can you analyze the project structure and dependencies?"
Enter
Sleep 3s

# Agent will likely use tree-sitter analysis
Sleep 8s

# Ask agent to read a specific file
Type "Can you show me the README.md file?"
Enter
Sleep 3s

# Agent will likely use read_file tool
Sleep 8s

# Agentic coding: Ask to make a change to a file
Type "Create a simple hello world function in a new file called demo.rs"
Enter
Sleep 3s

# Agent will likely use file creation tools
Sleep 10s

# Exit using slash command
Hide
Type "/exit"
Enter
Show
Sleep 1s

# Demonstrate single ask mode with tool usage
Type "vtcode ask 'Check current directory using pwd command'"
Enter
Sleep 5s

# Show file analysis capability
Type "vtcode analyze"
Enter
Sleep 5s

# Final showcase message
Type "clear"
Enter
Sleep 1s

Type "# VT Code - Agentic coding with tool usage, code analysis, and autonomous file operations!"
Enter
Sleep 2s

# End the demo
Type "echo 'Agentic coding demo completed!'"
Enter
Sleep 1s