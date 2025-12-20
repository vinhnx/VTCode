# VT Code Agent Skills Standard Implementation

This document explains how VT Code implements and complies with the [open Agent Skills standard](http://agentskills.io/).

## What is Agent Skills Standard?

The [Agent Skills standard](http://agentskills.io/) is an open, portable format for giving agents new capabilities and expertise. It was developed by Anthropic and released as an open standard, now supported by leading AI development tools including VT Code.

**Benefits of the standard:**

-   **Portability** - Skills work across multiple tools and agents
-   **Reusability** - Build once, deploy everywhere
-   **Consistency** - Common format understood by all implementations
-   **Interoperability** - Standardized skill discovery and loading

## VT Code's Implementation

### Compliance

VT Code fully complies with the [Agent Skills specification](http://agentskills.io/specification). All skills created in or used by VT Code follow the standard format:

```
my-skill/
  SKILL.md                    # Required: Metadata + Instructions
  scripts/                    # Optional: Executables
  references/                 # Optional: Detailed reference docs
  assets/                     # Optional: Templates, images, data
```

### SKILL.md Format

Every VT Code skill must have a valid `SKILL.md` file with:

**Frontmatter (YAML)**:
```yaml
---
name: my-skill               # Required: lowercase, hyphens, max 64 chars
description: What it does    # Required: 1-1024 chars
version: 1.0.0               # Optional: semantic versioning
author: Your Name            # Optional: creator name
license: MIT                 # Optional: license type or file
compatibility: Product info  # Optional: environment requirements
metadata:                    # Optional: custom key-value pairs
  category: productivity
---
```

**Body (Markdown)**:
- Step-by-step instructions
- Examples of usage
- Edge cases and limitations

See the full [specification](http://agentskills.io/specification) for details.

### Features Aligned with Standard

#### 1. Progressive Disclosure

VT Code loads skills efficiently in three levels:

| Level | Content | Context Cost | When Loaded |
|-------|---------|---------------|-------------|
| **Metadata** | Name, description | ~100 tokens | Always available |
| **Instructions** | Full SKILL.md body | <5K tokens | When skill activated |
| **Resources** | Scripts, templates, assets | On-demand | Only when needed |

This approach maximizes context efficiency while keeping agents informed about available capabilities.

#### 2. Multi-Location Discovery

VT Code discovers skills from multiple locations with precedence:

```
1. ~/.vtcode/skills/           (VT Code user skills - highest)
2. .vtcode/skills/             (Project-specific)
3. ~/.pi/skills/               (Pi framework)
4. .pi/skills/
5. ~/.claude/skills/           (Claude Code legacy)
6. .claude/skills/
7. ~/.codex/skills/            (Codex CLI - lowest)
```

Higher precedence locations override same-named skills.

#### 3. Validation

VT Code validates all skills against the standard:

```bash
vtcode skills validate ./my-skill
```

Validates:
- SKILL.md exists
- Valid YAML frontmatter
- Name follows constraints (lowercase, hyphens, 1-64 chars)
- Description is 1-1024 characters
- No reserved names ("anthropic", "claude")
- No reserved special characters

#### 4. Standardized Metadata

VT Code stores and manages standardized skill metadata:

```bash
vtcode skills info code-review
```

Displays:
- Name and description
- Version and author
- License and compatibility info
- Full instructions and examples

## Creating Standard-Compliant Skills

### Quick Start

```bash
# Create a new skill from template
vtcode skills create ~/.vtcode/skills/my-skill

# Validate your skill
vtcode skills validate ~/.vtcode/skills/my-skill

# View skill details
vtcode skills info my-skill
```

### Example: Code Review Skill

```yaml
---
name: code-review
description: Performs comprehensive code reviews checking for security vulnerabilities, performance bottlenecks, and code quality issues
version: 1.0.0
author: Your Team
license: MIT
---

# Code Review Skill

## Instructions

When asked to review code:

1. **Security Analysis**
   - Identify injection vulnerabilities
   - Check authentication/authorization
   - Review cryptographic usage

2. **Performance Review**
   - Find algorithmic bottlenecks
   - Identify resource leaks
   - Suggest optimization opportunities

3. **Code Quality**
   - Check consistency and style
   - Evaluate error handling
   - Assess test coverage

## Examples

**Input**: Python function vulnerable to SQL injection
**Output**: Detailed explanation with fixed code

**Input**: JavaScript component with memory leaks
**Output**: Analysis and refactored version

## Guidelines

- Provide specific, actionable feedback
- Include code examples
- Suggest concrete improvements
- Link to relevant docs
```

## Supported Optional Fields

VT Code recognizes all standard optional metadata fields:

| Field | Purpose | Max Length |
|-------|---------|-----------|
| `version` | Semantic versioning | - |
| `author` | Skill creator | - |
| `license` | License type or file | - |
| `compatibility` | Environment requirements | 500 chars |
| `metadata` | Custom key-value pairs | - |
| `allowed-tools` | Pre-approved tools (experimental) | - |

### Using License Field

```yaml
---
name: my-skill
license: MIT
# OR
license: LICENSE.txt
---
```

### Using Compatibility Field

```yaml
---
name: data-analysis
compatibility: Requires Python 3.9+, pandas, and numpy libraries
---
```

## Directory Structure Best Practices

### Minimal Skill

```
my-skill/
  SKILL.md
```

### Well-Organized Skill

```
my-skill/
  SKILL.md
  scripts/
    validate.py
    process.py
  references/
    ADVANCED.md
    EXAMPLES.md
  assets/
    template.json
    schema.md
```

### Large Skill

```
my-skill/
  SKILL.md
  references/
    REFERENCE.md
    TROUBLESHOOTING.md
    API_GUIDE.md
  scripts/
    validate/
    process/
    format/
  assets/
    templates/
    examples/
    data/
```

Keep `SKILL.md` under 500 lines. Move detailed content to reference files.

## Interoperability

Skills created in VT Code are immediately compatible with:

- **OpenCode** - Code assistant platform
- **Cursor** - AI code editor
- **Amp** - Agent framework
- **Letta** - Agent operating system
- **Goose** - Automation agent
- **VS Code** - With Agent Skills extension
- **Claude** - With CodeWiki integration

This means your skills can be shared across your entire AI agent ecosystem.

## Validation and Testing

### Validate Syntax

```bash
vtcode skills validate ~/path/to/skill
```

Checks:
- SKILL.md exists and is readable
- Valid YAML frontmatter
- Required fields present
- Field constraints met

### Test Discovery

```bash
# List all discovered skills
vtcode skills list

# Show where skills are found
vtcode skills config

# Get full info on a skill
vtcode skills info my-skill
```

### Validate Naming

Valid names:
- `code-review`
- `my-skill`
- `python-refactoring`
- `a` (single character)

Invalid names:
- `code_review` (underscore)
- `Code-Review` (uppercase)
- `my--skill` (consecutive hyphens)
- `-my-skill` (starts with hyphen)
- `anthropic-pdf` (reserved prefix)

## Contributing to the Standard

The Agent Skills standard is open to contributions. The standard is maintained at:

- **GitHub**: https://github.com/agentskills/agentskills
- **Specification**: http://agentskills.io/specification
- **Discussion**: https://github.com/agentskills/agentskills/discussions

To propose changes or additions:

1. Review the current specification
2. Open an issue describing your proposal
3. Engage with the community
4. Submit a pull request

## Resources

### Official Resources

- **Agent Skills Home**: http://agentskills.io/
- **What are Skills?**: http://agentskills.io/what-are-skills
- **Specification**: http://agentskills.io/specification
- **Integration Guide**: http://agentskills.io/integrate-skills
- **GitHub**: https://github.com/agentskills/agentskills

### VT Code Resources

- **Skills Guide**: [SKILLS_GUIDE.md](./SKILLS_GUIDE.md)
- **Authoring Guide**: [SKILL_AUTHORING_GUIDE.md](./SKILL_AUTHORING_GUIDE.md)
- **Example Skills**: `/skills` directory
- **Implementation**: `vtcode-core/src/skills/`

## FAQ

**Q: Are my VT Code skills compatible with other tools?**

A: Yes. Skills following the Agent Skills standard are portable across all compatible tools.

**Q: Can I use pre-built Anthropic skills?**

A: Yes. Anthropic provides built-in skills (PDF, Excel, Word, PowerPoint) available via future marketplace integration.

**Q: What if I need a skill that references other files?**

A: Use the `references/` directory for additional documentation and `assets/` for templates/data. Reference them with relative paths from `SKILL.md`.

**Q: Can skills run code?**

A: Yes. Include executable scripts in the `scripts/` directory. Agents can execute them directly.

**Q: How do I ensure my skills are valid?**

A: Run `vtcode skills validate` to check compliance with the standard.

## See Also

- [SKILLS_GUIDE.md](./SKILLS_GUIDE.md) - Comprehensive user guide
- [SKILL_AUTHORING_GUIDE.md](./SKILL_AUTHORING_GUIDE.md) - Detailed authoring instructions
- [ARCHITECTURE.md](./ARCHITECTURE.md) - Technical architecture
