---
name: pdf-generator-vtcode
description: Generate PDF documents using Python libraries (VTCode-compatible alternative to Anthropic's container skills)
version: 1.0.0
author: VTCode Team
---

# PDF Generator for VTCode

Generate professional PDF documents using Python libraries (reportlab, fpdf2). This is a VTCode-compatible alternative to Anthropic's container skills.

## Requirements

Install required Python libraries:
```bash
pip install reportlab fpdf2
```

## How to Use

When you need to generate a PDF, I'll:
1. Write Python code using reportlab or fpdf2
2. Execute it with VTCode's execute_code tool
3. Provide you with the generated PDF file

## Examples

### Example 1: Simple Text PDF
```
User: Create a PDF with "Hello World" in large text

Agent: I'll generate a simple PDF using fpdf2.

execute_code with Python:
```python
from fpdf import FPDF

pdf = FPDF()
pdf.add_page()
pdf.set_font('Arial', 'B', 24)
pdf.cell(0, 20, 'Hello World', 0, 1, 'C')
pdf.output('hello_world.pdf')
result = {"status": "PDF created", "filename": "hello_world.pdf"}
```

### Example 2: Report with Table
```
User: Generate a quarterly sales report PDF

Agent: I'll create a professional report with a table using reportlab.

execute_code with Python:
```python
from reportlab.pdfgen import canvas
from reportlab.lib.pagesizes import letter
from reportlab.platypus import SimpleDocTemplate, Paragraph, Table, TableStyle, Spacer
from reportlab.lib.styles import getSampleStyleSheet
from reportlab.lib import colors

doc = SimpleDocTemplate("sales_report.pdf", pagesize=letter)
styles = getSampleStyleSheet()
content = []

# Title
content.append(Paragraph("Q4 2024 Sales Report", styles['Title']))
content.append(Spacer(1, 20))

# Executive Summary
content.append(Paragraph("Executive Summary", styles['Heading1']))
content.append(Paragraph("This quarter exceeded targets by 15% with strong performance across all regions.", styles['Normal']))
content.append(Spacer(1, 12))

# Sales Data Table
data = [
    ['Region', 'Q3 Sales', 'Q4 Sales', 'Growth'],
    ['North America', '$450,000', '$520,000', '+15.6%'],
    ['Europe', '$380,000', '$440,000', '+15.8%'],
    ['Asia Pacific', '$320,000', '$380,000', '+18.8%'],
    ['Total', '$1,150,000', '$1,340,000', '+16.5%']
]
table = Table(data)
table.setStyle(TableStyle([
    ('BACKGROUND', (0,0), (-1,0), colors.HexColor('#4472C4')),
    ('TEXTCOLOR', (0,0), (-1,0), colors.white),
    ('ALIGN', (0,0), (-1,-1), 'CENTER'),
    ('FONTNAME', (0,0), (-1,0), 'Helvetica-Bold'),
    ('FONTSIZE', (0,0), (-1,0), 12),
    ('BOTTOMPADDING', (0,0), (-1,0), 12),
    ('BACKGROUND', (0,1), (-1,-1), colors.HexColor('#E7E6E6')),
    ('GRID', (0,0), (-1,-1), 1, colors.black)
]))
content.append(table)

doc.build(content)
result = {"status": "Sales report generated", "filename": "sales_report.pdf"}
```

### Example 3: Invoice
```
User: Create an invoice PDF for client "ABC Corp" for $5,000

Agent: I'll generate a professional invoice.

execute_code with Python:
```python
from fpdf import FPDF
from datetime import date

class InvoicePDF(FPDF):
    def header(self):
        self.set_font('Arial', 'B', 16)
        self.cell(0, 10, 'INVOICE', 0, 1, 'C')
        self.ln(5)
    
    def footer(self):
        self.set_y(-15)
        self.set_font('Arial', 'I', 8)
        self.cell(0, 10, f'Page {self.page_no()}', 0, 0, 'C')

pdf = InvoicePDF()
pdf.add_page()

# Invoice details
pdf.set_font('Arial', '', 12)
pdf.cell(0, 10, f'Date: {date.today().strftime("%B %d, %Y")}', 0, 1, 'R')
pdf.ln(5)

# Bill To
pdf.set_font('Arial', 'B', 12)
pdf.cell(0, 10, 'Bill To:', 0, 1)
pdf.set_font('Arial', '', 12)
pdf.cell(0, 10, 'ABC Corp', 0, 1)
pdf.cell(0, 10, '123 Business Ave', 0, 1)
pdf.cell(0, 10, 'City, State 12345', 0, 1)
pdf.ln(10)

# Table header
pdf.set_font('Arial', 'B', 12)
pdf.cell(100, 10, 'Description', 1)
pdf.cell(30, 10, 'Quantity', 1)
pdf.cell(30, 10, 'Rate', 1)
pdf.cell(30, 10, 'Amount', 1)
pdf.ln()

# Table row
pdf.set_font('Arial', '', 12)
pdf.cell(100, 10, 'Software Development Services', 1)
pdf.cell(30, 10, '100', 1, 0, 'C')
pdf.cell(30, 10, '$50.00', 1, 0, 'R')
pdf.cell(30, 10, '$5,000.00', 1, 0, 'R')
pdf.ln(20)

# Total
pdf.set_font('Arial', 'B', 14)
pdf.cell(160, 10, 'Total:', 0, 0, 'R')
pdf.cell(30, 10, '$5,000.00', 0, 0, 'R')

pdf.output('invoice_abc_corp.pdf')
result = {"status": "Invoice generated", "filename": "invoice_abc_corp.pdf"}
```

## Code Templates

### Template: Basic PDF with fpdf2
```python
from fpdf import FPDF

pdf = FPDF()
pdf.add_page()
pdf.set_font('Arial', 'B', 16)

# Your content here
pdf.cell(0, 10, 'Title', 0, 1, 'C')
pdf.set_font('Arial', '', 12)
pdf.multi_cell(0, 10, 'Your text content goes here...')

pdf.output('output.pdf')
result = {"status": "PDF created", "filename": "output.pdf"}
```

### Template: Professional Report with reportlab
```python
from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer
from reportlab.lib.styles import getSampleStyleSheet
from reportlab.lib.pagesizes import letter

doc = SimpleDocTemplate("report.pdf", pagesize=letter)
styles = getSampleStyleSheet()
content = []

# Add title
content.append(Paragraph("Report Title", styles['Title']))
content.append(Spacer(1, 20))

# Add sections
content.append(Paragraph("Section 1", styles['Heading1']))
content.append(Paragraph("Content for section 1...", styles['Normal']))

# Build PDF
doc.build(content)
result = {"status": "Report generated", "filename": "report.pdf"}
```

## Troubleshooting

**Error: "No module named 'reportlab'"**
- Solution: Run `pip install reportlab` in your terminal

**Error: "No module named 'fpdf'"**
- Solution: Run `pip install fpdf2` in your terminal

**PDF not appearing?**
- Check that the code executed successfully (exit_code = 0)
- Verify the filename in the result matches what you're looking for
- Ensure you're checking the correct workspace directory

## Library Documentation

- **reportlab**: https://www.reportlab.com/docs/reportlab-userguide.pdf
- **fpdf2**: https://pyfpdf.github.io/fpdf2/
- **weasyprint**: https://doc.courtbouillon.org/weasyprint/stable/
