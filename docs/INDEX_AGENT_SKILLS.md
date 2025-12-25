# VT Code Agent Skills Documentation Index

Complete reference for all Agent Skills documentation, examples, and guides.

## Quick Navigation

### I just want to...

-   **Use skills right now** → [Quick Reference](#quick-reference)
-   **Understand how skills work** → [Integration Guide](#integration-guide)
-   **See working code** → [Examples](#examples)
-   **Create a custom skill** → [Skills Guide](#skills-guide)
-   **Troubleshoot** → [Troubleshooting](#troubleshooting)

---

## Documentation Files

### Quick Reference

**File:** `docs/AGENT_SKILLS_QUICKREF.md`

-   One-page quick reference
-   CLI commands at a glance
-   Common tasks
-   Built-in skills table
-   Error quick fixes

**Start here if:** You want quick answers

### Integration Guide

**File:** `docs/AGENT_SKILLS_INTEGRATION.md`

-   Complete integration guide
-   How to use skills with VT Code agent
-   Agent integration points
-   Code examples (Rust and Python)
-   Workflow patterns
-   Best practices

**Start here if:** You need to understand how to use skills in the agent

### Complete Skills Guide

**File:** `docs/SKILLS_GUIDE.md`

-   Comprehensive skills documentation
-   Skill structure and creation
-   SKILL.md format and requirements
-   Progressive disclosure explained
-   File organization
-   Advanced topics

**Start here if:** You want to create or deeply understand skills

### Summary

**File:** `docs/AGENT_SKILLS_SUMMARY.md`

-   What was implemented
-   How to use skills
-   Architecture overview
-   File structure
-   Key features
-   Next steps

**Start here if:** You want an overview of the implementation

---

## Skills Directory

### Project Skills Location

**Directory:** `.claude/skills/`

Contains VTCode's project-specific skills:

-   `spreadsheet-generator/` - Excel/xlsx skill
-   `doc-generator/` - Word/docx skill
-   `pdf-report-generator/` - PDF skill
-   `strict-architecture/` - Code governance skill
-   `bdd-workflow/` - BDD/TDD workflow skill
-   `code-orchestration/` - Orchestration skill
-   `forensic-debugging/` - CRASH-RCA skill

### Skills Directory Guide

**File:** `.claude/skills/README.md`

-   All available skills documented
-   Usage patterns and examples
-   How to create new skills
-   Validation requirements
-   Tips and best practices

---

## Skill Examples

### Spreadsheet Generation

**File:** `docs/skills/SPREADSHEET_EXAMPLE.md`

-   Excel/xlsx skill examples
-   Use cases: financial reports, data analysis, inventory
-   Features: formulas, charts, formatting
-   Integration patterns

**Code:** `examples/skills_spreadsheet.py`

-   Create climate data spreadsheet
-   Create financial spreadsheet
-   Demonstrates code execution with xlsx skill

### Word Document Generation

**File:** `docs/skills/WORD_DOCUMENT_EXAMPLE.md`

-   Word/docx skill examples
-   Use cases: proposals, meeting minutes, technical docs
-   Features: formatting, tables, styles
-   Document types and best practices

**Code:** `examples/skills_word_document.py`

-   Create business report document
-   Create meeting minutes document
-   Demonstrates code execution with docx skill

### PDF Generation

**File:** `docs/skills/PDF_GENERATION_EXAMPLE.md`

-   PDF skill examples
-   Use cases: invoices, certificates, reports
-   Features: styling, charts, professional layout
-   Performance considerations

**Code:** `examples/skills_pdf_generation.py`

-   Generate invoice PDF
-   Generate data report PDF
-   Demonstrates code execution with pdf skill

---

## Examples

All examples are runnable with Anthropic API key:

```bash
export ANTHROPIC_API_KEY=sk-...
python examples/skills_spreadsheet.py
python examples/skills_word_document.py
python examples/skills_pdf_generation.py
```

### Example: Create Spreadsheet

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
        "skills": [{"type": "anthropic", "skill_id": "xlsx", "version": "latest"}]
    },
    betas=["code-execution-2025-08-25", "skills-2025-10-02"]
)
```

### Example: Use in VT Code Agent

```bash
vtcode ask "Create Excel spreadsheet with Q4 financial data"
```

---

## Built-in Skills

### Document Generation Skills

| Skill                   | Type           | Location                                | Use For                          |
| ----------------------- | -------------- | --------------------------------------- | -------------------------------- |
| `spreadsheet-generator` | Anthropic xlsx | `.claude/skills/spreadsheet-generator/` | Excel, dashboards, data analysis |
| `doc-generator`         | Anthropic docx | `.claude/skills/doc-generator/`         | Word docs, proposals, reports    |
| `pdf-report-generator`  | Anthropic pdf  | `.claude/skills/pdf-report-generator/`  | PDFs, invoices, certificates     |

### Development Skills

| Skill                 | Type            | Location                              | Use For                          |
| --------------------- | --------------- | ------------------------------------- | -------------------------------- |
| `strict-architecture` | Code Governance | `.claude/skills/strict-architecture/` | Architecture review, constraints |
| `bdd-workflow`        | Process         | `.claude/skills/bdd-workflow/`        | TDD/BDD feature development      |
| `code-orchestration`  | Process         | `.claude/skills/code-orchestration/`  | Orchestrated development         |
| `forensic-debugging`  | Process         | `.claude/skills/forensic-debugging/`  | Bug investigation                |

---

## How to Use Skills

### Method 1: CLI Commands

```bash
# List skills
vtcode skills list

# Show details
vtcode skills info spreadsheet-generator

# Create custom skill
vtcode skills create ~/.vtcode/skills/my-skill

# Validate skill
vtcode skills validate ./.claude/skills/spreadsheet-generator

# Show configuration
vtcode skills config
```

### Method 2: Ask Agent

```bash
vtcode ask "Create Excel spreadsheet with Q4 financial data"
```

### Method 3: Interactive Chat

```bash
vtcode chat
# Then: /skills load spreadsheet-generator
# Then: Ask agent to use it
```

### Method 4: Auto Mode

```bash
vtcode auto "Generate financial report with spreadsheet-generator"
```

---

## Common Tasks

### Create Financial Dashboard

```bash
vtcode ask "Use spreadsheet-generator to create Q4 financial dashboard"
```

**See:** `docs/skills/SPREADSHEET_EXAMPLE.md`
**Code:** `examples/skills_spreadsheet.py`

### Generate Project Proposal

```bash
vtcode ask "Use doc-generator to create project proposal document"
```

**See:** `docs/skills/WORD_DOCUMENT_EXAMPLE.md`
**Code:** `examples/skills_word_document.py`

### Create PDF Report

```bash
vtcode ask "Use pdf-report-generator to create quarterly report"
```

**See:** `docs/skills/PDF_GENERATION_EXAMPLE.md`
**Code:** `examples/skills_pdf_generation.py`

### Code Architecture Review

```bash
vtcode ask "Review this code using strict-architecture rules"
```

**See:** `.claude/skills/strict-architecture/SKILL.md`

---

## Troubleshooting

### Issue: Skill Not Found

**Solution:**

```bash
# Check available skills
vtcode skills list

# Check search paths
vtcode skills config
```

**Docs:** `docs/AGENT_SKILLS_INTEGRATION.md` → Troubleshooting

### Issue: Invalid SKILL.md

**Solution:**

```bash
# Validate manifest
vtcode skills validate <path>

# Requirements:
# - Valid YAML frontmatter
# - name: 1-64 chars, lowercase
# - description: 1-1024 chars
# - No "anthropic" or "claude" in name
```

**Docs:** `docs/AGENT_SKILLS_INTEGRATION.md` → Troubleshooting

### Issue: File Not Generated

**Solution:**

1. Check code execution errors in response
2. Verify skill is enabled in container
3. Ensure API key is valid

**Docs:** `docs/AGENT_SKILLS_INTEGRATION.md` → Troubleshooting

---

## Architecture Concepts

### Progressive Disclosure

Skills load in three levels to minimize context cost:

```
Level 1: Metadata (100 tokens)
  → Loaded at startup
  → Agent knows what skills do
  → No cost when unused

Level 2: Instructions (<5K tokens)
  → Loaded when skill is triggered
  → Full documentation on-demand
  → Used only during execution

Level 3: Resources (scripts, templates)
  → Executed without loading
  → Reference materials as needed
  → No context overhead
```

**Read:** `docs/SKILLS_GUIDE.md` → Advanced Topics

### Skill Container

Skills are passed to Claude via the container parameter:

```python
container={
    "type": "skills",
    "skills": [
        {"type": "anthropic", "skill_id": "xlsx", "version": "latest"}
    ]
}
```

**Read:** `docs/AGENT_SKILLS_INTEGRATION.md` → Skill Architecture

---

## Best Practices

1. **Choose Right Skill**

    - Spreadsheets for data/analysis
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

4. **Integration**
    - Combine multiple skills
    - Use code execution alongside
    - Chain for complex workflows

**Read:** `docs/AGENT_SKILLS_INTEGRATION.md` → Best Practices

---

## File Structure

```
VTCode/
 .claude/skills/                          # Project skills
    README.md                            # Skills overview
    spreadsheet-generator/
       SKILL.md
    doc-generator/
       SKILL.md
    pdf-report-generator/
       SKILL.md
    [other skills]/

 docs/
    AGENT_SKILLS_INTEGRATION.md          # Integration guide
    AGENT_SKILLS_QUICKREF.md             # Quick reference
    AGENT_SKILLS_SUMMARY.md              # Implementation summary
    SKILLS_GUIDE.md                      # Complete skills guide
    INDEX_AGENT_SKILLS.md                # This file
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

## Related Documentation

-   `.claude/CLAUDE.md` - Claude Code configuration
-   `README.md` - Project overview
-   `CONTRIBUTING.md` - Contribution guidelines
-   `.claude/skills/README.md` - Skills directory guide

---

## Resources

### Official Documentation

-   [Anthropic Agent Skills Overview](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview)
-   [Agent Skills Quickstart](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/quickstart)
-   [Agent Skills API Guide](https://platform.claude.com/docs/en/build-with-claude/skills-guide)
-   [Skills Cookbook](https://github.com/anthropics/claude-cookbooks/tree/main/skills)

### VT Code Implementation

-   `vtcode-core/src/skills/` - Core skills module
-   `src/cli/skills.rs` - CLI command handlers
-   `src/agent/runloop/skills_commands.rs` - Agent integration

---

## Next Steps

1. **Understand Skills** → Read `docs/AGENT_SKILLS_INTEGRATION.md`
2. **Quick Reference** → Bookmark `docs/AGENT_SKILLS_QUICKREF.md`
3. **Try Examples** → Run `python examples/skills_spreadsheet.py`
4. **Use Skills** → Try `vtcode ask "Use spreadsheet-generator to..."`
5. **Create Skills** → Follow `docs/SKILLS_GUIDE.md`
6. **Integrate** → See `.claude/skills/README.md`

---

## Support

For help:

1. Check relevant documentation file (see above)
2. Review examples in `examples/`
3. Check troubleshooting section
4. See Anthropic API docs: https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview

---

**Last Updated:** December 13, 2024
**VT Code Agent Skills Implementation**
