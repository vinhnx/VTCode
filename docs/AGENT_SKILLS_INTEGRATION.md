# Using Agent Skills with VT Code Agent

Complete guide on integrating and using VT Code Agent Skills with the VT Code agent harness.

## Overview

Agent Skills extend VT Code's capabilities with specialized expertise for document generation, data analysis, and custom workflows. VT Code integrates Agent Skills through:

1. **CLI Commands** - Discover, load, and manage skills
2. **Skill Container** - Progressive disclosure and execution
3. **Agent Harness** - Skills as first-class tools
4. **Session Integration** - Load skills for interactive sessions

---

## Quick Start

### 1. List Available Skills

```bash
# List all discovered skills
vtcode skills list

# Shows skills from:
# - .vtcode/skills/          (project-local)
# - ./skills/                (workspace)
# - ~/.vtcode/skills/        (user global)
```

### 2. View Skill Details

```bash
# Show full skill documentation
vtcode skills info spreadsheet-generator

# Output includes:
# - Name and description
# - Version and author
# - Full instructions
# - Examples and use cases
# - Available resources
```

### 3. Create a Custom Skill

```bash
# Create skill template
vtcode skills create ~/.vtcode/skills/my-skill

# Creates:
# ~/.vtcode/skills/my-skill/SKILL.md
# ~/.vtcode/skills/my-skill/scripts/

# Edit SKILL.md with your skill definition
# YAML frontmatter + Markdown instructions
```

### 4. Validate Skill

```bash
# Check SKILL.md format
vtcode skills validate ./.claude/skills/spreadsheet-generator

# Validates:
#  YAML syntax
#  Required fields (name, description)
#  Name constraints (lowercase, alphanumeric)
#  Description length
```

### 5. Show Configuration

```bash
# View skill search paths
vtcode skills config

# Output:
# Workspace: /your/project
# Skill Search Paths:
#   • .claude/skills/       (project-local skills)
#   • ./skills/             (workspace skills)
#   • ~/.vtcode/skills/     (user global skills)
```

---

## Using Skills in Agent Sessions

### Load Skills for Chat Session

```bash
# Interactive chat with skill available
vtcode chat

# In chat, use:
/skills load spreadsheet-generator
/skills list
/skills info spreadsheet-generator
```

### Use Skills with Ask Command

```bash
# Ask agent to use a skill
vtcode ask "Use spreadsheet-generator to create a financial dashboard"

# The agent will:
# 1. Discover the skill
# 2. Load its metadata (100 tokens)
# 3. Load full instructions when triggered
# 4. Execute the skill's workflows
```

### Use Skills with Auto Mode

```bash
# Automatic task execution with skills available
vtcode auto "Create quarterly financial report with spreadsheet-generator"

# Agent autonomously:
# 1. Identifies relevant skills
# 2. Loads and executes them
# 3. Returns results
```

---

## Skill Architecture

### Three Levels of Progressive Disclosure

```
Level 1: Metadata (100 tokens)
 Skill name and description
 Always in system prompt
 No context cost when unused

    ↓ (triggered when needed)

Level 2: Instructions (<5K tokens)
 Full SKILL.md body
 Workflows and guidance
 Loaded on-demand

    ↓ (when executing)

Level 3: Resources (scripts, templates)
 Executable scripts
 Reference materials
 No context loading

**Prompt rendering and budgeting**

- Lean prompt now shows `name: description (dir + scope)` without absolute paths/backticks to avoid path leakage.
- Context manager uses tokenizer-backed sizing for instructions/resources instead of char/4 estimates; eviction honors actual token cost.
```

### Skill Metadata Structure

Every skill has a SKILL.md file:

```yaml
---
name: spreadsheet-generator
description: Generate professional Excel spreadsheets with data, charts, and formatting
version: 1.0.0
author: VT Code Team
license: MIT
model: inherit
mode: false
# Optional controls
# allowed-tools:
#   - Read
#   - Write
# disable-model-invocation: false
# when-to-use: "Trigger for multi-sheet spreadsheet builds"  # avoid relying on this; keep description explicit
# requires-container: false
# disallow-container: false
---

# Spreadsheet Generator Skill

## Instructions
[Step-by-step guidance for Claude]

## Examples
- Example 1
- Example 2

## Features Supported
- Multiple sheets
- Charts and visualizations
- Professional formatting
```

#### Manifest Control Fields

