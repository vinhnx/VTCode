# VT Code Output Styles

Output styles allow you to customize the behavior and response format of VT Code by modifying the system prompt. This feature enables different modes of operation, from concise responses to detailed explanations.

## Overview

Output styles work by modifying VT Code's system prompt. Each style can:
- Add custom instructions to the base prompt (when `keep-coding-instructions: true`)
- Replace the base prompt entirely (when `keep-coding-instructions: false`)

## Configuration

Output styles are configured in your `vtcode.toml` file:

```toml
[output_style]
active_style = "default"  # Set the active output style
```

## Creating Custom Output Styles

Output styles are defined in `.vtcode/output-styles/` directory as markdown files with YAML frontmatter:

```markdown
---
name: My Custom Style
description: A brief description of what this style does
keep-coding-instructions: true  # Whether to keep base VT Code instructions
---

# Custom Instructions

Add your custom instructions here. These will be added to the system prompt.
```

### Frontmatter Options

- `name`: The name of the output style (required)
- `description`: A brief description of the style (optional)
- `keep-coding-instructions`: Whether to preserve VT Code's base instructions (default: true)

### Style Behavior

- When `keep-coding-instructions` is `true`: Your custom content is appended to VT Code's base system prompt
- When `keep-coding-instructions` is `false`: Your custom content replaces VT Code's base system prompt entirely

## Available Output Styles

VT Code ships with several built-in output styles:

### Default
- Name: `default`
- Description: Standard VT Code output style with concise responses and efficient tool handling
- Keeps base instructions: Yes

### Explanatory
- Name: `explanatory`
- Description: Educational output style that provides insights and explanations
- Keeps base instructions: Yes

### Learning
- Name: `learning`
- Description: Collaborative learning mode where VT Code guides users to contribute code themselves
- Keeps base instructions: Yes

### Developer
- Name: `developer`
- Description: Developer-focused output style optimized for coding tasks and technical work
- Keeps base instructions: Yes

### Architect
- Name: `architect`
- Description: Architecture-focused output style for system design and high-level planning
- Keeps base instructions: Yes

## Examples

### Custom Style That Adds Code Review Instructions

```markdown
---
name: Code Reviewer
description: Style that focuses on code quality and best practices
keep-coding-instructions: true
---

## Code Review Guidelines

When reviewing code, always consider:
1. Security implications
2. Performance impact
3. Maintainability
4. Test coverage
5. Documentation completeness
```

### Style That Changes VT Code's Personality

```markdown
---
name: Pair Programmer
description: Style that simulates pair programming
keep-coding-instructions: true
---

## Pair Programming Mode

You are a friendly pair programming partner. 
- Explain your thought process as you work
- Ask questions to understand the user's goals
- Suggest alternatives and discuss trade-offs
- Encourage good practices through conversation
```

## Using Output Styles

To switch between output styles, update your `vtcode.toml` configuration file and restart VT Code.

## Best Practices

1. **Start Simple**: Begin with existing styles and modify them to suit your needs
2. **Test Thoroughly**: Different styles may affect VT Code's behavior significantly
3. **Document Your Styles**: Add clear descriptions to help others understand your custom styles
4. **Consider Context**: Some styles work better for specific tasks or projects