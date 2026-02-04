# VTCode Skills Directory

This directory contains VTCode skills following the enhanced location system, compatible with pi-mono, Claude Code, and Codex CLI formats.

## Skill Location System

VTCode now supports multiple skill locations with proper precedence:

### Location Precedence (Highest to Lowest)

1. **VTCode User**: `~/.vtcode/skills/**` (recursive, colon-separated names)
2. **VTCode Project**: `./.vtcode/skills/**` (recursive, colon-separated names)  
3. **Pi User**: `~/.pi/agent/skills/**` (recursive, colon-separated names)
4. **Pi Project**: `./.pi/skills/**` (recursive, colon-separated names)
5. **Claude Code User**: `~/.claude/skills/*` (one-level only)
6. **Claude Code Project**: `./.claude/skills/*` (one-level only)
7. **Codex CLI User**: `~/.codex/skills/**` (recursive)

### Name Formatting

- **Recursive locations** (VTCode, Pi, Codex): Use colon (`:`) as separator
  - Example: `web:tools:search-engine` for `web/tools/search-engine/`
- **One-level locations** (Claude Code): Use directory name as-is
  - Example: `search-engine` for `search-engine/`

### Migration Notes

Skills have been migrated from `.claude/skills` to `.vtcode/skills` to take advantage of:
- Higher precedence in the location system
- Recursive directory support
- Colon-separated naming for nested skills
- Better integration with VTCode's enhanced skills system

## Available Skills

### Document Generation

#### spreadsheet-generator
**v1.0.0** | VTCode Team

Generate professional Excel spreadsheets with data, charts, and formatting.

**Features:**
- Multiple sheets with complex layouts
- Data formulas (SUM, AVERAGE, VLOOKUP, etc.)
- Charts and visualizations
- Professional formatting and styling

#### doc-generator
**v1.0.0** | VTCode Team

Generate professional Word documents with formatted text, tables, and layouts.

**Features:**
- Rich text formatting
- Heading styles and hierarchy
- Tables with merged cells
- Lists and professional layout

#### pdf-report-generator
**v1.0.0** | VTCode Team

Generate professional PDF documents with charts, styling, and complex layouts.

**Features:**
- Advanced styling and custom fonts
- Headers, footers, and page numbers
- Charts and data visualizations
- Professional branding support

### Development Process

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

#### code-orchestration
**v1.0.0** | VTCode Team

Orchestrated development with automatic task breakdown and delegation.

**Includes:**
- Task decomposition
- Delegated implementation
- Coding standards enforcement
- Automated testing
- Quality gates

#### forensic-debugging
**v1.0.0** | VTCode Team

CRASH-RCA forensic debugging for systematic bug investigation.

**Workflow:**
1. Initialize session with issue description
2. Log hypotheses with confidence levels
3. Gather evidence with read-only tools
4. Generate structured RCA report

#### strict-architecture
**v0.0.1** | Anthropic

Enforces universal strict governance rules for Python, Golang, and .NET codebases.

**Constraints:**
- 500 lines per file maximum
- 5 functions per file maximum
- 4 function arguments maximum
- Interface-first I/O design

## Usage

### Direct Skill Loading
```rust
use vtcode_core::skills::{EnhancedSkillLoader, SkillLocations};

let loader = EnhancedSkillLoader::new(workspace_root);
let skill = loader.get_skill("spreadsheet-generator").await?;
```

### Discovery and Listing
```rust
let discovery_result = loader.discover_all_skills().await?;
for skill in discovery_result.traditional_skills {
    println!("Found skill: {}", skill.manifest().name);
}
```

### Template-Based Skill Creation
```rust
use vtcode_core::skills::{TemplateEngine, TemplateType};

let engine = TemplateEngine::new();
let mut variables = HashMap::new();
variables.insert("skill_name".to_string(), "my-custom-skill".to_string());
variables.insert("description".to_string(), "My custom skill description".to_string());

let skill_path = engine.generate_skill("traditional", variables, output_dir)?;
```

## Skill Structure

Each skill follows the standard format:

```
skill-name/
 SKILL.md              # Metadata (YAML) + Instructions (Markdown)
 ADVANCED.md           # Optional: Advanced documentation  
 scripts/              # Optional: Executable scripts
    helper.py
 templates/            # Optional: Reference templates
     example.json
```

### SKILL.md Template

```yaml
---
name: skill-name
description: Brief description and when to use it
version: 1.0.0
author: Your Name
---

# Skill Name

## Instructions
[Step-by-step guidance for the agent]

## Examples
- Example 1
- Example 2

## Best Practices
[Usage guidelines and tips]
```

## Creating New Skills

### Method 1: Manual Creation
```bash
mkdir -p ~/.vtcode/skills/my-skill
cat > ~/.vtcode/skills/my-skill/SKILL.md << 'EOF'
---
name: my-skill
description: My custom skill for specific tasks
version: 1.0.0
author: Your Name
---

# My Custom Skill

## Instructions
[Your instructions here]
EOF
```

### Method 2: Template Generation
```rust
use vtcode_core::skills::{TemplateEngine, TemplateType};

let engine = TemplateEngine::new();
let mut variables = HashMap::new();
variables.insert("skill_name".to_string(), "my-skill".to_string());
variables.insert("description".to_string(), "My custom skill".to_string());

let skill_path = engine.generate_skill("traditional", variables, 
                                      PathBuf::from("~/.vtcode/skills"))?;
```

## Validation

Validate your skills before use:

```rust
use vtcode_core::skills::{SkillValidator, ValidationConfig};

let mut validator = SkillValidator::new();
let report = validator.validate_skill_directory(skill_path).await?;

if report.status == ValidationStatus::Valid {
    println!("Skill is valid!");
} else {
    println!("Validation issues: {:?}", report.recommendations);
}
```

## Migration from Other Tools

### From Claude Code
Skills in `.claude/skills/*` are automatically discovered with one-level scanning.

### From Pi
Skills in `.pi/skills/**` are discovered recursively with colon-separated names.

### From Codex CLI  
Skills in `.codex/skills/**` are discovered recursively with path-based names.

## Tips

1. **Use VTCode locations** for highest precedence and best integration
2. **Follow naming conventions** for your chosen location type
3. **Validate skills** before deployment
4. **Test thoroughly** with the streaming execution system
5. **Document clearly** with comprehensive instructions
6. **Version your skills** for better tracking and updates

## Examples

See the `examples/` directory for complete usage examples of the enhanced skills system.