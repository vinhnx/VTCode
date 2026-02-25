# Agent Skills Guide

## Overview

Agent Skills are a simple, open format for giving agents new capabilities and expertise. VT Code implements the [open Agent Skills standard](http://agentskills.io/), allowing you to:

- **Discover** skills from your filesystem or Anthropic's marketplace
- **Load** skills into your agent sessions
- **Create** custom skills tailored to your workflow
- **Execute** skills with full access to VT Code's tools

Skills are modular instruction sets that guide Claude on how to complete specific tasks—whether that's processing documents, analyzing code, or automating workflows.

---

## Quick Start

### List Available Skills

```bash
vtcode skills list
```

Shows all discovered skills from multiple locations with precedence handling:

- **VT Code User Skills** (`~/.vtcode/skills/`) - Highest precedence
- **VT Code Project Skills** (`.agents/skills/`) - Project-specific skills
- **Pi User Skills** (`~/.pi/skills/`) - Pi framework user skills
- **Pi Project Skills** (`.pi/skills/`) - Pi framework project skills
- **Claude User Skills** (`~/.claude/skills/`) - Claude Code user skills
- **Claude Project Skills** (`.claude/skills/`) - Claude Code project skills
- **Codex User Skills** (`~/.codex/skills/`) - Codex CLI user skills

Skills from higher precedence locations override skills with the same name from lower precedence locations.
Claude skill directories support nested discovery; VT Code will scan nested `.claude/skills/**/SKILL.md` files.
Legacy note: `.vtcode/skills/` is deprecated for project skills but still supported for backward compatibility.

### View Skill Details

```bash
vtcode skills info strict-architecture
```

Displays the skill's metadata, description, and full instructions.

### Create a Custom Skill

```bash
vtcode skills create ~/.vtcode/skills/my-skill
```

Generates a template with:

- `SKILL.md` - Metadata (YAML) + Instructions (Markdown)
- `scripts/` - Optional executable scripts
- `templates/` - Optional reference materials

### Validate a Skill

```bash
vtcode skills validate ./my-skill
```

Checks that `SKILL.md` is valid per the [Agent Skills specification](http://agentskills.io/specification).

### Show Skill Paths

```bash
vtcode skills config
```

Displays configured skill search paths and directory structure.

---

## Skill Structure

Every skill is a directory with a required `SKILL.md` file, following the [Agent Skills standard](http://agentskills.io/specification):

```
my-skill/
 SKILL.md
    YAML frontmatter (metadata)
    Markdown body (instructions)
 scripts/
    helper.py          # Optional: executable scripts
 ADVANCED.md            # Optional: detailed guides
 templates/
     example.json       # Optional: reference materials
```

### SKILL.md Format

```yaml
---
name: my-skill
description: Brief description of what this skill does and when to use it
version: 1.0.0
author: Your Name
---

# My Skill

## Instructions
[Step-by-step guidance for Claude to follow]

## Examples
- Example usage 1
- Example usage 2

## Guidelines
- Guideline 1
- Guideline 2
```

### Metadata Requirements

**Required**:

- `name` - Lowercase alphanumeric + hyphens, max 64 chars
    - Cannot contain "anthropic" or "claude"
    - If omitted, VT Code defaults to the skill directory name
- `description` - Non-empty, max 1024 chars
    - Should include what it does and when to use it
    - If omitted, VT Code uses the first paragraph of the skill body

**Optional**:

- `version` - Semantic versioning (e.g., "1.0.0")
- `author` - Skill creator name
- `license` - License name or bundled license file reference
- `compatibility` - Environment requirements and product compatibility
- `allowed-tools` - Space- or comma-delimited list of tools allowed for the skill
- `argument-hint` - Usage hint for slash-command style invocation
- `user-invocable` - Toggle visibility in user menus
- `disable-model-invocation` - Prevents model invocation when skill is active
- `when-to-use` - Short guidance for automatic triggering
- `context` - Set to `fork` to run in a subagent context
- `agent` - Subagent type when `context = "fork"`
- `hooks` - Skill-scoped hook configuration

### Compliance with Agent Skills Standard

VT Code's skill system fully complies with the [open Agent Skills standard](http://agentskills.io/specification). Your skills are:

- **Portable** - Work across any Agent Skills-compatible tool or agent
- **Discoverable** - Listed and managed by VT Code's skill discovery system
- **Standardized** - Follow the common SKILL.md format for consistency
- **Composable** - Can be shared with other developers and teams

For the complete specification, visit http://agentskills.io/specification.

---

## Examples

### Example 1: Code Review Skill

```yaml
---
name: code-review-skill
description: Performs comprehensive code reviews focusing on security, performance, and maintainability
---

# Code Review Skill

## Instructions

1. Analyze the provided code for:
   - Security vulnerabilities
   - Performance bottlenecks
   - Code clarity and readability
   - Test coverage gaps

2. Provide specific, actionable feedback

3. Suggest improvements with code examples

## Examples

- Review a Python function for SQL injection risks
- Analyze a React component for memory leaks
- Check Go code for error handling gaps
```

### Example 2: Documentation Skill

```yaml
---
name: doc-generator
description: Generates comprehensive documentation from code and design docs
version: 2.0.0
author: DevOps Team
---

# Documentation Generator

## Instructions

When asked to generate documentation:

1. Extract key concepts from the code/design
2. Create clear, hierarchical documentation
3. Include examples and common use cases
4. Add troubleshooting sections for complex topics

## Guidelines

- Use Markdown for all output
- Include code blocks with language tags
- Link to related documentation
- Keep technical level appropriate for target audience
```

---

## Using Skills in Chat (Coming Soon)

In future updates, you'll be able to use skills directly in chat mode:

```bash
# Load a skill for the session
/skills load strict-architecture

# Execute a skill immediately with custom input
/skills use code-review-skill "review this function..."

# List loaded skills
/skills list

# Unload a skill
/skills unload strict-architecture
```

---

## Try More Examples

VT Code provides practical examples for using Anthropic's built-in Agent Skills (pptx, xlsx, docx, pdf). These examples demonstrate progressive disclosure and efficient file handling.

### Create a Spreadsheet

See `examples/skills_spreadsheet.py` for complete implementation.

```python
import anthropic

client = anthropic.Anthropic()
response = client.messages.create(
    model="claude-haiku-4-5",
    max_tokens=4096,
    tools=[{"type": "code_execution", "name": "bash"}],
    messages=[{
        "role": "user",
        "content": "Create an Excel spreadsheet with climate data for different cities"
    }],
    container={
        "type": "skills",
        "skills": [{"type": "anthropic", "skill_id": "xlsx", "version": "latest"}]
    },
    betas=["code-execution-2025-08-25", "skills-2025-10-02"]
)
```

**Use Cases:**

- Financial reports and dashboards
- Data analysis and summaries
- Employee records management
- Inventory tracking
- Sales performance reports

See [SPREADSHEET_EXAMPLE.md](docs/skills/SPREADSHEET_EXAMPLE.md) for detailed examples.

---

### Create a Word Document

See `examples/skills_word_document.py` for complete implementation.

```python
import anthropic

client = anthropic.Anthropic()
response = client.messages.create(
    model="claude-haiku-4-5",
    max_tokens=4096,
    tools=[{"type": "code_execution", "name": "bash"}],
    messages=[{
        "role": "user",
        "content": "Create a Word document with formatted project report"
    }],
    container={
        "type": "skills",
        "skills": [{"type": "anthropic", "skill_id": "docx", "version": "latest"}]
    },
    betas=["code-execution-2025-08-25", "skills-2025-10-02"]
)
```

**Use Cases:**

- Project proposals and reports
- Meeting minutes and agendas
- API documentation
- User guides and manuals
- Contract and legal documents

See [WORD_DOCUMENT_EXAMPLE.md](docs/skills/WORD_DOCUMENT_EXAMPLE.md) for detailed examples.

---

### Generate a PDF

See `examples/skills_pdf_generation.py` for complete implementation.

```python
import anthropic

client = anthropic.Anthropic()
response = client.messages.create(
    model="claude-haiku-4-5",
    max_tokens=4096,
    tools=[{"type": "code_execution", "name": "bash"}],
    messages=[{
        "role": "user",
        "content": "Generate a professional PDF invoice"
    }],
    container={
        "type": "skills",
        "skills": [{"type": "anthropic", "skill_id": "pdf", "version": "latest"}]
    },
    betas=["code-execution-2025-08-25", "skills-2025-10-02"]
)
```

**Use Cases:**

- Invoice and receipt generation
- Certificate and diploma creation
- Data reports and analysis
- Marketing collateral
- Technical documentation

See [PDF_GENERATION_EXAMPLE.md](docs/skills/PDF_GENERATION_EXAMPLE.md) for detailed examples.

---

### Create a PowerPoint Presentation

The PowerPoint skill (pptx) allows you to create professional presentations programmatically:

```python
import anthropic

client = anthropic.Anthropic()
response = client.messages.create(
    model="claude-haiku-4-5",
    max_tokens=4096,
    tools=[{"type": "code_execution", "name": "bash"}],
    messages=[{
        "role": "user",
        "content": "Create a PowerPoint presentation about renewable energy with 5 slides"
    }],
    container={
        "type": "skills",
        "skills": [{"type": "anthropic", "skill_id": "pptx", "version": "latest"}]
    },
    betas=["code-execution-2025-08-25", "skills-2025-10-02"]
)
```

**Slide Types:**

- Title slide with branding
- Content slides with bullet points
- Two-column layouts
- Image and multimedia slides
- Data visualization slides

---

## Running the Examples

All examples require the Anthropic API key:

```bash
# Set up authentication
export ANTHROPIC_API_KEY=your-api-key-here

# Run spreadsheet example
python examples/skills_spreadsheet.py

# Run Word document example
python examples/skills_word_document.py

# Run PDF generation example
python examples/skills_pdf_generation.py
```

### What You'll Learn

1. **Progressive Disclosure**: How Claude loads skill metadata, then full instructions on demand
2. **File Handling**: Extracting and downloading generated files using the Files API
3. **Error Handling**: Properly handling API errors and edge cases
4. **Integration Patterns**: Combining skills with code execution and other tools

---

## API Concepts

### Three Levels of Skill Loading

**Level 1: Metadata** (~100 tokens)

- Claude knows what skills are available
- Names and brief descriptions only
- Always included in system prompt
- No context cost when unused

**Level 2: Instructions** (<5K tokens)

- Full skill documentation and workflows
- Loaded when skill is triggered
- Consumed only during use

**Level 3: Resources** (on-demand)

- Scripts, templates, reference materials
- Executed via bash without loading contents
- No context overhead

### File Output Handling

Generated files are created in the code execution environment:

```python
# Extract file reference
file_id = None
for block in response.content:
    if hasattr(block, 'type') and block.type == 'file':
        file_id = block.file_id
        break

# Download file using Files API
if file_id:
    pdf_content = client.beta.files.retrieve_raw(file_id)
    with open('output.pdf', 'wb') as f:
        f.write(pdf_content.read())
```

---

## New VT Code Skills Location System

### Multi-Location Support

VT Code now supports a comprehensive skills location system similar to pi-mono, with proper precedence handling:

**Supported Locations:**

- **VT Code User Skills**: `~/.vtcode/skills/` (highest precedence)
- **VT Code Project Skills**: `.agents/skills/` (legacy `.vtcode/skills/` supported)
- **Pi User Skills**: `~/.pi/skills/`
- **Pi Project Skills**: `.pi/skills/`
- **Claude User Skills**: `~/.claude/skills/`
- **Claude Project Skills**: `.claude/skills/`
- **Codex User Skills**: `~/.codex/skills/` (lowest precedence)

### Key Features

**Precedence Handling**: Skills from higher precedence locations override skills with the same name from lower precedence locations.

**Recursive Scanning**: Automatically discovers skills in nested directories for user locations.

**Name Separators**: Different separators for different frameworks:

- VT Code locations: `/` (path separator)
- Pi locations: `:` (colon separator)

**Backward Compatibility**: All existing Claude Code skills continue to work in their original locations.

### Migration from .claude/skills to .agents/skills

All existing skills have been migrated from `.claude/skills` to `.agents/skills` to take advantage of the higher precedence system. The old locations are still supported for backward compatibility. Project skills in `.vtcode/skills/` are deprecated but will continue to load.

### Migration from .vtcode/skills to .agents/skills

Use the helper script to copy or move legacy project skills:

```bash
# Copy (default)
scripts/migrate_skills.sh --copy

# Move
scripts/migrate_skills.sh --move
```

### Discovery Process

1. **Scanning**: VT Code scans all configured locations recursively
2. **Precedence Resolution**: When name collisions occur, higher precedence skills win
3. **Loading**: Skills are loaded on-demand with progressive disclosure
4. **Execution**: Skills execute with full access to VT Code's tool ecosystem

---

## Advanced Topics

### Combining Skills with Code Execution

Skills work seamlessly with code execution:

```python
response = client.messages.create(
    model="claude-haiku-4-5",
    max_tokens=4096,
    tools=[
        {"type": "code_execution", "name": "bash"},
        {"type": "code_execution", "name": "python3"}
    ],
    messages=[{
        "role": "user",
        "content": "Analyze the data.csv file and create a report PDF"
    }],
    container={
        "type": "skills",
        "skills": [{"type": "anthropic", "skill_id": "pdf", "version": "latest"}]
    }
)
```

### Chaining Multiple Skills

Create complex workflows using multiple skills:

```python
# Step 1: Process data with code execution
# Step 2: Create spreadsheet with xlsx skill
# Step 3: Generate report PDF with pdf skill
# Step 4: Create presentation with pptx skill
```

---

## Troubleshooting

### File Not Generated

If files aren't created:

1. Check for code execution errors in response
2. Verify skill is enabled in container
3. Ensure proper beta headers are set
4. Check file ID extraction logic

### API Errors

Common errors and solutions:

```python
# Missing beta headers
# Fix: Add betas=["code-execution-2025-08-25", "skills-2025-10-02"]

# Invalid skill ID
# Fix: Use "pptx", "xlsx", "docx", or "pdf"

# File download fails
# Fix: Check file_id extraction, ensure API key is valid
```

---

## File Organization

### Organization by Role

**VT Code Global Skills** (`~/.vtcode/skills/`):

- Reusable across all projects (highest precedence)
- Examples: code-review, doc-generator, security-audit

**VT Code Project Skills** (`.agents/skills/`): (legacy `.vtcode/skills/` supported)

- Specific to your project
- Examples: brand-guidelines, api-spec, deployment-playbook

**Pi Framework Skills** (`~/.pi/skills/`, `.pi/skills/`):

- Skills following the Pi framework specification
- Uses `:` as name separator for nested skills

**Claude Code Skills** (`~/.claude/skills/`, `.claude/skills/`):

- Legacy Claude Code skills (backward compatible)
- Examples: existing skills migrated from Claude Code

**Codex CLI Skills** (`~/.codex/skills/`):

- Codex CLI compatible skills (lowest precedence)

### Precedence System

VT Code uses a precedence system to handle skill name collisions:

1. **VT Code User Skills** (highest precedence)
2. **VT Code Project Skills**
3. **Pi User Skills**
4. **Pi Project Skills**
5. **Claude User Skills**
6. **Claude Project Skills**
7. **Codex User Skills** (lowest precedence)

When multiple skills have the same name, the skill from the higher precedence location is used.

### Organization by Category

You can further organize skills by purpose:

```
~/.vtcode/skills/
 coding/
    code-review/
    refactoring/
    testing/
 documentation/
    api-docs/
    architecture/
 operations/
     deployment/
     monitoring/
```

---

## Best Practices

VT Code follows best practices from the [Agent Skills standard](http://agentskills.io/what-are-skills). Here are practical guidelines for creating effective skills:

### 1. Clear Descriptions

Write descriptions that help your agent understand when and how to use the skill:

```yaml
# Good
description: Analyzes code

# Better
description: Performs comprehensive code reviews, checking for security vulnerabilities, performance bottlenecks, and code quality issues in Python and Go
```

### 2. Progressive Detail

Structure instructions with increasing detail:

```markdown
## Instructions

### Quick Start

[Simple 3-step overview]

### Detailed Process

[Step-by-step with examples]

### Advanced Options

[Optional parameters and configurations]
```

### 3. Provide Examples

Show Claude concrete examples of how to use your skill:

```markdown
## Examples

**Input**: A Python function vulnerable to SQL injection
**Output**: Detailed explanation and fixed code

**Input**: React component with missing error boundaries
**Output**: Refactored component with error handling
```

### 4. Include Edge Cases

Document limitations and special cases:

```markdown
## Limitations

- Only supports Python 3.9+
- Requires git repository
- Cannot analyze binary files

## When Not to Use

- For real-time performance analysis (use profilers)
- For dynamic code generation patterns
```

### 5. Use Scripts for Automation

Bundle scripts in `scripts/` for deterministic operations:

```bash
my-skill/scripts/
 validate.py           # Validate input
 process.py            # Core logic
 format_output.py      # Format results
 README.md             # Script documentation
```

---

## Advanced Topics

### Progressive Disclosure

Skills use three levels of loading for efficiency:

**Level 1: Metadata** (~100 tokens)

- Always available in system prompt
- Agent knows skill exists and what it does
- No context cost when unused

**Level 2: Instructions** (<5K tokens)

- Loaded when skill is triggered
- Full SKILL.md body with guidance
- Consumed only when skill is used

**Level 3: Resources** (on-demand)

- Scripts executed via bash (output only)
- Templates accessed as needed
- No context penalty for bundled files

### Skill Composition

Skills can reference other skills:

```markdown
## Related Skills

This skill pairs well with:

- `code-review` - For comprehensive reviews
- `testing` - For test generation

## Dependencies

Requires `python >= 3.9` and `pytest`
```

### Skill Versioning

Use semantic versioning to manage skill evolution:

```yaml
version: 2.1.0 # Major.Minor.Patch


# In SKILL.md, document breaking changes:
# v2.0.0 - Changed output format from JSON to Markdown
# v2.1.0 - Added support for TypeScript
```

---

## Integration with IDE Extensions

When available, IDE extensions can:

- Discover and list skills
- Load skills with a single click
- Execute skills on selected code
- Display skill results inline

Check for Zed and VS Code extensions on the VT Code repository.

---

## Troubleshooting

### Skill Not Found

```bash
# Verify skill exists in new VT Code locations
ls ~/.vtcode/skills/
ls .agents/skills/

# Check legacy locations
ls ~/.claude/skills/
ls .claude/skills/
ls ~/.pi/skills/
ls .pi/skills/
ls ~/.codex/skills/

# Check search paths and precedence
vtcode skills config

# Validate SKILL.md
vtcode skills validate <path>
```

### Invalid Manifest

```bash
# Get detailed validation errors
vtcode skills validate <path>

# Requirements:
# - name: lowercase, hyphens, 1-64 chars
# - description: 1-1024 chars
# - Must not contain "anthropic" or "claude"
```

### Skill Not Loading in Chat

Currently loading in chat is being implemented. Use:

```bash
vtcode skills list      # Discover
vtcode skills info <name>  # Preview
```

---

## Resources

- **Agent Skills Standard**: http://agentskills.io/ - The open standard for agent skills
- **Agent Skills Specification**: http://agentskills.io/specification - Complete SKILL.md format spec
- **Agent Skills Integration Guide**: http://agentskills.io/integrate-skills - How to add skills support to your agent
- **Anthropic Skills Spec**: https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview
- **Skills Cookbook**: https://github.com/anthropics/claude-cookbooks/tree/main/skills
- **VT Code Skills Implementation**: `SKILLS_IMPLEMENTATION_SUMMARY.md`
- **Integration Plan**: `SKILLS_INTEGRATION_PLAN.md`
- **Location System**: `vtcode-core/src/skills/locations.rs` - Implementation details
- **Precedence System**: Follows pi-mono pattern with VT Code-specific enhancements

---

## Getting Help

### Common Questions

**Q: Can I use Anthropic's pre-built skills?**
A: Yes! Pre-built skills (PDF, Excel, Word, PowerPoint) will be downloadable in future updates via `vtcode skills fetch anthropic-pdf`.

**Q: Can skills call other tools?**
A: Yes! When integrated with the agent harness, skills can call VT Code tools (file operations, shell commands, etc.).

**Q: How are skills different from prompts?**
A: Skills are reusable, discoverable, versioned modules with explicit metadata. Prompts are conversation-level instructions.

**Q: Can I share skills with teammates?**
A: Yes! Keep skills in `.agents/skills/` (version control) or `~/.vtcode/skills/` (personal). Legacy project skills in `.vtcode/skills/` are supported but deprecated.

---

## Contributing Skills

If you've created a useful skill, consider sharing it:

1. Ensure SKILL.md follows the specification
2. Test with `vtcode skills validate`
3. Document in your project's `README.md`
4. Consider contributing to VT Code's skill library

See `CONTRIBUTING.md` for guidelines.
