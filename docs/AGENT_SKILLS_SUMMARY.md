# Agent Skills Implementation Summary

## What Was Added

Complete Anthropic Agent Skills integration for VTCode, following the Claude API quickstart guide and best practices.

### New Skills in `.claude/skills/`

1. **spreadsheet-generator** (v1.0.0)

    - Generate Excel spreadsheets with data, formulas, charts
    - Integrates with Anthropic's `xlsx` Agent Skill
    - Use cases: financial dashboards, data analysis, inventory tracking

2. **doc-generator** (v1.0.0)

    - Generate Word documents with formatting, tables, styles
    - Integrates with Anthropic's `docx` Agent Skill
    - Use cases: proposals, meeting minutes, technical docs

3. **pdf-report-generator** (v1.0.0)
    - Generate PDF documents with charts, styling, professional layouts
    - Integrates with Anthropic's `pdf` Agent Skill
    - Use cases: invoices, certificates, financial reports

### Documentation Files

1. **docs/SKILLS_GUIDE.md** (updated)

    - Comprehensive skills guide
    - Added "Try More Examples" section with:
        - Create spreadsheet examples
        - Create Word document examples
        - Generate PDF examples
        - Create PowerPoint examples
    - API concepts and progressive disclosure
    - Advanced topics and troubleshooting

2. **docs/AGENT_SKILLS_INTEGRATION.md** (new)

    - How to use skills with VT Code agent
    - Quick start guide
    - Available built-in skills
    - Code examples (Rust and Python)
    - Workflow patterns
    - Best practices and troubleshooting

3. **docs/AGENT_SKILLS_QUICKREF.md** (new)

    - Quick reference card
    - CLI commands
    - Common tasks
    - Validation and error handling
    - Troubleshooting tips

4. **.claude/skills/README.md** (new)

    - Skills directory overview
    - All available skills documented
    - Usage patterns
    - Examples and validation

5. **docs/skills/SPREADSHEET_EXAMPLE.md** (new)

    - Spreadsheet skill examples
    - Python code snippets
    - Use cases and patterns

6. **docs/skills/WORD_DOCUMENT_EXAMPLE.md** (new)

    - Word document skill examples
    - Document types and features
    - Best practices

7. **docs/skills/PDF_GENERATION_EXAMPLE.md** (new)
    - PDF generation examples
    - Use cases (invoices, certificates, reports)
    - Performance considerations

### Example Scripts

1. **examples/skills_spreadsheet.py**

    - Create climate data spreadsheet
    - Create financial spreadsheet
    - Demonstrates code execution with xlsx skill

2. **examples/skills_word_document.py**

    - Create business report document
    - Create meeting minutes document
    - Demonstrates code execution with docx skill

3. **examples/skills_pdf_generation.py**
    - Generate invoice PDF
    - Generate data report PDF
    - Demonstrates code execution with pdf skill

---

## How to Use Skills

### Quick Start

```bash
# 1. List available skills
vtcode skills list

# 2. View skill details
vtcode skills info spreadsheet-generator

# 3. Use skill in agent
vtcode ask "Create Excel spreadsheet with Q4 financial data"

# 4. Or in interactive chat
vtcode chat
# Then: /skills load spreadsheet-generator
```

### Common Tasks

**Create Financial Dashboard:**

```bash
vtcode ask "Use spreadsheet-generator to create Q4 financial dashboard"
```

**Generate Project Proposal:**

```bash
vtcode ask "Use doc-generator to create project proposal document"
```

**Build PDF Report:**

```bash
vtcode ask "Use pdf-report-generator to create quarterly report"
```

### In Agent Code

```rust
use vtcode_core::skills::loader::SkillLoader;

// Discover skills
let loader = SkillLoader::new(workspace_root);
let skills = loader.discover_skills()?;

// Load a skill
let skill = loader.load_skill("spreadsheet-generator")?;

// Use skill metadata in system prompt
// Full instructions loaded on-demand
```

### In Python

```python
import anthropic

client = anthropic.Anthropic()
response = client.messages.create(
    model="claude-4-5-sonnet",
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
    betas=["code-execution-2025-08-25", "skills-2025-10-02"]
)
```

---

## Architecture

### Progressive Disclosure

Skills use three levels of loading to minimize context cost:

```
Level 1: Metadata (~100 tokens)
   Loaded at startup
   Agent knows skill exists and what it does
   No context cost when unused

Level 2: Instructions (<5K tokens)
   Loaded when skill is triggered
   Full SKILL.md body with workflows
   Consumed only during use

Level 3: Resources (scripts, templates)
   Executed via bash without loading
   Reference materials as needed
   No context overhead
```

### Skill Container

Skills are passed to Claude via the `container` parameter:

```python
container={
    "type": "skills",
    "skills": [
        {"type": "anthropic", "skill_id": "xlsx", "version": "latest"},
        {"type": "anthropic", "skill_id": "docx", "version": "latest"}
    ]
}
```

### Agent Integration

VT Code agent harness integrates skills through:

1. **CLI Module** (`src/cli/skills.rs`)

    - Handle `vtcode skills` commands
    - List, info, create, validate operations

2. **Runloop Module** (`src/agent/runloop/skills_commands.rs`)

    - Parse `/skills` slash commands
    - Load/unload skills in sessions
    - Execute skills with input

