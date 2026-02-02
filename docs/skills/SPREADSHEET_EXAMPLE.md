# Example: Create a Spreadsheet with Agent Skills

This example demonstrates how to use Anthropic's Excel (xlsx) Skill with the Claude API to create a spreadsheet.

## Python Example

```python
import anthropic
import os
import base64

def create_spreadsheet():
    """
    Create an Excel spreadsheet with climate data using Agent Skills.
    """
    client = anthropic.Anthropic(api_key=os.environ.get("ANTHROPIC_API_KEY"))

    response = client.messages.create(
        model="claude-haiku-4-5",
        max_tokens=4096,
        tools=[
            {
                "type": "code_execution",
                "name": "bash",
                "description": "Execute bash commands"
            }
        ],
        messages=[
            {
                "role": "user",
                "content": "Create an Excel spreadsheet with climate data for different cities. Include columns for city name, average temperature, humidity, and precipitation. Use realistic data."
            }
        ],
        betas=[
            "code-execution-2025-08-25",
            "skills-2025-10-02"
        ]
    )

    return response

if __name__ == "__main__":
    result = create_spreadsheet()
    print(result)
```

## Key Points

1. **Skill Specification**: Use the `skills` parameter in the Messages API to specify which skills Claude can use
2. **Progressive Disclosure**: Claude loads skill metadata, then instructions when needed
3. **Code Execution**: The xlsx skill requires code execution capabilities
4. **File Output**: Files are created in the code execution environment and can be downloaded using the Files API

## Expected Output

The response will include:

- Claude's planning and execution steps
- File creation commands
- A file reference containing the created Excel spreadsheet

## Next Steps

See WORD_DOCUMENT_EXAMPLE.md for creating Word documents, or PDF_GENERATION_EXAMPLE.md for PDF generation.
