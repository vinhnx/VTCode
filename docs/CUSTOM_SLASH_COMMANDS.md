# Custom Slash Commands for VT Code

This document describes the custom slash command functionality that has been implemented based on the Claude documentation.

## Overview

Custom slash commands allow users to define their own commands in markdown files that can be executed directly from the VT Code chat interface. These commands work similarly to custom prompts but are invoked with `/command-name` instead of `/prompt:command-name`.

## Directory Structure

Custom slash commands are loaded from:

-   Personal commands: `~/.vtcode/commands/` (default)
-   Project-specific commands: `./.vtcode/commands/` (if exists)
-   Additional directories can be configured in `vtcode.toml`

## File Format

Commands are defined in markdown files with the following structure:

```markdown
---
description: Brief description of the command
argument-hint: [arg1] [arg2] - optional hint for arguments
allowed-tools: List of tools that should be allowed when this command is used
model: Specific model to use for this command (optional)
disable-model-invocation: Whether to prevent model invocation (optional)
---

Command content with optional placeholders:

-   $1, $2, etc. for positional arguments
-   $ARGUMENTS for all arguments joined together
-   $VARIABLE_NAME for named arguments like VARIABLE_NAME=value
-   !`command` to execute bash commands and substitute output
```

## Examples

### Simple Review Command

File: `~/.vtcode/commands/review.md` (personal command)
File: `.vtcode/commands/review.md` (project-specific command)

```markdown
---
description: Review a specific file
argument-hint: [file]
---

Please review the file $1 and provide feedback on code quality, potential issues, and suggestions for improvement.
```

Usage: `/review src/main.rs`

### Command with Bash Execution

File: `~/.vtcode/commands/status.md`

```markdown
---
description: Show current git status
---

Current repository status:
!`git status --short`
```

Usage: `/status`

### Command with Named Arguments

File: `~/.vtcode/commands/analyze.md`

```markdown
---
description: Analyze code with specific focus
argument-hint: TARGET=FILE FOCUS=ASPECT
---

Please analyze $TARGET with focus on $FOCUS. Look for potential issues, performance bottlenecks, and code quality improvements.
```

Usage: `/analyze TARGET=src/lib.rs FOCUS=performance`

## Features Implemented

-   [x] Project-specific and personal slash commands
-   [x] Argument handling (positional and named)
-   [x] Bash command execution with `!` syntax
-   [x] Frontmatter support for configuration
-   [x] Integration with existing slash command system
-   [x] Help system integration

## Configuration

Custom slash commands can be configured in `vtcode.toml`:

```toml
[agent.custom_slash_commands]
enabled = true
directory = "~/.vtcode/commands"  # Personal commands directory
extra_directories = [".vtcode/commands", "./custom-commands"]  # Additional directories to search
max_file_size_kb = 64
```

By default, VT Code will also look for project-specific commands in the `.vtcode/commands/` directory automatically.
