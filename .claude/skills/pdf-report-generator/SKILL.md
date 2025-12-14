---
name: pdf-report-generator
description: Generate professional PDF documents with charts, styling, and complex layouts using Claude and Anthropic's pdf skill
version: 1.0.0
author: VTCode Team
---

# PDF Report Generator Skill

Generate professional PDF documents with advanced styling, charts, complex layouts, and pixel-perfect formatting. This skill leverages Anthropic's pdf Agent Skill to create distribution-ready PDF files programmatically.

## Instructions

When asked to generate a PDF document:

1. **Understand Requirements**: Parse the request for:

    - Document type (report, invoice, certificate, proposal)
    - Content structure and sections
    - Visual design requirements
    - Charts, graphs, or data visualizations
    - Branding, colors, and styling
    - Multi-page layout needs

2. **Plan the PDF**:

    - Outline layout and visual hierarchy
    - Identify chart/graph requirements
    - Plan color scheme and branding
    - Determine pagination and flow
    - Design professional header/footer

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
            "content": "Generate a PDF with [specification]"
        }],
        container={
            "type": "skills",
            "skills": [{"type": "anthropic", "skill_id": "pdf", "version": "latest"}]
        },
        betas=["code-execution-2025-08-25", "skills-2025-10-02"]
    )
    ```

4. **Extract File Reference**:
    - Locate file_id in response content blocks
    - Return file reference for download/distribution

## Examples

### Financial Report

**Input**: "Generate a quarterly financial report PDF with revenue trends, expense breakdown, and key metrics"
**Output**: Professional PDF with charts, formatted tables, executive summary, and detailed sections

### Invoice Generation

**Input**: "Create an invoice PDF for a client with itemized services, totals, and payment terms"
**Output**: Formatted invoice with company branding, clear line items, and professional layout

### Certificate/Award

**Input**: "Design a certificate PDF for course completion with decorative border and recipient name"
**Output**: Professional certificate with elegant styling and fillable recipient section

## Features Supported

-   **Text Styling**: Custom fonts, sizes, colors, weights
-   **Headers/Footers**: Page numbers, running headers, consistent branding
-   **Tables**: Multi-column formatted tables with borders
-   **Charts**: Bar, line, pie, area charts with labels
-   **Images**: Embedded images, logos, signatures
-   **Backgrounds**: Colors, gradients, patterns
-   **Watermarks**: Background text or images
-   **Sections**: Different layouts for document parts
-   **Page Breaks**: Control pagination and flow
-   **Borders/Frames**: Professional design elements

## Use Cases

-   Financial and audit reports
-   Invoice and receipt generation
-   Certificate and diploma creation
-   Marketing proposals and case studies
-   Technical specifications and datasheets
-   Meeting agendas and minutes
-   Customer proposals and quotes
-   Data analysis reports

## Report Types

### Executive Reports

-   Summary metrics and KPIs
-   Data visualizations and charts
-   Recommendations and insights
-   Professional executive layout

### Financial Reports

-   Revenue and expense analysis
-   Balance sheets and financial statements
-   Trend analysis with charts
-   Budget variance reports

### Sales & Marketing

-   Sales performance reports
-   Customer analysis and segmentation
-   Campaign performance metrics
-   Proposal documents

### Technical Documentation

-   System specifications
-   Architecture diagrams
-   Integration guides
-   Performance benchmarks

## Best Practices

1. **Visual Hierarchy**: Clear distinction between sections and content
2. **Professional Branding**: Consistent logo, colors, fonts
3. **Data Visualization**: Appropriate charts for data type
4. **White Space**: Balanced layout with breathing room
5. **Typography**: Readable fonts with proper hierarchy
6. **Color Coordination**: Professional, accessible color scheme
7. **Footer Information**: Page numbers, dates, document identifiers

## Advanced Techniques

### Multi-Section Documents

```
Cover Page → Executive Summary → Detailed Analysis → Appendices
```

### Watermarks and Backgrounds

-   Company branding watermarks
-   Confidentiality stamps
-   Status indicators

### Complex Charts

-   Multi-series line charts with trends
-   Stacked bar charts for comparisons
-   Pie charts for composition

## Related Skills

-   `spreadsheet-generator` - For data preparation and analysis
-   `doc-generator` - For collaborative drafting in Word
-   `presentation-builder` - For stakeholder presentations

## Performance Considerations

-   Large documents (>100 pages) optimize with summaries
-   Image quality impacts file size (typically 100KB-5MB)
-   Complex layouts may require longer processing
-   Multiple charts increase generation time

## Limitations

-   Maximum document size: ~50MB in code execution environment
-   Interactive PDF forms require post-processing
-   Real-time data updates need external integration
-   Advanced PDF features (3D, multimedia) not supported
-   OCR capabilities limited
