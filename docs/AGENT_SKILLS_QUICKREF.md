# Agent Skills Quick Reference

## CLI Commands

```bash
# Discover
vtcode skills list                    # List all skills
vtcode skills info spreadsheet-gen    # Show skill details
vtcode skills config                  # Show search paths

# Create
vtcode skills create ~/.vtcode/skills/my-skill

# Validate
vtcode skills validate ./path/to/skill
```

## Using Skills with Agent

```bash
# Chat session (load skills interactively)
vtcode chat
# Then: /skills load spreadsheet-generator

# Ask agent to use a skill
vtcode ask "Create Excel spreadsheet with Q4 data"

# Auto mode (agent chooses skills)
vtcode auto "Generate financial report"
```

## Built-in Skills

| Skill                   | Type     | Use For                          | Command                                   |
| ----------------------- | -------- | -------------------------------- | ----------------------------------------- |
| `spreadsheet-generator` | Document | Excel, dashboards, data analysis | `/skills use spreadsheet-generator "..."` |
| `doc-generator`         | Document | Word docs, proposals, reports    | `/skills use doc-generator "..."`         |
| `pdf-report-generator`  | Document | PDFs, invoices, certificates     | `/skills use pdf-report-generator "..."`  |
| `strict-architecture`   | Code     | Architecture review, constraints | `/skills use strict-architecture "..."`   |
| `bdd-workflow`          | Process  | TDD/BDD feature development      | `/skills load bdd-workflow`               |
| `code-orchestration`    | Process  | Orchestrated development         | `/skills load code-orchestration`         |
| `forensic-debugging`    | Process  | Systematic bug investigation     | `/skills load forensic-debugging`         |

## Skill Structure

```
skill-name/
 SKILL.md              # Metadata + Instructions (REQUIRED)
 ADVANCED.md           # Optional: Extended guide
 scripts/              # Optional: Helper scripts
    helper.py
 templates/            # Optional: Reference materials
     example.json
```

## SKILL.md Template

```yaml
---
name: my-skill                    # lowercase, alphanumeric + hyphens
description: What it does and when to use it  # 1-1024 chars
version: 1.0.0                    # optional
author: Your Name                 # optional
license: MIT
model: inherit
mode: false
# Optional controls
# allowed-tools:
#   - Read
#   - Write
# disable-model-invocation: false
# when-to-use: "Trigger guidance (prefer descriptive 'description')"
# requires-container: false
# disallow-container: false
---

# My Skill

## Instructions
[Step-by-step guidance for Claude]

## Examples
- Example 1
- Example 2

## Features
- Feature 1
- Feature 2
```

## Agent Workflow

```
1. List skills        → vtcode skills list
2. View skill details → vtcode skills info <name>
3. Load in session    → /skills load <name>
4. Use in chat        → Ask agent to use the skill
5. Agent triggers     → Loads full instructions
6. Execute           → Uses Anthropic Agent Skill
7. Return result     → File ID or task completion
```

## Common Tasks

### Create Financial Dashboard

```bash
vtcode ask "Use spreadsheet-generator to create Q4 financial dashboard"
```

### Generate Project Proposal

```bash
vtcode ask "Use doc-generator to create project proposal document"
```

### Build PDF Report

```bash
vtcode ask "Use pdf-report-generator to create quarterly report"
```

### Code Architecture Review

```bash
vtcode ask "Review this code using strict-architecture rules"
```

### Implement with TDD

```bash
/skills load bdd-workflow
# Then use /architect command for BDD workflow
```

## Skill Search Paths

VTCode searches for skills in order:

1. `.claude/skills/` (project-local)
2. `./skills/` (workspace)
3. `~/.vtcode/skills/` (user global)

## File Handling

### Generated Files

Files created by skills are in code execution environment.

### Extract File ID (Python)

```python
file_id = None
for block in response.content:
    if hasattr(block, 'type') and block.type == 'file':
        file_id = block.file_id
        break
```

### Download File

```python
pdf_content = client.beta.files.retrieve_raw(file_id)
with open('output.pdf', 'wb') as f:
    f.write(pdf_content.read())
```

## Progressive Disclosure

```
Metadata (100 tokens)
  ↓ [triggered]
Instructions (<5K tokens)
  ↓ [executing]
Resources (scripts, templates)
  ↓ [on-demand]
No context loading
```

## Validation

```bash
# Check SKILL.md is valid
vtcode skills validate <path>

# Requirements:
#  Valid YAML frontmatter
#  name: 1-64 chars, lowercase
#  description: 1-1024 chars
#  Cannot contain "anthropic" or "claude"
```

## Examples Location

```bash
examples/skills_spreadsheet.py      # Spreadsheet example
examples/skills_word_document.py    # Word document example
examples/skills_pdf_generation.py   # PDF example

# Run with API key:
export ANTHROPIC_API_KEY=sk-...
python examples/skills_spreadsheet.py
```

## Documentation

-   `docs/SKILLS_GUIDE.md` - Complete guide
-   `docs/AGENT_SKILLS_INTEGRATION.md` - Integration details
-   `.claude/skills/README.md` - Skills directory overview
-   `docs/skills/SPREADSHEET_EXAMPLE.md` - Spreadsheet examples
-   `docs/skills/WORD_DOCUMENT_EXAMPLE.md` - Document examples
-   `docs/skills/PDF_GENERATION_EXAMPLE.md` - PDF examples

## Error Handling

| Error              | Solution                            |
| ------------------ | ----------------------------------- |
| Skill not found    | Check `vtcode skills list` and path |
| Invalid SKILL.md   | Run `vtcode skills validate`        |
| File not generated | Verify code execution succeeded     |
| API error          | Check API key and beta headers      |

## Tips

-   **Use metadata first** - Costs only 100 tokens initially
-   **Combine skills** - Chain multiple skills for complex workflows
-   **Iterate** - Ask agent to refine output
-   **Check examples** - See `examples/` for working code
-   **Validate skills** - Always validate before using
-   **Set container flags** - Use `requires-container` or `disallow-container` to control filtering instead of relying on keywords
-   **Lean prompt hygiene** - Paths are rendered as `dir + scope` only to avoid leaking absolute paths
-   **Use {baseDir} for paths** - Reference bundled resources with `{baseDir}` to keep skills portable
-   **Keep allowed-tools minimal** - Limit to the tools the skill actually needs (e.g., `Read,Write`)

## Slash Commands (In Chat)

```bash
/skills                          # Show help
/skills list                      # List available skills
/skills info <name>              # Show skill details
/skills load <name>              # Load skill for session
/skills unload <name>            # Unload skill
/skills use <name> <input>       # Execute skill with input
```

## Anthropic Agent Skills

VTCode integrates Anthropic's pre-built skills:

-   **xlsx** - Excel spreadsheet generation
-   **docx** - Word document generation
-   **pdf** - PDF document generation
-   **pptx** - PowerPoint presentation generation

Access via `spreadsheet-generator`, `doc-generator`, `pdf-report-generator` skills.

## Troubleshooting Quick Tips

```bash
# Skill not found?
vtcode skills config

# YAML error?
vtcode skills validate <path>

# File not created?
# Check response for code execution errors

# API error?
# Verify ANTHROPIC_API_KEY is set
```

---

See `docs/AGENT_SKILLS_INTEGRATION.md` for detailed guide.
