# VHS Recording Guide for vtcode Demo

This document provides instructions for creating high-quality demo recordings of vtcode using VHS.

## Prerequisites

1. Install VHS:
```bash
# On macOS
brew install vhs

# On Linux
curl -fsSL https://get.vhs.dev | bash
```

2. Ensure vtcode is installed and working in your PATH:
```bash
vtcode --version
```

## Creating the Demo Recording

1. **Navigate to the project directory:**
```bash
cd /Users/vinh.nguyenxuan/Developer/learn-by-doing/vtcode
```

2. **Run the VHS tape file:**
```bash
vhs demo.tape
```

This will generate a `demo.gif` file showing vtcode in action.

## Customizing the Recording

The `demo.tape` file contains the script for your recording. You can customize it with:

- Different commands to showcase vtcode features
- Adjust timing with `Sleep` commands
- Modify visual settings like font size, width, height, and theme
- Add more examples of vtcode usage

## Best Practices for vtcode Demos

1. **Visual Settings:**
   - Use `Set Width 1200` for GitHub compatibility
   - Use `Set Theme "Aardvark Blue"` for a professional look
   - Maintain proper font size (32px recommended)

2. **Timing:**
   - Add `Sleep 2s` after key actions to let viewers process information
   - Use appropriate typing speeds to show but not slow down the demo

3. **Clean Display:**
   - Use `Hide` and `Show` to hide command entry
   - Focus on vtcode's output rather than the command line itself
   - Clear screen between different demos with `clear` command

4. **Content Structure:**
   - Start with a title or introduction
   - Show basic functionality first (like `vtcode --help`)
   - Demonstrate specific use cases
   - End with a conclusion

## Example Tape File Features

The provided `demo.tape` file demonstrates:

- How to set up the recording environment
- Basic vtcode commands to showcase
- Proper use of Hide/Show for cleaner visuals
- Appropriate timing between actions
- Use of themes and visual elements

## VHS Syntax Notes

Important syntax rules for VHS tape files:

- Commands should not be quoted (e.g., `Set Shell bash` not `Set Shell "bash"`)
- String values for Type commands should not include quotes around the entire string
- Use proper command separation with newlines
- Commands like `Enter`, `Sleep`, etc. should be on separate lines after the `Type` command

## Publishing Your Demo

To publish your demo directly from VHS:

```bash
vhs publish demo.tape
```

This will upload your recording and provide a shareable URL.

## Available Demo Recordings

Currently, this project includes TUI-focused demo recordings:

1. `demo.tape` → `demo.gif`: Basic TUI demonstration showing vtcode in interactive mode
2. `demo-tui.tape` → `demo-tui.gif`: More comprehensive TUI demonstration with multiple interactions
3. `demo-advanced-tui.tape` → `demo-advanced-tui.gif`: Advanced demonstration showing complex interactions

To regenerate the demos, run:
```bash
vhs demo.tape            # Creates demo.gif
vhs demo-tui.tape        # Creates demo-tui.gif
vhs demo-advanced-tui.tape # Creates demo-advanced-tui.gif
```

## Creating Your Own TUI Recordings

To create TUI-focused recordings of vtcode:
1. Use `Hide` before running `vtcode` command and `Show` after to focus on the interface
2. Simulate real interactions as users would in the TUI
3. Allow adequate time between interactions (2-4 seconds) to let viewers see the responses
4. Include various types of queries to showcase different capabilities
5. Use the custom theme with background #262626, foreground #BFB38F, selection #D99A4E, and cursor #BF4545

## Additional Tips

1. Keep recordings short (under 30 seconds for simple demos)
2. Focus on one main feature per demo
3. Test your tape file before finalizing
4. Consider creating multiple demo recordings for different vtcode features
