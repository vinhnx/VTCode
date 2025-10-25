Output demo.gif

Require vtcode
Require rustc

Set Shell bash
Set FontSize 15
Set Width 1200
Set Height 800
Set Theme '{ "name": "CustomTheme", "background": "#262626", "foreground": "#BFB38F", "selection": "#D99A4E", "cursor": "#BF4545", "black": "#262626", "red": "#BF4545", "green": "#BFB38F", "yellow": "#D99A4E", "blue": "#BFB38F", "magenta": "#D99A4E", "cyan": "#BFB38F", "white": "#BFB38F", "brightBlack": "#262626", "brightRed": "#BF4545", "brightGreen": "#BFB38F", "brightYellow": "#D99A4E", "brightBlue": "#BFB38F", "brightMagenta": "#D99A4E", "brightCyan": "#BFB38F", "brightWhite": "#BFB38F" }'
Set Framerate 60
Set TypingSpeed 30ms


# Introduction to VT Code
Type "# VT Code - Semantic Coding Agent. Written in Rust. "
Enter
Sleep 1s

# Show vtcode version
Type "vtcode --version"
Enter
Sleep 2s

# Show help to demonstrate available commands
Type "vtcode --help"
Enter
Sleep 5s

# Clear the screen
Type "clear"
Enter
Sleep 1s

# Enter vtcode interactive chat mode
Hide
Type "vtcode chat"
Enter
Show
Sleep 3s

# Demonstrate asking a question in the interactive mode
Type "Hello! Can you explain what VT Code is and its main features?"
Enter
Sleep 8s

# Show how to use single ask command
Hide
Type "/exit"
Enter
Show
Sleep 1s

# Demonstrate single ask mode (using single quotes to avoid escaping issues)
Type "vtcode ask 'What are the main features of VT Code?'"
Enter
Sleep 5s

# Show file analysis capability
Type "vtcode analyze"
Enter
Sleep 5s

# Clear and show configuration options
Type "clear"
Enter
Sleep 1s

# Show available models
Type "vtcode models list"
Enter
Sleep 3s

# Show how to initialize a project
Type "vtcode init --help"
Enter
Sleep 3s

# Final showcase message
Type "clear"
Enter
Sleep 1s

Type "# VT Code - Research-preview Rust coding agent with multi-provider LLM support, Tree-sitter code analysis, and advanced context management!"
Enter
Sleep 2s

# End the demo
Type "echo 'Demo completed!'"
Enter
Sleep 1s