- `allowed-tools` (optional): explicit allowlist for the skill (e.g., `Read`, `Write`, `Bash(python {baseDir}/scripts/*:*)`). Keep minimal; do not expose unused tools.
- `disable-model-invocation` (optional): gate direct model calls when the skill is active; prefer tools/scripts.
- `when-to-use` (optional, <=512 chars): guidance appended to description in some stacks; rely on a descriptive `description` first since this field is not guaranteed in upstream docs.
- `requires-container` / `disallow-container` (optional, mutually exclusive): declare container requirements so VT Code can filter or prefer VT Code-native flows without string heuristics.
- `license` (optional): short license string for the skill.
- `model` (optional): override model; default inherits session.
- `mode` (optional): mark skills that change operating mode (highlighted separately in some UIs).

**Pathing best practice:** use `{baseDir}` in instructions when referencing bundled resources to avoid absolute paths and keep skills portable.

---

## Available Built-in Skills

### Document Generation (Anthropic Agent Skills)

#### spreadsheet-generator

Generate Excel spreadsheets with data, formulas, and charts.

**Integrates with:** Anthropic `xlsx` Agent Skill
**Location:** `.claude/skills/spreadsheet-generator/`

```bash
vtcode ask "Create a sales performance spreadsheet for Q4 2024"
```

**Features:**

- Multiple sheets and complex layouts
- Formulas (SUM, AVERAGE, VLOOKUP)
- Charts and visualizations
- Professional formatting

---

#### doc-generator

Generate Word documents with formatted text, tables, and styles.

**Integrates with:** Anthropic `docx` Agent Skill
**Location:** `.claude/skills/doc-generator/`

```bash
vtcode ask "Create project proposal document with timeline and budget"
```

**Features:**

- Rich text formatting
- Heading styles and hierarchy
- Tables and lists
- Page management

---

#### pdf-report-generator

Generate PDF documents with charts, styling, and layouts.

**Integrates with:** Anthropic `pdf` Agent Skill
**Location:** `.claude/skills/pdf-report-generator/`

```bash
vtcode ask "Generate quarterly financial report PDF"
```

**Features:**

- Advanced styling
- Charts and visualizations
- Headers/footers
- Professional branding

---

### Development Process Skills

#### strict-architecture

Enforce strict governance rules (500 lines, 5 functions, 4 args).

**Location:** `.claude/skills/strict-architecture/`

```bash
vtcode ask "Review this code with strict-architecture rules"
```

---

#### bdd-workflow

BDD and TDD feature development.

**Location:** `.claude/skills/bdd-workflow/`

---

#### code-orchestration

Orchestrated development with task breakdown.

**Location:** `.claude/skills/code-orchestration/`

---

#### forensic-debugging

Systematic CRASH-RCA debugging.

**Location:** `.claude/skills/forensic-debugging/`

---

## Agent Integration Points

### 1. Skill Discovery in Agent Harness

The agent harness discovers skills at startup:

```rust
use vtcode_core::skills::loader::SkillLoader;

let loader = SkillLoader::new(workspace_root);
let skills = loader.discover_skills()?;

// Skills are now available to the agent
for skill in skills {
    println!("Found: {} - {}", skill.name(), skill.description());
}
```

### 2. Load Skill into Agent Context

```rust
let skill = loader.load_skill("spreadsheet-generator")?;

// Skill metadata is added to system prompt (~100 tokens)
// Agent knows:
// - Skill name and description
// - When to use it
// - What it can do

// Full instructions loaded on-demand when agent decides to use it
```

### 3. Execute Skill from Agent

When agent decides to use a skill:

```rust
use vtcode_core::skills::executor::execute_skill_with_sub_llm;

let result = execute_skill_with_sub_llm(
    &skill,
    &agent_context,
    &llm_provider
).await?;

// Returns file IDs for generated documents
// Agent can reference in responses
```

---

## Code Examples

### Example 1: List Skills in CLI

```rust
// src/cli/skills.rs
use vtcode_core::skills::loader::SkillLoader;

pub async fn handle_skills_list(options: &SkillsCommandOptions) -> Result<()> {
    let loader = SkillLoader::new(options.workspace.clone());
    let skills = loader.discover_skills()?;

    for skill_ctx in &skills {
        let manifest = skill_ctx.manifest();
        println!("{:<30} | {}", manifest.name, manifest.description);
    }
    Ok(())
}
```

### Example 2: Load Skill for Session

```rust
// src/agent/runloop/skills_commands.rs
pub async fn handle_skill_command(
    action: SkillCommandAction,
    workspace: PathBuf,
) -> Result<SkillCommandOutcome> {
    let loader = SkillLoader::new(workspace);

    match action {
        SkillCommandAction::Load { name } => {
            let skill = loader.load_skill(&name)?;
            Ok(SkillCommandOutcome::LoadSkill { skill })
        }
        // ... other actions
    }
}
```

### Example 3: Use Skills in Python

