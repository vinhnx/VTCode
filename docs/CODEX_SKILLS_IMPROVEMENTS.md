# Codex-Inspired Skills Improvements

This document summarizes the OpenAI Codex-inspired improvements to VT Code's skills system implemented in December 2024.

## Overview

Based on analysis of [OpenAI Codex's skills implementation](https://github.com/openai/codex), we've adopted three key patterns to improve context efficiency and user experience:

1. **Lean Rendering Mode** (40-60% token savings)
2. **Embedded Usage Rules** (improved LLM behavior)
3. **Auto-Trigger Detection** (better UX with `$skill-name` syntax)

## Key Features

### 1. Lean Rendering Mode

**Problem**: VT Code's full rendering mode included version, author, and native flags for every skill, consuming excessive context tokens.

**Codex Pattern**: Only show name + description + file path in system prompt. Full metadata stays on disk.

**Implementation**:

```rust
// vtcode-core/src/skills/prompt_integration.rs
pub enum SkillsRenderMode {
    Lean,  // Codex-style: name + description + path only
    Full,  // Legacy: all metadata
}

// Usage
let prompt = generate_skills_prompt_with_mode(&skills, SkillsRenderMode::Lean);
```

**Output Format (Lean)**:

```markdown
## Skills

Available skills (name: description + file path). Content on disk; open file when triggered.

-   `pdf-analyzer`: Extract text and tables from PDFs (file: /path/to/pdf-analyzer)
-   `spreadsheet-generator`: Create Excel reports (file: /path/to/spreadsheet-generator)

(+12 more skills available)

**Usage Rules:**

-   **Discovery**: Skills listed above (name + description + file path)
-   **Trigger**: Use skill if user mentions `$SkillName` OR task matches description
-   **Progressive disclosure**:
    1. Open SKILL.md to get full instructions
    2. Load referenced files (scripts/, references/) only if needed
    3. Prefer running existing scripts vs. retyping code
-   **Missing/blocked**: State issue briefly and continue with fallback approach
```

**Token Savings**: Shows 10 skills instead of full manifests with version/author/flags. Estimated 40-60% reduction vs. full mode.

### 2. Embedded Usage Rules

**Problem**: Usage rules were documented separately, not visible to LLM during inference.

**Codex Pattern**: Embed comprehensive usage rules as footer in skills section of system prompt.

**Implementation**: Automatic in lean mode (see `SKILL_USAGE_RULES` constant in `prompt_integration.rs`).

**Rules Embedded**:

-   **Discovery**: How to find available skills
-   **Trigger**: When to use `$SkillName` vs. description matching
-   **Progressive Disclosure**: 3-step loading (SKILL.md → referenced files → scripts)
-   **Error Handling**: Graceful degradation when skills missing/blocked

### 3. Auto-Trigger Detection

**Problem**: Users had to explicitly invoke skills with commands. Not ergonomic.

**Codex Pattern**: Auto-trigger when user mentions `$skill-name` OR task matches description keywords.

**Implementation**:

```rust
// vtcode-core/src/skills/loader.rs
pub fn detect_skill_mentions(
    user_input: &str,
    available_skills: &[SkillManifest]
) -> Vec<String>
```

**Trigger Patterns**:

1. **Explicit `$skill-name`** (case-insensitive):

    ```
    User: "Use $pdf-analyzer to process the document"
    → Triggers: pdf-analyzer
    ```

2. **Description Keyword Matching** (fuzzy, requires 2+ keyword matches):
    ```
    User: "Extract data from PDF and create spreadsheet with charts"
    → Triggers: pdf-analyzer, spreadsheet-generator (if descriptions match)
    ```

**Configuration**:

```toml
# vtcode.toml
[skills]
render-mode = "lean"              # "lean" | "full"
max-skills-in-prompt = 10
enable-auto-trigger = true
enable-description-matching = true
min-keyword-matches = 2
```

## Configuration

### Default (Lean Mode)

```toml
[skills]
render-mode = "lean"
max-skills-in-prompt = 10
enable-auto-trigger = true
enable-description-matching = true
min-keyword-matches = 2
```

### Legacy (Full Mode)

```toml
[skills]
render-mode = "full"
max-skills-in-prompt = 10
enable-auto-trigger = false
enable-description-matching = false
```

## Usage Examples

### For Skill Authors

**SKILL.md Frontmatter** (unchanged):

```yaml
---
name: my-skill
description: What this skill does and when to use it
version: 1.0.0 # Optional (not shown in lean mode)
author: Your Name # Optional (not shown in lean mode)
vtcode-native: true # Optional (not shown in lean mode)
---
# My Skill Instructions
```

### For Users

**Explicit Trigger**:

```
User: Use $pdf-analyzer to extract tables
```

**Implicit Trigger** (description matching):

```
User: Extract tables from this PDF document
→ Auto-triggers if description contains "extract" and "tables" or "PDF"
```

### For Integrators

```rust
use vtcode_core::skills::{
    generate_skills_prompt_with_mode,
    SkillsRenderMode,
    detect_skill_mentions,
};

// Render skills section
let skills_prompt = generate_skills_prompt_with_mode(
    &skills_map,
    SkillsRenderMode::Lean
);

// Detect mentions in user input
let user_input = "Use $pdf-analyzer to process this";
let triggers = detect_skill_mentions(user_input, &available_skills);
// triggers = ["pdf-analyzer"]
```

## Comparison: Codex vs. VT Code

| Feature                 | Codex                        | VT Code (Before)                          | VT Code (Now)                     |
| ----------------------- | ---------------------------- | ----------------------------------------- | --------------------------------- |
| **Prompt Format**       | Name + desc + path only      | Full manifest with metadata               | Configurable (lean or full)       |
| **Token Usage**         | Minimal (~50 tokens/skill)   | High (~150 tokens/skill)                  | **Lean: ~50 tokens** / Full: ~150 |
| **Usage Rules**         | Embedded in prompt footer    | Separate docs                             | **Embedded in lean mode**         |
| **Trigger Syntax**      | `$skill-name` OR description | Explicit command only                     | **Both patterns supported**       |
| **Discovery**           | Startup-only, recursive      | Dynamic, multi-path                       | Dynamic, multi-path (kept)        |
| **Progressive Loading** | File-path reference          | 3-level (metadata/instructions/resources) | 3-level (kept)                    |

**Winner**: VT Code now matches Codex's context efficiency while preserving advanced features (progressive loading, CLI tool bridge, token budgets).

## Testing

All features tested with comprehensive unit tests:

```bash
# Rendering modes
cargo nextest run --package vtcode-core --lib 'skills::prompt_integration'

# Auto-trigger detection
cargo nextest run --package vtcode-core --lib 'skills::loader::tests::test_detect'

# Configuration
cargo nextest run --package vtcode-config --lib 'core::skills'
```

**Test Coverage**:

-   Lean vs. full rendering
-   Token savings verification
-   `$skill-name` detection (case-insensitive)
-   Description keyword matching (2+ matches required)
-   Configuration serialization/deserialization

## Migration Guide

### For Existing Skills

**No changes required!** Skills work identically in both lean and full modes. The only difference is how they're rendered in the system prompt.

### For Codebases Using Skills API

**Before**:

```rust
let prompt = generate_skills_prompt(&skills);
```

**After (Explicit Mode)**:

```rust
use vtcode_core::skills::SkillsRenderMode;

let prompt = generate_skills_prompt_with_mode(&skills, SkillsRenderMode::Lean);
```

**After (From Config)**:

```rust
let render_mode = config.skills.render_mode;  // SkillsRenderMode from vtcode.toml
let prompt = generate_skills_prompt_with_mode(&skills, render_mode);
```

### For Configuration Files

Add to `vtcode.toml`:

```toml
[skills]
render-mode = "lean"  # Recommended for context efficiency
enable-auto-trigger = true
```

## References

-   [OpenAI Codex PR #7412](https://github.com/openai/codex/pull/7412/changes)
-   [Codex Skills Rendering](https://github.com/openai/codex/blob/ad7b9d63c326d5c92049abd16f9f5fb64a573a69/codex-rs/core/src/skills/render.rs#L20-L38)
-   [Codex Skills Documentation](https://github.com/openai/codex/blob/main/docs/skills.md)

## Future Enhancements

1. **XML Injection Format**: Codex uses `<skill><name>...</name>...</skill>` tags. Evaluate if this improves LLM caching/parsing.
2. **Startup Validation Modal**: Codex shows blocking TUI modal for skill load errors. Consider for better user visibility.
3. **Frontmatter Simplification**: Make version/author/vtcode-native optional by default (already supported, just need docs).

---

**Implementation Date**: December 14, 2024
**Status**: ✅ Complete (all tests passing)
**Token Savings**: 40-60% vs. full mode
**Backward Compatibility**: 100% (lean mode is default, full mode still available)
