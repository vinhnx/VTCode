---
name: spreadsheet-generator
description: Generate professional Excel spreadsheets with data, charts, and formatting using Claude and Anthropic's xlsx skill
version: 1.0.0
author: VTCode Team
---

# Spreadsheet Generator Skill

Generate professional Excel spreadsheets with structured data, formulas, charts, and professional formatting. This skill leverages Anthropic's xlsx Agent Skill to create Excel documents programmatically.

## Instructions

** IMPORTANT: vtcode Compatibility Note**

This skill requires Anthropic's container skills feature (xlsx skill) which is only available through:
- Anthropic's official CLI (`anthropic` command)
- Claude Desktop app with skills enabled
- Direct Anthropic API with `container.skills` parameter

**vtcode does not currently support Anthropic container skills.** Instead, use one of these approaches:

### Option 1: Use Anthropic CLI (Recommended)
```bash
# Install Anthropic CLI: pip install anthropic
# Set API key: export ANTHROPIC_API_KEY=your_key
anthropic messages create \
  --model claude-3-5-sonnet-20241022 \
  --max-tokens 4096 \
  --container-skills anthropic:xlsx:latest \
  --message "Create an Excel file with [your specification]"
```

### Option 2: Python Script with openpyxl
Use vtcode's `execute_code` tool with Python and openpyxl library:

```python
import openpyxl
from openpyxl.styles import Font, PatternFill, Alignment
from openpyxl.chart import BarChart, Reference

# Create workbook
wb = openpyxl.Workbook()
ws = wb.active
ws.title = "Financial Dashboard"

# Add headers
headers = ["Month", "Revenue", "Expenses", "Profit"]
ws.append(headers)

# Style headers
for cell in ws[1]:
    cell.font = Font(bold=True)
    cell.fill = PatternFill(start_color="366092", fill_type="solid")

# Add sample data
data = [
    ["Q1", 150000, 90000, 60000],
    ["Q2", 175000, 95000, 80000],
    ["Q3", 200000, 100000, 100000],
    ["Q4", 225000, 110000, 115000]
]
for row in data:
    ws.append(row)

# Save file
wb.save("financial_dashboard.xlsx")
print("Spreadsheet created: financial_dashboard.xlsx")
```

### Option 3: Use CSV + Manual Import
Generate CSV data that users can import into Excel:

```python
import csv

data = [
    ["Month", "Revenue", "Expenses", "Profit"],
    ["Q1", 150000, 90000, 60000],
    ["Q2", 175000, 95000, 80000],
]

with open("data.csv", "w", newline="") as f:
    writer = csv.writer(f)
    writer.writerows(data)

print("CSV created: data.csv (import into Excel manually)")
```

## Original Anthropic API Instructions (For Reference)

When using Anthropic's official tools with container skills:

1. **Understand Requirements**: Parse data content, structure, formatting, charts, formulas
2. **Plan the Spreadsheet**: Sketch layout, identify calculations, determine chart types
3. **Create with Anthropic API**:
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
4. **Extract File Reference**: Locate file_id in response content blocks

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
