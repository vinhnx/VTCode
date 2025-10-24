Output demo-agentic-improved.gif

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
Sleep 5s

# Agentic coding use case: Ask agent to check current directory
Type "What directory are we in?"
Enter
Sleep 2s
# Wait for agent response to appear and TUI to update
Sleep 8s

# Ask agent to list files in current directory
Type "Can you list the files in this directory?"
Enter
Sleep 2s
# Wait for agent to use tools and respond
Sleep 10s

# Ask agent to analyze the project structure
Type "Can you analyze the project structure and dependencies?"
Enter
Sleep 2s
# Wait for analysis to complete
Sleep 12s

# Ask agent to read a specific file
Type "Can you show me the README.md file?"
Enter
Sleep 2s
# Wait for file to be read and displayed
Sleep 12s

# Agentic coding: Ask to make a change to a file
Type "Create a simple hello world function in a new file called demo.rs"
Enter
Sleep 2s
# Wait for file creation process
Sleep 12s

# Verify the file was created
Type "Can you read the demo.rs file to verify it was created correctly?"
Enter
Sleep 2s
# Wait for verification
Sleep 10s

# Exit using slash command
Hide
Type "/exit"
Enter
Show
Sleep 2s

# Demonstrate single ask mode with tool usage
Type "vtcode ask 'Check current directory using pwd command'"
Enter
Sleep 8s

# Show file analysis capability
Type "vtcode analyze"
Enter
Sleep 10s

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