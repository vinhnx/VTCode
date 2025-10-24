Output demo-tools-improved.gif

Require vtcode
Require rustc

Set Shell bash
Set FontSize 15
Set Width 1200
Set Height 800
Set Theme '{ "name": "CustomTheme", "background": "#262626", "foreground": "#BFB38F", "selection": "#D99A4E", "cursor": "#BF4545", "black": "#262626", "red": "#BF4545", "green": "#BFB38F", "yellow": "#D99A4E", "blue": "#BFB38F", "magenta": "#D99A4E", "cyan": "#BFB38F", "white": "#BFB38F", "brightBlack": "#262626", "brightRed": "#BF4545", "brightGreen": "#BFB38F", "brightYellow": "#D99A4E", "brightBlue": "#BFB38F", "brightMagenta": "#D99A4E", "brightCyan": "#BFB38F", "brightWhite": "#BFB38F" }'
Set Framerate 60
Set TypingSpeed 30ms


# VT Code Tool Usage Demo
Type "# VT Code - Agentic Tool Usage Demo"
Enter
Sleep 1s

# Launch the interactive TUI
Hide
Type "vtcode"
Enter
Show
Sleep 5s

# Show tool usage: Ask for current directory
Type "What directory are we in right now? Please use the appropriate tool to find out."
Enter
Sleep 2s
# Wait for tool usage and response
Sleep 10s

# Show tool usage: Ask to list files
Type "Can you list the files in the current directory using ls?"
Enter
Sleep 2s
# Wait for file listing tool
Sleep 10s

# Show tool usage: Run a shell command
Type "Can you run 'pwd' command to confirm our location?"
Enter
Sleep 2s
# Wait for command execution
Sleep 8s

# Show tool usage: File operations
Type "Create a new file called 'tool_demo.txt' with the content 'Hello from VT Code agent tool usage demo!'"
Enter
Sleep 2s
# Wait for file creation
Sleep 12s

# Show tool usage: Read the created file
Type "Can you read the content of tool_demo.txt to verify it was created?"
Enter
Sleep 2s
# Wait for file reading and verification
Sleep 10s

# Show how to use slash commands for tools
Type "/command pwd"
Enter
Sleep 2s
# Wait for slash command execution
Sleep 6s

# Show how to use slash commands for file listing
Type "/command ls -la"
Enter
Sleep 2s
# Wait for command output
Sleep 8s

# Show tree-sitter code analysis tool
Type "Can you analyze the structure of any Rust file in this project?"
Enter
Sleep 2s
# Wait for analysis completion
Sleep 12s

# Show how to use help for tools
Type "/help"
Enter
Sleep 2s
# Wait for help to display
Sleep 8s

# Exit the TUI
Type "/exit"
Enter
Sleep 2s

# Show the advanced chat-verbose mode to see tool usage details
Hide
Type "vtcode chat-verbose"
Enter
Show
Sleep 5s

# Demonstrate tool usage in verbose mode
Type "Show me the tools available and run a file listing to see how they work."
Enter
Sleep 2s
# Wait for verbose tool demonstration
Sleep 15s

# Exit verbose mode
Type "/exit"
Enter
Sleep 2s

# Final message
Type "clear"
Enter
Sleep 1s
Type "# VT Code - Featuring agentic tool usage, file operations, and real-time command execution!"
Enter
Sleep 2s