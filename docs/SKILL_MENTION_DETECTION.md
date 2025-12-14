# Skill Mention Detection in Chat

## Overview

VT Code now supports **Codex-style skill auto-triggering** through mention detection in chat messages. Users can invoke skills naturally without explicit `/skills` commands.

## Syntax Patterns

### 1. Explicit Mention (`$skill-name`)

Use `$` prefix followed by skill name to explicitly trigger a skill:

```
User: Use $pdf-analyzer to process the document
→ Auto-loads and invokes pdf-analyzer skill
```

```
User: Can you $spreadsheet-generator create a report?
→ Auto-loads and invokes spreadsheet-generator skill
```

**Case-insensitive**: `$PDF-ANALYZER`, `$pdf-analyzer`, and `$Pdf-Analyzer` all work.

### 2. Description Keyword Matching

Skills auto-trigger when user input contains multiple keywords from the skill's description (requires 2+ matches):

```
# Skill: pdf-analyzer
# Description: "Extract text and tables from PDF documents"

User: Extract tables from this PDF document
→ Keywords match: "Extract", "tables", "PDF"
→ Auto-triggers pdf-analyzer
```

```
# Skill: spreadsheet-generator
# Description: "Generate Excel spreadsheets with data analysis"

User: Create a spreadsheet with data analysis for the report
→ Keywords match: "spreadsheet", "data", "analysis"
→ Auto-triggers spreadsheet-generator
```

## Implementation

### For Chat Integration

```rust
use vtcode_core::agent::runloop::detect_mentioned_skills;
use std::path::PathBuf;

async fn process_user_input(input: &str, workspace: PathBuf) {
    // Detect skills mentioned in user input
    match detect_mentioned_skills(input, workspace).await {
        Ok(skills) => {
            for (name, skill) in skills {
                println!("Auto-triggering skill: {}", name);
                // Load skill context into conversation
                // Add skill instructions to system prompt
                // Execute skill with user input
            }
        }
        Err(e) => {
            eprintln!("Skill detection error: {}", e);
        }
    }
}
```

### Integration with Slash Commands

The `/skills` command still works for explicit control:

```bash
# Explicit commands (still supported)
/skills list                    # List all available skills
/skills load pdf-analyzer       # Load specific skill
/skills info pdf-analyzer       # Show skill details
/skills use pdf-analyzer <input>  # Execute skill with input

# Auto-trigger (new feature)
Use $pdf-analyzer to process this  # Same as /skills use
```

## Configuration

Enable/disable auto-trigger in `vtcode.toml`:

```toml
[skills]
render-mode = "lean"              # "lean" | "full"
enable-auto-trigger = true        # Enable $skill-name detection
enable-description-matching = true # Enable keyword matching
min-keyword-matches = 2           # Minimum keywords for match
```

## Examples

### Example 1: PDF Processing

```
User: Use $pdf-analyzer to extract tables from report.pdf

Agent detects:
  - Explicit mention: $pdf-analyzer
  - Loads pdf-analyzer skill
  - Injects skill instructions into conversation
  - Processes request with skill context
```

### Example 2: Spreadsheet Generation

```
User: Generate a spreadsheet with sales data analysis

Agent detects:
  - Keywords: "spreadsheet", "data", "analysis" (3 matches)
  - Loads spreadsheet-generator skill (description contains these keywords)
  - Executes skill with user input
```

### Example 3: Multiple Skills

```
User: Extract data from PDF and create Excel spreadsheet

Agent detects:
  - Keywords match pdf-analyzer: "Extract", "data", "PDF"
  - Keywords match spreadsheet-generator: "create", "Excel", "spreadsheet"
  - Loads both skills
  - Coordinates execution (PDF → Spreadsheet pipeline)
```

## Skill Authoring Best Practices

To optimize for auto-trigger detection:

### 1. Write Descriptive Descriptions

**Good**:

```yaml
---
name: pdf-report-generator
description: Generate professional PDF reports with charts and tables from data
---
```

