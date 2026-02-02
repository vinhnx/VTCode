# Example: Create a Word Document with Agent Skills

This example demonstrates how to use Anthropic's Word (docx) Skill with the Claude API to create documents.

## Python Example

```python
import anthropic
import os

def create_word_document():
    """
    Create a Word document with formatted content using Agent Skills.
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
                "content": """Create a Word document with the following structure:
                - Title: "Project Report"
                - Introduction paragraph
                - Three main sections with headings and bullet points
                - A conclusion
                - Include some formatted text (bold, italic)
                - Add page breaks between sections"""
            }
        ],
        betas=[
            "code-execution-2025-08-25",
            "skills-2025-10-02"
        ]
    )

    return response

def extract_file_id(response):
    """Extract file ID from API response."""
    # File ID will be in the response content blocks
    for content_block in response.content:
        if hasattr(content_block, 'type') and content_block.type == 'file':
            return content_block.file_id
    return None

if __name__ == "__main__":
    result = create_word_document()

    # Extract and print file ID
    file_id = extract_file_id(result)
    if file_id:
        print(f"Created document with file ID: {file_id}")

    print(result)
```

## Features Supported

- **Text Formatting**: Bold, italic, underline
- **Headings**: Multiple heading levels
- **Lists**: Bullet points and numbered lists
- **Tables**: Create and format tables
- **Page Breaks**: Insert page breaks between sections
- **Images**: Embed images in documents
- **Styles**: Apply predefined or custom styles

## Usage Patterns

### Pattern 1: Meeting Minutes

```python
"Create a Word document for meeting minutes with:
- Date and attendees
- Agenda items
- Discussion notes
- Action items with owners and due dates"
```

### Pattern 2: API Documentation

```python
"Generate API documentation in Word format with:
- Endpoint descriptions
- Parameter specifications
- Example requests and responses
- Error codes and meanings"
```

### Pattern 3: Report Generation

```python
"Create a professional report with:
- Executive summary
- Detailed findings
- Charts and data tables
- Recommendations
- Appendices"
```

## Integration with VT Code

Use Agent Skills in VT Code to generate documents programmatically:

```bash
vtcode /skills use doc-generator "Create a project proposal document"
```

## See Also

- PDF_GENERATION_EXAMPLE.md - Generate PDFs
- SPREADSHEET_EXAMPLE.md - Create Excel spreadsheets
