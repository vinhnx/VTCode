# Skill Container API Guide

Quick reference for using VT Code's skill container system (Claude API-aligned).

## Basic Usage

### Single Skill
```rust
use vtcode_core::skills::{SkillContainer, SkillSpec};

// Create container with one skill
let container = SkillContainer::single(
    SkillSpec::custom("my-analysis-skill")
);

// Execute
executor.execute_container(container, "analyze this data").await?;
```

### Multiple Skills
```rust
let mut container = SkillContainer::new();

// Add Anthropic-managed skills
container.add_anthropic("xlsx")?;  // Excel support
container.add_anthropic("pptx")?;  // PowerPoint support
container.add_custom("my-skill")?; // Your custom skill

assert_eq!(container.len(), 3);
executor.execute_container(container, input).await?;
```

## Advanced Usage

### Version Pinning
```rust
use vtcode_core::skills::{SkillVersion, SkillSpec};

// Use latest (default)
let spec = SkillSpec::custom("analysis-skill");

// Pin to specific version
let spec = SkillSpec::custom("analysis-skill")
    .with_version(SkillVersion::Specific("1759178010641129".to_string()));

container.add_skill(spec)?;
```

### Container Reuse
```rust
// Create container once
let mut container = SkillContainer::new();
container.add_anthropic("xlsx")?;
container.add_anthropic("pptx")?;
container.set_id("session-123");  // Set ID for reuse

// Turn 1: Analyze data
let result1 = executor.execute_container(&container, 
    "analyze sales data from Q1"
).await?;

// Turn 2: Create presentation
let result2 = executor.execute_container(&container, 
    "create a presentation from the analysis"
).await?;

// Container state preserved across turns
```

### Skill Filtering
```rust
let container = SkillContainer::single(SkillSpec::custom("test"));

// Check skill types
container.anthropic_count();  // Count Anthropic skills
container.custom_count();     // Count custom skills

// Query
container.has_skill("xlsx");              // Check existence
container.get_skill("my-skill");          // Get by ID
container.skill_ids();                    // List all IDs
container.skills_by_type(SkillType::Custom); // Filter by type
```

## Builder Pattern

```rust
let container = SkillContainer::new()
    // Add anthropic skills
    .with_id("session-456")?
    
    // ... error handling for convenience
```

Or use individual methods:

```rust
let mut container = SkillContainer::new();
container.add_anthropic("xlsx")?;
container.add_anthropic("pptx")?;
container.add_custom("analysis")?;
container.set_id("session-456");

container.validate()?;  // Explicit validation
```

## Validation

```rust
let mut container = SkillContainer::new();

// Add 8 skills (max)
for i in 0..8 {
    container.add_skill(SkillSpec::custom(format!("skill{}", i)))?;
}

// This fails - exceed limit
container.add_skill(SkillSpec::custom("skill9"))?;  // Error!

// Validate
container.validate()?;  // Checks:
                         // - Max 8 skills
                         // - No duplicate IDs
```

## Serialization

```rust
use serde_json;

let container = SkillContainer::single(
    SkillSpec::custom("analysis-skill")
);

// Serialize to JSON
let json = serde_json::to_string(&container)?;

// Deserialize back
let restored: SkillContainer = serde_json::from_str(&json)?;
```

JSON Output:
```json
{
  "skills": [
    {
      "type": "custom",
      "skill_id": "analysis-skill",
      "version": "latest"
    }
  ]
}
```

## CLI Usage (When Integrated)

```bash
# List available skills
vtcode skills list

# Show skill details
vtcode skills info my-skill

# Create container with multiple skills
vtcode exec-container --skill xlsx --skill pptx --skill my-skill \
  "analyze data and create presentation"

# Use specific version
vtcode exec-container --skill my-skill:1759178010641129 \
  "use v1 of my skill"
```

## API Reference

### SkillContainer

```rust
// Creation
SkillContainer::new()                         // Empty container
SkillContainer::single(spec)                  // With one skill
SkillContainer::with_id(id)                   // With ID

// Management
container.add_skill(spec)                     // Add one
container.add_skills(specs)                   // Add multiple
container.add_anthropic(id)                   // Add Anthropic skill
container.add_custom(id)                      // Add custom skill

// Queries
container.len()                               // Number of skills
container.is_empty()                          // Check empty
container.has_skill(id)                       // Check existence
container.get_skill(id)                       // Get by ID
container.skill_ids()                         // List all IDs
container.skills_by_type(type)                // Filter by type
container.anthropic_count()                   // Count Anthropic
container.custom_count()                      // Count custom

// ID Management
container.set_id(id)                          // Set for reuse
container.clear_id()                          // Clear ID
container.id                                  // Access ID

// Validation
container.validate()                          // Full validation
```

### SkillSpec

```rust
// Creation
SkillSpec::new(type, id)                      // Generic
SkillSpec::anthropic(id)                      // Anthropic
SkillSpec::custom(id)                         // Custom

// With version
SkillSpec::custom("skill")
    .with_version(SkillVersion::Latest)
    .with_version(SkillVersion::Specific("epoch"))

// Access
spec.skill_type                                // Type (Anthropic/Custom)
spec.skill_id                                  // ID
spec.version                                   // Version (Latest/Specific)
```

### SkillVersion

```rust
SkillVersion::Latest                          // Always latest
SkillVersion::Specific("1759178010641129")   // Specific epoch

// Methods
version.as_str()                              // Get as string
version.is_latest()                           // Check if latest
```

### SkillType

```rust
SkillType::Anthropic                          // Pre-built by Anthropic
SkillType::Custom                             // User-uploaded
```

## Common Patterns

### Excel + PowerPoint Analysis
```rust
let mut container = SkillContainer::new();
container.add_anthropic("xlsx")?;   // Read Excel
container.add_anthropic("pptx")?;   // Create PowerPoint

let input = "analyze sales_data.xlsx and create a presentation";
executor.execute_container(container, input).await?;
```

### Custom Analysis Pipeline
```rust
let mut container = SkillContainer::new();
container.add_custom("data-cleaner")?;
container.add_custom("statistical-analyzer")?;
container.add_custom("report-generator")?;

let input = "clean and analyze customer_data.csv";
executor.execute_container(container, input).await?;
```

### Progressive Enhancement
```rust
// Start simple
let mut container = SkillContainer::single(
    SkillSpec::custom("basic-analysis")
);

// Add more skills based on input
if input.contains("excel") {
    container.add_anthropic("xlsx")?;
}
if input.contains("presentation") {
    container.add_anthropic("pptx")?;
}

executor.execute_container(container, input).await?;
```

## Error Handling

```rust
use anyhow::Result;

fn build_container() -> Result<SkillContainer> {
    let mut container = SkillContainer::new();
    
    // May fail if skill ID invalid
    container.add_anthropic("xlsx")?;
    container.add_custom("analysis")?;
    
    // May fail if exceeds 8 skills
    for i in 0..10 {
        container.add_custom(format!("skill{}", i))?;  // Error on 9th
    }
    
    // Validate before execution
    container.validate()?;
    
    Ok(container)
}
```

## Performance Tips

1. **Reuse containers**: Set ID for multi-turn conversations
2. **Batch operations**: Use `add_skills()` instead of repeated `add_skill()`
3. **Validate once**: Call `validate()` before heavy operations
4. **Pin versions**: Use specific versions in production

## Compatibility

-  JSON serialization (serde)
-  Backward compatible with existing skills
-  Works with existing skill loader
-  Compatible with tool registry
-  No breaking changes

## See Also

- [Skill Architecture](./ARCHITECTURE.md)
- [Skill Manifest Guide](./MANIFEST.md)
- [Skills Enhancement Plan](../SKILLS_ENHANCEMENT_PLAN.md)
- [Claude Agent Skills API](https://docs.anthropic.com/en/docs/agents-and-tools/agent-skills)
