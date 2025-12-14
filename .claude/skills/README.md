# VTCode Project Skills

This directory contains Claude Agent Skills tailored for VTCode development and document generation workflows.

## Python Environment Support

**VTCode automatically detects and uses safe Python environments** for skill execution:

1. **Active venv** - Uses `$VIRTUAL_ENV/bin/python` if virtual environment is activated
2. **Workspace .venv** - Uses `.venv/bin/python` if exists in project root
3. **uv** - Uses `uv run python` if uv is available in PATH (recommended)
4. **System python3** - Falls back to system Python as last resort

### Recommended Setup

```bash
# Option 1: Create project venv
python3 -m venv .venv
source .venv/bin/activate
pip install reportlab fpdf2 openpyxl python-docx

# Option 2: Use uv (faster, better isolation)
uv pip install reportlab fpdf2 openpyxl python-docx
```

Skills will automatically use your detected environment - no configuration needed!

## Available Skills

### Document Generation Skills

These skills leverage Anthropic's Agent Skills API to generate professional documents using Claude.

#### spreadsheet-generator

**v1.0.0** | VTCode Team

Generate professional Excel spreadsheets with data, charts, and formatting.

**Features:**

-   Multiple sheets with complex layouts
-   Data formulas (SUM, AVERAGE, VLOOKUP, etc.)
-   Charts and visualizations
-   Professional formatting and styling
-   Conditional formatting and number formats

**Use Cases:**

-   Financial reporting and dashboards
-   Sales performance analysis
-   Inventory management
-   Budget planning and tracking
-   Data analysis and summaries

**See:** `docs/skills/SPREADSHEET_EXAMPLE.md` and `examples/skills_spreadsheet.py`

---

#### doc-generator

**v1.0.0** | VTCode Team

Generate professional Word documents with formatted text, tables, and complex layouts.

**Features:**

-   Rich text formatting (bold, italic, underline, colors)
-   Heading styles and text hierarchy
-   Tables with merged cells and formatting
-   Lists (bullet points and numbered)
-   Page breaks and section management
-   Images and professional layout

**Use Cases:**

-   Business proposals and reports
-   Meeting minutes and agendas
-   Technical documentation
-   Training materials and manuals
-   Employee handbooks and policies

**See:** `docs/skills/WORD_DOCUMENT_EXAMPLE.md` and `examples/skills_word_document.py`

---

#### pdf-report-generator

**v1.0.0** | VTCode Team

Generate professional PDF documents with charts, styling, and complex layouts.

**Features:**

-   Advanced styling and custom fonts
-   Headers, footers, and page numbers
-   Charts and data visualizations
-   Images and professional branding
-   Color coordination and watermarks
-   Multi-section documents

**Use Cases:**

-   Financial and audit reports
-   Invoice and receipt generation
-   Certificates and diplomas
-   Marketing proposals
-   Technical specifications
-   Customer proposals and quotes

**See:** `docs/skills/PDF_GENERATION_EXAMPLE.md` and `examples/skills_pdf_generation.py`

---

### Development Process Skills

#### strict-architecture

**v0.0.1** | Anthropic

Enforces universal strict governance rules for Python, Golang, and .NET codebases.

**Constraints:**

-   500 lines per file maximum
-   5 functions per file maximum
-   4 function arguments maximum
-   Interface-first I/O design

**Use:** Apply during code reviews and refactoring to maintain clean architecture.

---

#### bdd-workflow

**v1.0.0** | VTCode Team

BDD and TDD development workflow for feature implementation.

**Flow:**

1. Create greenfield specification
2. Generate Gherkin scenarios
3. Convert to TDD prompts
4. Write tests (Red phase)
5. Implement code (Green phase)
6. Refactor and quality gates

---

#### code-orchestration

**v1.0.0** | VTCode Team

Orchestrated development with automatic task breakdown and delegation.

**Includes:**