```python
import anthropic
import os

client = anthropic.Anthropic(api_key=os.environ.get("ANTHROPIC_API_KEY"))

# Enable spreadsheet skill
response = client.messages.create(
    model="claude-haiku-4-5",
    max_tokens=4096,
    tools=[{"type": "code_execution", "name": "bash"}],
    messages=[{
        "role": "user",
        "content": "Create an Excel spreadsheet with climate data"
    }],
    container={
        "type": "skills",
        "skills": [
            {"type": "anthropic", "skill_id": "xlsx", "version": "latest"}
        ]
    },
    betas=[
        "code-execution-2025-08-25",
        "skills-2025-10-02"
    ]
)

# Extract file ID from response
for block in response.content:
    if hasattr(block, 'type') and block.type == 'file':
        print(f"Created file: {block.file_id}")
```

---

## Workflow Patterns

### Pattern 1: Simple Document Generation

```
User: "Create a project proposal document"
    ↓
Agent loads doc-generator skill
    ↓
Agent understands requirements
    ↓
Agent uses Anthropic docx skill via code execution
    ↓
Agent returns file reference
    ↓
User: File ready for download
```

### Pattern 2: Multi-Step Analysis

```
User: "Analyze sales data and create reports"
    ↓
Agent: Load spreadsheet-generator
    ↓
Step 1: Process CSV with code execution
    ↓
Step 2: Create Excel with spreadsheet-generator
    ↓
Step 3: Generate PDF report with pdf-report-generator
    ↓
User: Multiple files ready
```

### Pattern 3: Strict Architecture Review

```
User: "Review code with strict guidelines"
    ↓
Agent: Load strict-architecture skill
    ↓
Agent scans code against rules:
  - 500 lines per file
  - 5 functions per file
  - 4 arguments per function
    ↓
Agent: Provides refactoring recommendations
    ↓
User: Actionable improvements
```

---

## CLI Commands Summary

```bash
# List skills
vtcode skills list

# Show skill details
vtcode skills info <name>

# Create skill template
vtcode skills create <path>

# Validate SKILL.md
vtcode skills validate <path>

# Show configuration
vtcode skills config

# Use skill in question
vtcode ask "Use <skill> to <task>"

# Interactive session with skills
vtcode chat
# Then: /skills load <name>
# Then: /skills list
# Then: /skills info <name>
```

---

## Best Practices

1. **Use Appropriate Skill**
    - Financial reports → spreadsheet-generator
    - Technical docs → doc-generator
    - Final distribution → pdf-report-generator

2. **Progressive Disclosure**
    - Metadata loaded first (minimal cost)
    - Full instructions on-demand
    - Resources executed without loading

3. **Error Handling**
    - Check API responses for errors
    - Validate file creation
    - Handle edge cases gracefully

4. **Performance**
    - Large documents may take longer
    - Optimize charts and images
    - Use summaries for 100+ page documents

5. **Integration**
    - Combine multiple skills for complex workflows
    - Use code execution alongside skills
    - Chain skills for multi-step tasks

---

## Troubleshooting

### Skill Not Found

```bash
# Check skill discovery
vtcode skills list

# Verify path
ls -la .claude/skills/
ls -la ~/.vtcode/skills/

# Check environment
vtcode skills config
```

### Invalid Skill Manifest

```bash
# Validate SKILL.md
vtcode skills validate ./.claude/skills/my-skill

# Requirements:
# - Valid YAML frontmatter
# - name: 1-64 chars, lowercase, alphanumeric + hyphens
# - description: 1-1024 chars
# - Cannot contain "anthropic" or "claude"
```

### File Not Generated

```bash
# Check code execution in response
vtcode ask "Create file with <skill>"

# Verify:
# 1. Code execution errors in response
# 2. Skill enabled in container
# 3. API key valid
# 4. File ID extraction works
```

### API Errors

```python
# Missing beta headers
# Fix: Add betas=["code-execution-2025-08-25", "skills-2025-10-02"]

# Invalid skill ID
# Fix: Use "pptx", "xlsx", "docx", or "pdf"

# File download fails
# Fix: Check file_id, ensure API authentication
```

---

## Resources

- `.claude/skills/README.md` - Skills directory overview
- `docs/SKILLS_GUIDE.md` - Complete skills guide
- `examples/skills_*.py` - Working examples
- `docs/skills/*.md` - Specific skill documentation
- https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview - Anthropic skills spec

---

## Next Steps

1. **Explore Skills**: `vtcode skills list`
2. **Learn Details**: `vtcode skills info spreadsheet-generator`
3. **Try Examples**: `python examples/skills_spreadsheet.py`
4. **Create Custom Skills**: `vtcode skills create ~/.vtcode/skills/my-skill`
5. **Use in Agent**: `vtcode ask "Use spreadsheet-generator to..."`
