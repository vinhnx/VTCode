# Skill Mention Detection - Chat Integration Example

This document shows how to integrate skill mention detection into the VT Code chat flow.

## Quick Integration

```rust
use vtcode_core::agent::runloop::detect_mentioned_skills;

async fn process_chat_message(user_input: &str, workspace: PathBuf) -> Result<String> {
    // Auto-detect skills from user input
    let mentioned_skills = detect_mentioned_skills(user_input, workspace).await?;

    // Build system prompt with detected skills
    let mut system_prompt = base_prompt.to_string();

    if !mentioned_skills.is_empty() {
        system_prompt.push_str("\n\n## Active Skills\n");
        for (name, skill) in &mentioned_skills {
            system_prompt.push_str(&format!("\n### {}\n{}\n", name, skill.instructions));
        }
    }

    // Send to LLM with enhanced context
    Ok(llm_generate(system_prompt, user_input).await?)
}
```

## Usage Examples

### Example 1: PDF Processing

**User Input**:

```
Use $pdf-analyzer to extract tables from report.pdf
```

**Detection Result**:

```rust
mentioned_skills = [("pdf-analyzer", Skill { ... })]
```

**Enhanced System Prompt**:

```
Base prompt...

## Active Skills

### pdf-analyzer
Extract text and tables from PDF documents.
Use read_file tool to access PDFs, extract structured data...
```

### Example 2: Keyword Matching

**User Input**:

```
Generate Excel spreadsheet with sales data analysis
```

**Detection Result** (if description contains "spreadsheet", "data", "analysis"):

```rust
mentioned_skills = [("spreadsheet-generator", Skill { ... })]
```

## See Also

