# Skill Authoring Guide

VT Code authoring targets the strict Agent Skills `SKILL.md` format.

## Create a Skill

```bash
vtcode skills create my-skill
```

By default this creates:

```text
.agents/skills/my-skill/
├── SKILL.md
├── scripts/
├── references/
└── assets/
```

## Write `SKILL.md`

Use only the supported frontmatter fields:

```yaml
---
name: my-skill
description: Explain what this skill does and when to use it.
license: Apache-2.0
compatibility: Requires git and network access
allowed-tools: Read Write Bash
metadata:
  owner: platform-team
---
```

Then write the workflow in the Markdown body.

Recommended structure:

```markdown
# My Skill

## Purpose

Summarize the workflow and outcome.

## Workflow

1. Confirm the request matches the description.
2. Keep core instructions here.
3. Move large detail to `references/`.
4. Reuse scripts or assets when possible.

## Resources

- `scripts/`
- `references/`
- `assets/`
```

## Validation

```bash
vtcode skills validate ./.agents/skills/my-skill
```

Validation fails if `SKILL.md` contains unsupported fields such as `version`, `author`, `when-to-use`, `when-not-to-use`, `model`, `mode`, `context`, `agent`, `network`, or `permissions`.

## Routing Guidance

Write the `description` as routing logic:

- what the skill does
- when it should trigger
- what result it produces

Good:

```yaml
description: Extract text and tables from PDF files. Use when the request involves PDFs, forms, or document extraction.
```

Bad:

```yaml
description: Helps with documents.
```

## Non-Support

- No `agents/openai.yaml`
- No legacy VT Code skill frontmatter
- No deprecated project or user skill locations
