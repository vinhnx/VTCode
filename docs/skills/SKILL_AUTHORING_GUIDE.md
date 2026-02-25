# Skill Authoring Guide for VT Code

VT Code now includes comprehensive skill authoring capabilities following Anthropic's Agent Skills specification.

## Overview

Skills are modular packages that extend VT Code's capabilities by providing:

-   Specialized workflows and domain expertise
-   Tool integrations and file format handlers
-   Company-specific knowledge and procedures
-   Reusable scripts, templates, and resources

## Quick Start

### Create a New Skill

```bash
# In chat
/skills create my-skill

# With custom output directory
/skills create my-skill --path ./custom/skills/

# Creates:
# skills/my-skill/
# ├── SKILL.md
# ├── scripts/
# │   └── example.py
# ├── references/
# │   └── api_reference.md
# └── assets/
#     └── .gitkeep
```

### Edit the Skill

1. **Update SKILL.md frontmatter**:

```yaml
---
name: my-skill
description: Complete description of what the skill does and WHEN to use it. Include specific triggers: file types, keywords, tasks that should activate this skill.
---
```

2. **Write instructions in the body**:

```markdown
# My Skill

## Overview

Brief explanation of what this skill enables.

## Quick Start

Simple example of using this skill.

## Advanced Usage

Detailed workflows and patterns.
```

3. **Add resources**:

-   **scripts/**: Executable Python/Bash scripts for deterministic operations
-   **references/**: Documentation loaded as needed (schemas, API docs, guides)
-   **assets/**: Templates, images, fonts used in output

### Validate the Skill

```bash
/skills validate my-skill
```

Output:

```
Validation Report for: skills/my-skill
Status: ✓ VALID

Warnings:
  ⚠ Description needs to be completed
```

### Package the Skill

```bash
/skills package my-skill
```

Creates `my-skill.skill` (zip format) ready for distribution.

## Skill Structure

### Required: SKILL.md

Every skill must have a `SKILL.md` file with:

**YAML Frontmatter** (required fields):

-   `name`: lowercase-with-hyphens (max 64 chars)
-   `description`: what it does + when to use it (max 1024 chars)

**Optional fields**:

-   `license`: License terms
-   `allowed-tools`: Pre-approved tools (for VT Code compatibility)
-   `metadata`: Custom key-value pairs

**Markdown Body**:

-   Instructions for using the skill
-   Workflows and patterns
-   References to bundled resources

### Optional: Bundled Resources

```
my-skill/
├── SKILL.md              # Required
├── scripts/              # Executable code
│   ├── process.py
│   └── validate.sh
├── references/           # Documentation
│   ├── api_docs.md
│   └── schemas.md
└── assets/               # Output resources
    ├── template.pptx
    └── logo.png
```

**When to include**:

**scripts/**:

-   Deterministic operations (validation, data processing)
-   Token-efficient (executed, not loaded into context)
-   Example: `scripts/analyze_pdf.py`

**references/**:

-   Documentation loaded as needed
-   API specs, schemas, workflow guides
-   Example: `references/database_schema.md`

**assets/**:

-   Files used in output (not loaded into context)
-   Templates, images, icons, fonts
-   Example: `assets/report_template.docx`

## Best Practices

### 1. Write Effective Descriptions

The description is **critical** for skill discovery. Claude uses it to decide when to load the skill.

**Good**:

```yaml
description: Extract text and tables from PDF files, fill forms, merge documents. Use when working with PDF files or when the user mentions PDFs, forms, or document extraction.
```

**Bad**:

```yaml
description: Helps with documents
```

**Include**:

-   What the skill does (actions: extract, generate, analyze)
-   When to use it (triggers: file types, keywords, scenarios)
-   Specific capabilities (tables, forms, charts)

### 2. Keep SKILL.md Concise

-   Target <500 lines for SKILL.md body
-   Move detailed content to `references/` files
-   Use progressive disclosure: link to details instead of including them

**Example**:

````markdown
## PDF Form Filling

For comprehensive form filling guide, see [references/forms.md](references/forms.md).

Quick example:

```python
python scripts/fill_form.py input.pdf data.json output.pdf
```
````

### 3. Provide Executable Scripts

Include scripts for:

-   Repetitive operations (avoid re-generating similar code)
-   Fragile operations (complex validation, precise formatting)
-   Token-heavy tasks (processing large files, batch operations)

**Example**: `scripts/validate_pdf.py`

```python
#!/usr/bin/env python3
"""Validate PDF form fields before filling"""

import sys
import json

def validate_fields(pdf_path, fields_json):
    # Deterministic validation logic
    pass

if __name__ == "__main__":
    validate_fields(sys.argv[1], sys.argv[2])
```

### 4. Use Progressive Disclosure

Organize by specificity:

**Level 1** (SKILL.md frontmatter): Name + description (always loaded)
**Level 2** (SKILL.md body): Core workflows + references to details
**Level 3** (references/): Detailed docs, loaded only when needed

**Pattern**:

```markdown
# BigQuery Analysis

## Quick Start

[Basic query example]

## Available Datasets

**Finance**: Revenue, billing → See [references/finance.md](references/finance.md)
**Sales**: Pipeline, accounts → See [references/sales.md](references/sales.md)
**Product**: Usage, features → See [references/product.md](references/product.md)
```

Claude loads only the relevant reference file (`finance.md` for revenue questions).

### 5. Avoid Common Mistakes

❌ **Don't include**:

-   README.md (use SKILL.md only)
-   INSTALLATION_GUIDE.md (include in SKILL.md)
-   Windows-style paths (`dir\file.txt` → use `dir/file.txt`)
-   Reserved words in name (`anthropic-*`, `claude-*`)

✓ **Do include**:

-   Clear, specific descriptions with triggers
-   Forward slashes in all paths
-   Executable permissions on scripts (`chmod +x scripts/*.py`)
-   Validation before packaging

## Skill Naming Conventions

Use **gerund form** (verb + -ing) for clarity:

✓ Good:

-   `processing-pdfs`
-   `analyzing-spreadsheets`
-   `managing-databases`

✓ Acceptable:

-   `pdf-processing`
-   `spreadsheet-analysis`

❌ Avoid:

-   `helper`, `utils`, `tools` (too vague)
-   `My-Skill` (uppercase not allowed)
-   `my_skill` (underscores not allowed)

## Validation Rules

The validator checks:

**Name**:

-   Lowercase alphanumeric + hyphens only
-   Max 64 characters
-   No `anthropic` or `claude` keywords

**Description**:

-   Non-empty, max 1024 characters
-   Should not contain `[TODO]`
-   Should include "when to use" information

**Structure**:

-   SKILL.md exists and has valid frontmatter
-   No unnecessary files (README.md, etc.)
-   Forward slashes in paths
-   Body <50k characters (warning if exceeded)

**Output**:

```
Validation Report for: skills/my-skill
Status: ✓ VALID

Warnings:
  ⚠ SKILL.md body is very long (>50k chars). Consider splitting into reference files.
```

## Skill Lifecycle

### 1. Development

```bash
# Create from template
/skills create pdf-analyzer

# Edit SKILL.md and add resources
# (Use your editor)

# Validate frequently
/skills validate pdf-analyzer
```

### 2. Testing

Load the skill and test with real requests:

```bash
/skills load pdf-analyzer
```

Then: "Extract tables from report.pdf"

Observe:

-   Does the skill activate correctly?
-   Are instructions clear?
-   Are bundled resources accessible?

### 3. Iteration

Based on usage:

-   Refine description triggers
-   Add missing scripts/references
-   Improve workflows
-   Update examples

### 4. Distribution

```bash
# Package for sharing
/skills package pdf-analyzer

# Creates: pdf-analyzer.skill
# Share with team or upload to skill repository
```

## Example: PDF Analysis Skill

Complete example following best practices:

**Directory structure**:

```
skills/pdf-analyzer/
├── SKILL.md
├── scripts/
│   ├── extract_text.py
│   ├── extract_tables.py
│   └── validate_pdf.py
├── references/
│   ├── api_docs.md
│   └── examples.md
└── assets/
    └── report_template.md
```

**SKILL.md**:

````yaml
---
name: pdf-analyzer
description: Extract text, tables, and metadata from PDF documents. Use when analyzing PDF files, extracting data from reports, or when the user mentions PDF processing, document extraction, or table extraction.
---

# PDF Analyzer

## Overview

Comprehensive PDF analysis toolkit for extracting text, tables, and metadata.

## Quick Start

Extract text:
```bash
python scripts/extract_text.py input.pdf output.txt
```

Extract tables:
```bash
python scripts/extract_tables.py input.pdf tables.json
```

## Workflows

### 1. Text Extraction
1. Validate PDF: `scripts/validate_pdf.py input.pdf`
2. Extract: `scripts/extract_text.py input.pdf output.txt`
3. Verify output

### 2. Table Extraction
1. Analyze structure: `scripts/extract_tables.py --analyze input.pdf`
2. Extract tables: `scripts/extract_tables.py input.pdf output.json`
3. Format as needed

## References

- **API Documentation**: [references/api_docs.md](references/api_docs.md)
- **Examples**: [references/examples.md](references/examples.md)

## Templates

Report template available in `assets/report_template.md`.
````

## Integration with VT Code

### Auto-Trigger with `$SkillName`

Skills can be auto-loaded when mentioned:

```
User: Use $pdf-analyzer to extract tables from report.pdf
→ Auto-loads pdf-analyzer skill
→ Executes with skill context
```

See [SKILL_MENTION_DETECTION.md](SKILL_MENTION_DETECTION.md) for details.

### Progressive Loading

VT Code loads skills progressively:

1. **Startup**: Load all skill names + descriptions
2. **Triggered**: Load SKILL.md body when skill is relevant
3. **On-demand**: Load references/scripts only when accessed

This minimizes token usage while keeping all skills discoverable.

## Command Reference

```bash
# Create skill
/skills create <name> [--path <dir>]

# List available skills
/skills list

# Validate skill
/skills validate <name>

# Package skill
/skills package <name>

# Load skill into session
/skills load <name>

# Show skill details
/skills info <name>

# Execute skill with input
/skills use <name> <input>
```

## Related Documentation

-   [Codex Skills Improvements](CODEX_SKILLS_IMPROVEMENTS.md) - Lean rendering mode
-   [Skill Mention Detection](SKILL_MENTION_DETECTION.md) - Auto-trigger patterns
-   [Agent Skills Integration](AGENT_SKILLS_INTEGRATION.md) - Architecture overview
-   [Anthropic Skills Spec](https://github.com/anthropics/skills/blob/main/spec/agent-skills-spec.md) - Official specification

## Troubleshooting

### Validation Fails

**Problem**: "Invalid skill name"
**Solution**: Use lowercase letters, numbers, hyphens only. No `anthropic` or `claude`.

**Problem**: "Description needs to be completed"
**Solution**: Replace `[TODO]` placeholders with actual description including triggers.

### Skill Not Auto-Triggering

**Problem**: Skill doesn't load when expected
**Solution**: Improve description with specific keywords and "when to use" scenarios.

**Problem**: Wrong skill triggers
**Solution**: Make description more specific to avoid overlapping with other skills.

### Packaging Fails

**Problem**: "SKILL.md not found"
**Solution**: Ensure SKILL.md exists in skill directory root.

**Problem**: "Validation failed"
**Solution**: Run `/skills validate <name>` first and fix errors.

---

**Implementation Status**: ✅ Complete (December 2024)
**Specification**: Anthropic Agent Skills v1.0
**VT Code Version**: 0.49.5+
