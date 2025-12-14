# VT Code TUI Skills Integration Fix

## Problem

Skills are discovered in traditional locations (`.claude/skills/`, `.vtcode/skills/`) but fail to load with error: 
"Failed to load skill 'strict-architecture': This indicates an internal error"

## Root Cause

The TUI `/skills` command uses a different skill loading mechanism than the CLI. Skills need proper integration with:

1. **Skill Discovery**: Traditional locations need to be indexed
2. **Skill Loading**: TUI needs proper context and validation
3. **Skill Execution**: CLI tool bridge for external commands

## Solution

### 1. Ensure Skills Are Properly Formatted

Check that SKILL.md files have correct format:

```yaml
---
name: skill-name
description: Clear description of what the skill does and when to use it
---

# Skill Name

## Quick Start

Simple usage example

## Configuration

Required parameters and format
```

### 2. Integrate with TUI Discovery

Skills should be in locations the TUI scans:
- `.claude/skills/` (project-local)
- `~/.claude/skills/` (user-global)
- `~/.vtcode/skills/` (VT Code specific)

### 3. Debug Skill Loading

Run with debug logging to see actual errors:

```bash
RUST_LOG=debug vtcode
# Then in TUI: /skills info strict-architecture
```

### 4. Fix Common Issues

**Issue 1: Container skills validation failing**
- Skills requiring container skills (API-dependent) need fallbacks
- Add local implementation as fallback

**Issue 2: Missing dependencies**
- Ensure all required tools are available
- Check for missing Python/Node dependencies

**Issue 3: Incorrect manifest format**
- Validate YAML frontmatter syntax
- Ensure required fields (name, description) exist

## Testing

Test that `/skills` commands work correctly:

```bash
# List all skills
/skill list

# Get info on specific skill
/skills info strict-architecture

# Load skill
/skills load strict-architecture

# Use loaded skill
Use strict-architecture to validate code
```

## Expected Behavior

✅ **Before Fix**: 
```
/skill info strict-architecture
❌ Failed to load skill 'strict-architecture': This indicates an internal error
```

✅ **After Fix**:
```
/skill info strict-architecture
✓ strict-architecture: Enforces universal strict governance rules (500 lines, 5 funcs, 4 args) and interface-first I/O for Python, Golang, and .NET.
  Status: Available (local fallback)
  Dependencies: None
  Platform: Compatible
```

## Integration Checklist

- [ ] Skills properly formatted in .claude/skills/ or .vtcode/skills/
- [ ] SKILL.md has correct YAML frontmatter
- [ ] Skills have local fallback implementations
- [ ] All dependencies available in environment
- [ ] Skills appear in `/skills list` output
- [ ] `/skills info <name>` shows details correctly
- [ ] `/skills load <name>` loads without errors
- [ ] Loaded skills work in agent context