-   Task decomposition
-   Delegated implementation
-   Coding standards enforcement
-   Automated testing
-   Quality gates

---

#### forensic-debugging

**v1.0.0** | VTCode Team

CRASH-RCA forensic debugging for systematic bug investigation.

**Workflow:**

1. Initialize session with issue description
2. Log hypotheses with confidence levels
3. Gather evidence with read-only tools
4. Generate structured RCA report

---

## Using These Skills

### In Claude Code

Use skills with `/skills` commands:

```bash
# List all available skills
/skills list

# Show skill details
/skills info spreadsheet-generator

# Load a skill for the session
/skills load spreadsheet-generator

# Execute a skill with input
/skills use spreadsheet-generator "Create financial dashboard"
```

### In Python Scripts

Use the Anthropic API with skill containers:

```python
import anthropic

client = anthropic.Anthropic()
response = client.messages.create(
    model="claude-4-5-sonnet",
    max_tokens=4096,
    tools=[{"type": "code_execution", "name": "bash"}],
    messages=[{
        "role": "user",
        "content": "Create an Excel spreadsheet with Q4 financial data"
    }],
    container={
        "type": "skills",
        "skills": [
            {"type": "anthropic", "skill_id": "xlsx", "version": "latest"}
        ]
    },
    betas=["code-execution-2025-08-25", "skills-2025-10-02"]
)
```

See `examples/skills_*.py` for complete implementations.

---

## Examples

### Run Spreadsheet Example

```bash
export ANTHROPIC_API_KEY=your-key-here
python examples/skills_spreadsheet.py
```

### Run Word Document Example

```bash
export ANTHROPIC_API_KEY=your-key-here
python examples/skills_word_document.py
```

### Run PDF Generation Example

```bash
export ANTHROPIC_API_KEY=your-key-here
python examples/skills_pdf_generation.py
```

---

## Skill Structure

Each skill is a directory with:

```
skill-name/
 SKILL.md              # Metadata (YAML) + Instructions (Markdown)
 ADVANCED.md           # Optional: Advanced documentation
 scripts/              # Optional: Executable scripts
    helper.py
 templates/            # Optional: Reference templates
     example.json
```

---

## Creating New Skills

Use the skill creation tool:

```bash
vtcode skills create ~/.vtcode/skills/my-skill
```

Or manually create the SKILL.md file with:

```yaml
---
name: my-skill
description: Brief description and when to use it
version: 1.0.0
author: Your Name
---

# My Skill

## Instructions
[Step-by-step guidance]

## Examples
- Example 1
- Example 2
```

---

## Validation

Validate a skill's SKILL.md:

```bash
vtcode skills validate ./.claude/skills/spreadsheet-generator
```

Requirements:

-   Valid YAML frontmatter
-   `name`: lowercase, alphanumeric + hyphens, 1-64 chars
-   `description`: 1-1024 characters
-   Must not contain "anthropic" or "claude" in name

---

## Related Documentation

-   `docs/SKILLS_GUIDE.md` - Complete skills guide
-   `docs/skills/CONTAINER_GUIDE.md` - Container configuration
-   `docs/skills/SPREADSHEET_EXAMPLE.md` - Spreadsheet examples
-   `docs/skills/WORD_DOCUMENT_EXAMPLE.md` - Document examples
-   `docs/skills/PDF_GENERATION_EXAMPLE.md` - PDF examples

---

## Tips

1. **Progressive Disclosure**: Skills load metadata first (100 tokens), then instructions on-demand
2. **File Handling**: Generated files are in code execution container, extract via Files API
3. **Error Handling**: Always check for API errors and edge cases
4. **Integration**: Combine multiple skills for complex workflows
5. **Performance**: Large documents may require optimization

---

## Support

For issues or questions:

1. Check skill documentation in SKILL.md
2. Review examples in `examples/` directory
3. See troubleshooting in `docs/SKILLS_GUIDE.md`
4. Consult Anthropic API docs: https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview
