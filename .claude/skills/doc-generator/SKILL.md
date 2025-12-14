---
name: doc-generator
description: Generate professional Word documents with formatted text, tables, and layouts using Claude and Anthropic's docx skill
version: 1.0.0
author: VTCode Team
---

# Document Generator Skill

Generate professional Word documents with rich formatting, tables, styles, and structured content. This skill leverages Anthropic's docx Agent Skill to create Word documents programmatically.

## Instructions

When asked to generate a Word document:

1. **Understand Requirements**: Parse the request for:

    - Document type (report, proposal, minutes, guide)
    - Content sections and hierarchy
    - Formatting needs (styles, colors, fonts)
    - Tables, lists, or structured data
    - Branding and visual requirements

2. **Plan the Document**:

    - Outline sections and structure
    - Identify formatting requirements
    - Determine table layouts
    - Plan page breaks and flow

3. **Create with Code Execution**:

    ```python
    import anthropic

    client = anthropic.Anthropic()
    response = client.messages.create(
        model="claude-4-5-sonnet",
        max_tokens=4096,
        tools=[{"type": "code_execution", "name": "bash"}],
        messages=[{
            "role": "user",
            "content": "Create a Word document with [specification]"
        }],
        container={
            "type": "skills",
            "skills": [{"type": "anthropic", "skill_id": "docx", "version": "latest"}]
        },
        betas=["code-execution-2025-08-25", "skills-2025-10-02"]
    )
    ```

4. **Extract File Reference**:
    - Locate file_id in response content blocks
    - Return file reference for download/integration

## Examples

### Project Proposal

**Input**: "Create a project proposal for a mobile app development project"
**Output**: Professional document with executive summary, scope, timeline, budget, and appendices

### Meeting Minutes

**Input**: "Generate meeting minutes from our Q4 planning session with attendees, agenda, decisions, and action items"
**Output**: Formatted minutes with clear sections and action item tracking table

### API Documentation

**Input**: "Create API documentation for a REST service with endpoints, parameters, examples, and error codes"
**Output**: Comprehensive technical documentation with code blocks and tables

## Features Supported

-   **Text Formatting**: Bold, italic, underline, strikethrough
-   **Heading Styles**: Multiple heading levels with built-in styles
-   **Lists**: Bullet points, numbered lists, multi-level nesting
-   **Tables**: Complex tables with merged cells and formatting
-   **Page Breaks**: Section separation and flow control
-   **Images**: Embedding images and maintaining aspect ratio
-   **Margins & Spacing**: Professional document layout
-   **Headers/Footers**: Persistent page headers and footers
-   **Sections**: Different formatting for document parts
-   **Styles**: Apply predefined or custom paragraph/character styles

## Use Cases

-   Business proposals and contracts
-   Project reports and case studies
-   Meeting minutes and agendas
-   Technical documentation and guides
-   Training materials and manuals
-   Marketing collateral and brochures
-   Employee handbooks and policies
-   Academic papers and theses

## Document Types

### Reports

-   Executive summaries with findings
-   Data-driven analysis with tables
-   Recommendations and action plans

### Proposals

-   Project scope and deliverables
-   Timeline and milestones
-   Budget and resource requirements
-   Risk assessment

### Meeting Documentation

-   Attendee lists
-   Agenda items with discussions
-   Action items with owners and dates
-   Next meeting details

### Technical Documentation

-   System architecture overviews
-   Integration guides
-   API specifications
-   Troubleshooting guides

## Best Practices

1. **Clear Structure**: Use proper heading hierarchy
2. **Professional Layout**: Consistent spacing and margins
3. **Table Organization**: Clear headers and logical grouping
4. **Visual Hierarchy**: Effective use of bold, colors, and styles
5. **Page Management**: Appropriate breaks and flow
6. **Consistency**: Uniform formatting throughout
7. **Readability**: Appropriate font sizes and spacing

## Related Skills

-   `spreadsheet-generator` - For data analysis and reporting
-   `pdf-report-generator` - For final distribution-ready documents
-   `presentation-builder` - For stakeholder presentations

## Limitations

-   File size limit: ~20MB in code execution environment
-   Complex embedded objects require manual adjustment
-   Mail merge functionality limited
-   Form field creation requires post-processing
-   Advanced VBA macros not natively supported
