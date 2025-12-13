# Example: Generate PDFs with Agent Skills

This example demonstrates how to use Anthropic's PDF Skill with the Claude API to generate PDF documents.

## Python Example

```python
import anthropic
import os

def generate_pdf():
    """
    Generate a PDF document using Agent Skills.
    """
    client = anthropic.Anthropic(api_key=os.environ.get("ANTHROPIC_API_KEY"))
    
    response = client.messages.create(
        model="claude-3-5-sonnet-20241022",
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
                "content": """Generate a PDF with the following content:
                - A styled header with a title
                - Multiple paragraphs with different formatting
                - A table with sample data
                - Code snippets with syntax highlighting
                - Numbered and bulleted lists
                - Page numbers and footers"""
            }
        ],
        betas=[
            "code-execution-2025-08-25",
            "skills-2025-10-02"
        ]
    )
    
    return response

def download_pdf(client, file_id, output_path):
    """
    Download a generated PDF file using the Files API.
    """
    pdf_content = client.beta.files.retrieve_raw(file_id)
    
    with open(output_path, 'wb') as f:
        f.write(pdf_content.read())
    
    print(f"PDF saved to: {output_path}")

if __name__ == "__main__":
    client = anthropic.Anthropic(api_key=os.environ.get("ANTHROPIC_API_KEY"))
    
    result = generate_pdf()
    
    # Extract file ID and download
    file_id = None
    for content_block in result.content:
        if hasattr(content_block, 'type') and content_block.type == 'file':
            file_id = content_block.file_id
            break
    
    if file_id:
        download_pdf(client, file_id, "generated_document.pdf")
    
    print(result)
```

## Common PDF Use Cases

### 1. Invoice Generation
```python
message = """Generate an invoice PDF with:
- Invoice number and date
- Customer and vendor information
- Itemized list of products/services
- Subtotal, tax, and total amounts
- Payment terms and due date
- Professional formatting"""
```

### 2. Certificate Generation
```python
message = """Create a certificate PDF with:
- Ornate border design
- Recipient name (placeholder)
- Achievement or course name
- Issue date
- Signature lines
- Professional styling"""
```

### 3. Report with Charts
```python
message = """Generate a data report PDF containing:
- Executive summary
- Performance metrics
- Data visualizations and charts
- Detailed analysis
- Recommendations
- Professional layout with headers/footers"""
```

### 4. Data Sheet
```python
message = """Create a technical data sheet PDF with:
- Product specifications
- Comparison tables
- Technical diagrams
- Usage examples
- Warranty information
- Contact details"""
```

## Key Features

- **Styling**: Full control over fonts, colors, and layouts
- **Tables**: Create complex tables with merged cells
- **Images**: Embed images and graphics
- **Headers/Footers**: Add page numbers and persistent headers
- **Sections**: Create multi-section documents
- **Watermarks**: Add background text or images

## Integration with VTCode Skills

Create a custom skill for PDF generation:

```yaml
---
name: pdf-report-generator
description: Generate professional PDF reports with charts and styling
version: 1.0.0
---

# PDF Report Generator

## Instructions

When asked to generate a PDF report:

1. Understand the report requirements
2. Determine the data to include
3. Plan the layout and styling
4. Use the PDF Skill to generate
5. Return the file reference

## Examples

- Financial reports with charts
- Meeting agendas and minutes
- Technical documentation
- Customer proposals
```

## Performance Considerations

- **Large Documents**: For documents >50 pages, consider pagination strategies
- **Image Quality**: Optimize image resolution for file size vs. quality
- **Rendering Time**: Complex layouts may require longer processing
- **File Size**: Typical documents are 100KB-5MB

## Error Handling

```python
try:
    response = client.messages.create(...)
    file_id = extract_file_id(response)
    download_pdf(client, file_id, "output.pdf")
except anthropic.APIError as e:
    print(f"API Error: {e}")
except IOError as e:
    print(f"File I/O Error: {e}")
```

## See Also

- SPREADSHEET_EXAMPLE.md - Create Excel spreadsheets
- WORD_DOCUMENT_EXAMPLE.md - Create Word documents
- CONTAINER_GUIDE.md - Understand skill containers
