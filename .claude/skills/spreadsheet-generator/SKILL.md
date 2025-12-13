---
name: spreadsheet-generator
description: Generate professional Excel spreadsheets with data, charts, and formatting using Claude and Anthropic's xlsx skill
version: 1.0.0
author: VTCode Team
---

# Spreadsheet Generator Skill

Generate professional Excel spreadsheets with structured data, formulas, charts, and professional formatting. This skill leverages Anthropic's xlsx Agent Skill to create Excel documents programmatically.

## Instructions

When asked to generate a spreadsheet:

1. **Understand Requirements**: Parse the request for:
   - Data content (financial, operational, analytical)
   - Structure (sheets, columns, headers)
   - Formatting needs (colors, number formats, alignment)
   - Charts or visualizations required
   - Formulas or calculations

2. **Plan the Spreadsheet**:
   - Sketch sheet layout and column structure
   - Identify calculations and dependencies
   - Determine chart types and data ranges
   - Plan summary/dashboard sheets if needed

3. **Create with Code Execution**:
   ```python
   import anthropic
   
   client = anthropic.Anthropic()
   response = client.messages.create(
       model="claude-3-5-sonnet-20241022",
       max_tokens=4096,
       tools=[{"type": "code_execution", "name": "bash"}],
       messages=[{
           "role": "user",
           "content": "Create an Excel file with [specification]"
       }],
       container={
           "type": "skills",
           "skills": [{"type": "anthropic", "skill_id": "xlsx", "version": "latest"}]
       },
       betas=["code-execution-2025-08-25", "skills-2025-10-02"]
   )
   ```

4. **Extract File Reference**:
   - Locate file_id in response content blocks
   - Return file reference for download/integration

## Examples

### Financial Dashboard
**Input**: "Create a quarterly financial dashboard with revenue, expenses, and profit margins"
**Output**: Excel file with multiple sheets - raw data, calculations, visual dashboard

### Sales Analysis
**Input**: "Generate a sales report for Q4 2024 by region with growth percentages and trend analysis"
**Output**: Structured spreadsheet with regional data, metrics, and charts

### Inventory Tracking
**Input**: "Create an inventory management spreadsheet with SKU, quantity, cost, and reorder levels"
**Output**: Professional inventory tracker with formulas and conditional formatting

## Features Supported

- **Multiple Sheets**: Create and organize data across sheets
- **Formatting**: Colors, fonts, number formats, borders
- **Formulas**: SUM, AVERAGE, IF, VLOOKUP, and complex calculations
- **Charts**: Bar, line, pie, scatter plots with proper labeling
- **Tables**: Formatted data ranges with filters
- **Conditional Formatting**: Highlight cells based on values
- **Merged Cells**: Professional layout with spans
- **Number Formats**: Currency, percentages, dates

## Use Cases

- Financial reporting and analysis
- Sales performance dashboards
- Employee records and payroll
- Inventory and asset management
- Budget planning and tracking
- Data analysis and summaries
- Project timelines and schedules
- Survey results compilation

## Best Practices

1. **Clear Structure**: Use headers and organize data logically
2. **Formulas Over Static Data**: Use calculations for maintainability
3. **Professional Formatting**: Match company standards and branding
4. **Documentation**: Include notes or instructions sheets
5. **Chart Selection**: Use appropriate visualization for data type
6. **Performance**: Optimize large datasets with summaries

## Related Skills

- `doc-generator` - For written reports and documentation
- `pdf-report-generator` - For final distribution-ready reports
- `presentation-builder` - For stakeholder presentations

## Limitations

- Maximum file size: ~50MB for code execution environment
- Complex VBA macros not supported
- Real-time data connection limitations
- Advanced Power Query workflows require manual setup post-generation
