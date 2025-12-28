# VT Code Status Line Configuration

This directory contains custom status line scripts for VT Code, inspired by the Claude Code status line configuration guide but adapted for VT Code's specific implementation.

## Available Scripts

1. **statusline.sh** - Basic bash implementation with ASCII characters and clean formatting
2. **statusline-advanced.sh** - Advanced bash implementation with ANSI color codes
3. **statusline.py** - Python implementation for those who prefer Python

## VT Code Integration

VT Code supports custom status line commands through the `[ui.status_line]` configuration in `vtcode.toml`:

```toml
[ui.status_line]
mode = "command"
command = "./.vtcode/statusline/statusline.sh"
refresh_interval_ms = 1000
command_timeout_ms = 200
```

## JSON Input Structure

VT Code passes the following JSON structure to the status line command via stdin:

```json
{
  "hook_event_name": "Status",
  "cwd": "/current/working/directory",
  "workspace": {
    "current_dir": "/current/working/directory",
    "project_dir": "/original/project/directory"
  },
  "model": {
    "id": "model/id",
    "display_name": "Model Name"
  },
  "runtime": {
    "reasoning_effort": "effort_level"
  },
  "git": {
    "branch": "branch_name",
    "dirty": true/false
  },
  "version": "vtcode_version",
  "context": {
    "utilization_percent": 65.5,
    "total_tokens": 12500
  }
}
```

## Features

- Shows model name and display name
- Displays current directory name
- Shows git branch and dirty status (if in a git repo)
- Shows reasoning effort level
- Shows context utilization and token count
- Shows VT Code version
- Color-coded elements based on importance/utilization

## Testing

To test the scripts manually:

```bash
# Test with sample data
cat .vtcode/statusline/test-status-input.json | ./.vtcode/statusline/statusline.sh

# Test with minimal data
cat .vtcode/statusline/test-status-input-simple.json | ./.vtcode/statusline/statusline.sh
```

## Requirements

- `jq` for JSON parsing (for bash versions)
- `bc` for floating point comparisons (for advanced bash version)
- Python 3 (for Python version)