**Better for Detection**:

```yaml
---
name: pdf-report-generator
description: Create generate professional PDF report documents with data visualization charts tables analysis
---
```

Include key terms users might naturally say when requesting this functionality.

### 2. Use Action-Oriented Language

Focus on verbs and nouns users would use:

-   "Extract", "Generate", "Create", "Analyze", "Process"
-   "PDF", "Excel", "spreadsheet", "document", "report"
-   "data", "tables", "charts", "text"

### 3. Test with Natural Phrases

Think about how users would naturally request the skill:

```yaml
# Skill: code-review-assistant
# Description: "Review code analyze quality suggest improvements detect bugs"

Test phrases:
✅ "Review this code for bugs"
✅ "Analyze code quality"
✅ "Suggest improvements to the code"
```

## API Reference

### `detect_mentioned_skills()`

```rust
pub async fn detect_mentioned_skills(
    user_input: &str,
    workspace: PathBuf,
) -> Result<Vec<(String, Skill)>>
```

**Parameters**:

-   `user_input`: User's chat message to analyze
-   `workspace`: Workspace path for skill discovery

**Returns**: `Vec<(String, Skill)>` - List of (skill_name, skill) pairs detected

**Detection Logic**:

1. **Explicit Pattern**: `$skill-name` (case-insensitive)
2. **Keyword Pattern**: 2+ description keywords match (4+ chars each)

### Integration Points

**Before LLM Call**:

```rust
// Detect skills in user input
let mentioned_skills = detect_mentioned_skills(&user_input, workspace).await?;

// Inject skill instructions into system prompt
for (_name, skill) in &mentioned_skills {
    system_prompt.push_str(&skill.instructions);
}
```

**During Tool Execution**:

```rust
// Skills available as tools
for (name, skill) in mentioned_skills {
    tool_registry.register_skill_tool(name, skill);
}
```

## Testing

### Unit Tests

```rust
#[tokio::test]
async fn test_explicit_mention() {
    let input = "Use $pdf-analyzer to process doc";
    let skills = detect_mentioned_skills(input, workspace).await?;
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].0, "pdf-analyzer");
}

#[tokio::test]
async fn test_keyword_matching() {
    let input = "Extract tables from PDF document";
    let skills = detect_mentioned_skills(input, workspace).await?;
    // Asserts pdf-analyzer detected via keywords
}
```

### Integration Tests

Create test skills in `tests/fixtures/skills/`:

```
tests/fixtures/skills/
├── pdf-analyzer/
│   └── SKILL.md
└── spreadsheet-generator/
    └── SKILL.md
```

## Troubleshooting

### Skill Not Auto-Triggering

**Problem**: `$skill-name` not recognized

**Solutions**:

1. Check skill name matches exactly (case-insensitive)
2. Verify skill is discoverable: `/skills list`
3. Check `enable-auto-trigger = true` in config

**Problem**: Keyword matching not working

**Solutions**:

1. Verify `enable-description-matching = true`
2. Ensure description has 4+ char keywords
3. Check `min-keyword-matches` threshold (default: 2)
4. Add more descriptive keywords to skill description

### False Positives

**Problem**: Wrong skill triggered

**Solutions**:

1. Use explicit `$skill-name` syntax for precision
2. Increase `min-keyword-matches` (e.g., 3 instead of 2)
3. Make skill descriptions more specific
4. Disable keyword matching: `enable-description-matching = false`

## Related Documentation

-   [Codex Skills Improvements](../CODEX_SKILLS_IMPROVEMENTS.md) - Implementation details
-   [Skills Integration Guide](../AGENT_SKILLS_INTEGRATION.md) - Comprehensive skills guide
-   [Skills Quick Reference](../AGENT_SKILLS_QUICKREF.md) - Command reference

---

**Implementation Date**: December 15, 2024
**Pattern Source**: OpenAI Codex skills system
**Status**: ✅ Implemented and tested
