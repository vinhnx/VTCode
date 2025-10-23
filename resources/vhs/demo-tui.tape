Output demo-tui.gif

Require vtcode
Require rustc

Set Shell bash
Set FontSize 15
Set Width 1200
Set Height 800
Set Theme '{ "name": "CustomTheme", "background": "#262626", "foreground": "#BFB38F", "selection": "#D99A4E", "cursor": "#BF4545", "black": "#262626", "red": "#BF4545", "green": "#BFB38F", "yellow": "#D99A4E", "blue": "#BFB38F", "magenta": "#D99A4E", "cyan": "#BFB38F", "white": "#BFB38F", "brightBlack": "#262626", "brightRed": "#BF4545", "brightGreen": "#BFB38F", "brightYellow": "#D99A4E", "brightBlue": "#BFB38F", "brightMagenta": "#D99A4E", "brightCyan": "#BFB38F", "brightWhite": "#BFB38F" }'
Set Framerate 60
Set TypingSpeed 30ms


# VT Code TUI Showcase
Type "# VT Code - Advanced TUI Coding Agent Demo"
Enter
Sleep 1s

# Launch the interactive TUI
Hide
Type "vtcode"
Enter
Show
Sleep 5s

# Show the TUI interface and available commands
Type "Hello! Can you show me around the interface and explain how to use VT Code effectively?"
Enter
Sleep 10s

# Show how to use slash commands
Type "/help"
Enter
Sleep 5s

# Show theme options
Type "/list-themes"
Enter
Sleep 3s

# Exit the TUI using slash command
Type "/exit"
Enter
Sleep 1s

# Show the advanced chat-verbose mode
Hide
Type "vtcode chat-verbose"
Enter
Show
Sleep 3s

# Demonstrate a more detailed query
Type "Show me the architecture of this tool and explain the multi-provider LLM support."
Enter
Sleep 8s

# Exit verbose mode using slash command
Type "/exit"
Enter
Sleep 1s

# Final message
Type "clear"
Enter
Sleep 1s
Type "# VT Code - Featuring an advanced TUI with mouse support, real-time PTY integration, and multi-provider LLM support!"
Enter
Sleep 2s