3. **Core Skills Module** (`vtcode-core/src/skills/`)
    - Skill discovery and loading
    - Manifest parsing (YAML + Markdown)
    - Skill execution and adaptation
    - Tool integration

---

## File Structure

```
.claude/skills/
 README.md                        # Skills directory guide
 spreadsheet-generator/
    SKILL.md
 doc-generator/
    SKILL.md
 pdf-report-generator/
    SKILL.md
 strict-architecture/
    SKILL.md
 bdd-workflow/
    SKILL.md
 code-orchestration/
    SKILL.md
 forensic-debugging/
     SKILL.md

docs/
 SKILLS_GUIDE.md                  # Complete skills guide
 AGENT_SKILLS_INTEGRATION.md      # Integration guide
 AGENT_SKILLS_QUICKREF.md         # Quick reference
 skills/
     SPREADSHEET_EXAMPLE.md
     WORD_DOCUMENT_EXAMPLE.md
     PDF_GENERATION_EXAMPLE.md

examples/
 skills_spreadsheet.py
 skills_word_document.py
 skills_pdf_generation.py
```

---

## Key Features

### 1. Skill Discovery

-   Searches `.claude/skills/`, `./skills/`, `~/.vtcode/skills/`
-   Automatic metadata extraction from SKILL.md
-   Progressive disclosure (metadata only at startup)

### 2. Skill Management

-   List available skills: `vtcode skills list`
-   Show details: `vtcode skills info <name>`
-   Create templates: `vtcode skills create <path>`
-   Validate manifest: `vtcode skills validate <path>`

### 3. Agent Integration

-   Skills available in system prompt
-   Agent recognizes when to use skills
-   Full instructions loaded on-demand
-   File outputs integrated into responses

### 4. Anthropic Agent Skills

-   Direct integration with xlsx, docx, pdf, pptx skills
-   Code execution for skill implementation
-   File handling via Files API
-   Progressive disclosure of skill metadata
-   Manifest controls: `allowed_tools`, `disable-model-invocation`, `when-to-use`, and explicit `requires-container`/`disallow-container` flags to reduce heuristic misclassification
-   Lean prompt now shows `dir + scope` only (no absolute paths); tokenizer-backed budgeting replaces char/4 estimates for eviction

---

## Command Reference

### Discovery

```bash
vtcode skills list              # List all skills
vtcode skills info <name>       # Show skill details
vtcode skills config            # Show search paths
```

### Management

```bash
vtcode skills create <path>     # Create skill template
vtcode skills validate <path>   # Validate SKILL.md
```

### Usage

```bash
vtcode ask "Use <skill> to..."      # Ask agent to use skill
vtcode chat                         # Interactive with /skills commands
/skills load <name>                 # Load skill in chat
/skills list                        # List in chat
/skills info <name>                 # Info in chat
/skills use <name> <input>          # Execute skill with input
```

---

## Best Practices

1. **Choose Right Skill**

    - Spreadsheets for data analysis
    - Word docs for collaborative content
    - PDFs for final distribution

2. **Leverage Progressive Disclosure**

    - Metadata loaded first (cheap)
    - Instructions on-demand
    - Minimal context overhead

3. **Error Handling**

    - Check code execution in responses
    - Verify file IDs
    - Handle API errors gracefully

4. **Performance**

    - Large documents may take longer
    - Optimize images/charts
    - Use summaries for 100+ pages

5. **Integration**
    - Combine multiple skills
    - Use code execution alongside
    - Chain for complex workflows

---

## Examples Location

```bash
# Run examples with API key
export ANTHROPIC_API_KEY=sk-...

python examples/skills_spreadsheet.py       # Create Excel
python examples/skills_word_document.py     # Create Word doc
python examples/skills_pdf_generation.py    # Generate PDF
```

---

## Documentation Navigation

-   **Quick Start**: `docs/AGENT_SKILLS_QUICKREF.md`
-   **Complete Guide**: `docs/AGENT_SKILLS_INTEGRATION.md`
-   **Skills Overview**: `.claude/skills/README.md`
-   **Comprehensive Guide**: `docs/SKILLS_GUIDE.md`
-   **Spreadsheet Examples**: `docs/skills/SPREADSHEET_EXAMPLE.md`
-   **Document Examples**: `docs/skills/WORD_DOCUMENT_EXAMPLE.md`
-   **PDF Examples**: `docs/skills/PDF_GENERATION_EXAMPLE.md`

---

## Next Steps

1. **Explore Skills**: `vtcode skills list`
2. **Learn Details**: `vtcode skills info spreadsheet-generator`
3. **Try Examples**: `python examples/skills_spreadsheet.py`
4. **Use in Chat**: `vtcode chat` then `/skills load spreadsheet-generator`
5. **Create Custom**: `vtcode skills create ~/.vtcode/skills/my-skill`
6. **Integrate**: Ask agent to use skills in tasks

---

## Support

For detailed information:

-   Quick reference: `docs/AGENT_SKILLS_QUICKREF.md`
-   Integration guide: `docs/AGENT_SKILLS_INTEGRATION.md`
-   Skills directory: `.claude/skills/README.md`
-   Examples: `examples/skills_*.py`
-   Anthropic docs: https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview
