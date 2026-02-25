# VT Code Output Styles Feature

## Overview

VT Code now supports output styles, allowing you to customize the behavior and response format of the assistant. This feature enables different modes of operation, from concise responses to detailed explanations, and even specialized workflows like learning and architectural design.

## How It Works

Output styles modify VT Code's system prompt to change how the assistant behaves. Each style can either enhance the base system prompt or replace it entirely, depending on its configuration.

## Configuration

Output styles are configured in your `vtcode.toml` configuration file:

```toml
[output_style]
active_style = "default"  # Set the active output style
```

## Available Styles

- `default`: Standard VT Code behavior with concise responses
- `explanatory`: Provides detailed explanations and educational insights
- `learning`: Collaborative learning mode with guidance for users
- `developer`: Optimized for coding tasks and technical work
- `architect`: Focused on system design and architectural planning

## Creating Custom Styles

Custom output styles can be created in the `.vtcode/output-styles/` directory as markdown files with YAML frontmatter. Each style file defines:

- The style name and description
- Whether to keep the base VT Code instructions
- Custom instructions to modify the system prompt

## Benefits

- **Flexibility**: Adapt VT Code's behavior to different tasks and contexts
- **Customization**: Create specialized workflows for your specific needs
- **Learning**: Educational modes that help users improve their skills
- **Productivity**: Optimized responses for different types of work

## Implementation Details

The output styles feature is implemented through:

1. A new `OutputStyleConfig` in the configuration system
2. An `OutputStyleManager` to load and manage style definitions
3. Integration with the system prompt generation pipeline
4. Support for YAML frontmatter in markdown style files

The system applies the selected output style during system prompt generation, ensuring that the assistant's behavior matches the selected style from the start of each interaction.