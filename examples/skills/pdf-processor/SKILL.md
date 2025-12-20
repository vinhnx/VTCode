---
name: pdf-processor
description: Extract text and tables from PDF files, fill forms, and merge multiple PDF documents. Use when working with PDF files, forms, or document extraction tasks.
version: 1.0.0
license: MIT
compatibility: "Requires: Python 3.8+, PyPDF2, pdfplumber, internet access for API calls"
allowed-tools: "Read Write Bash Python"
metadata:
  author: document-tools-org
  category: document-processing
  requires_python: "true"
  requires_network: "false"
---

# PDF Processor Skill

## Overview

Process PDF documents to extract text, tables, and metadata. This skill can also fill PDF forms programmatically and merge multiple PDF files into a single document.

## When to Use This Skill

- Extract text content from PDF files
- Extract tables and structured data from PDFs
- Fill out PDF forms with dynamic data
- Merge multiple PDF documents
- Analyze PDF metadata and document structure

## Instructions

When a user asks to work with PDF files:

1. **Analyze the Request**: Determine what PDF operation is needed
   - Text extraction: Pull plain text from PDF pages
   - Table extraction: Extract tabular data in structured format
   - Form filling: Populate PDF form fields with data
   - PDF merging: Combine multiple PDFs into one document
   - Metadata analysis: Examine PDF properties and structure

2. **Check Prerequisites**: Ensure required Python packages are available
   - PyPDF2 for basic PDF operations
   - pdfplumber for advanced text/table extraction
   - reportlab for PDF generation (if creating new PDFs)

3. **Use Appropriate Scripts**: Run the correct script based on the operation needed
   - For text extraction: Run `scripts/extract_text.py`
   - For table extraction: Run `scripts/extract_tables.py`
   - For form filling: Run `scripts/fill_form.py`
   - For merging: Run `scripts/merge_pdfs.py`

4. **Handle Input/Output**: 
   - Accept PDF file paths as input
   - Provide extracted data in markdown, JSON, or CSV format as appropriate
   - Save generated/modified PDFs to specified locations
   - Include page numbers and section references in extracted text

5. **Error Handling**: 
   - Check if PDF files exist before processing
   - Validate PDF format and integrity
   - Handle encrypted/password-protected PDFs appropriately
   - Provide clear error messages for missing dependencies

## File References

- Text extraction script: `scripts/extract_text.py`
- Table extraction script: `scripts/extract_tables.py`
- Form filling script: `scripts/fill_form.py`
- PDF merging script: `scripts/merge_pdfs.py`
- Configuration guide: `references/config.md`
- Common issues: `references/troubleshooting.md`

## Examples

### Example 1: Extract Text from a Report

**User Input**: "Extract all text from quarterly-report.pdf"

**Process**:
1. Verify the file exists at the specified path
2. Run `scripts/extract_text.py quarterly-report.pdf`
3. Capture the output with page numbers

**Output**:
```
Extracted text from quarterly-report.pdf (12 pages):

Page 1:
Q3 2024 Financial Results...

Page 2:
Revenue increased by 15%...
```

### Example 2: Extract Tables from a Document

**User Input**: "Extract all tables from data-analysis.pdf and save as CSV"

**Process**:
1. Run table extraction script: `scripts/extract_tables.py data-analysis.pdf --format csv`
2. Save output to data-analysis-tables.csv
3. Preview the first few rows of extracted data

**Output**:
```
Extracted 3 tables from data-analysis.pdf:
- Table 1: 15 rows × 4 columns (Sales by Region)
- Table 2: 8 rows × 3 columns (Monthly Growth)
- Table 3: 12 rows × 5 columns (Product Performance)

Data saved to: data-analysis-tables.csv
```

### Example 3: Fill a PDF Form

**User Input**: "Fill out template-form.pdf with user data"

**Process**:
1. Parse the provided user data
2. Run form filler: `scripts/fill_form.py template-form.pdf --data user-data.json`
3. Generate completed-form.pdf

**Output**:
```
Successfully filled template-form.pdf
Output saved to: completed-form.pdf
Fields populated: 12/12
```

## Best Practices

- Always verify PDF files exist before processing
- Handle large PDFs (100+ pages) with appropriate memory management
- Preserve original PDF formatting when possible
- Provide progress updates for long-running operations
- Validate extracted data for accuracy
- Support both single PDF and batch processing modes

## Common Issues and Solutions

**Issue**: PDF is password protected
**Solution**: Request password from user or skip if not available

**Issue**: PDF contains scanned images (not text)
**Solution**: Use OCR tools or inform user that text extraction isn't possible

**Issue**: Tables have complex merged cells
**Solution**: Use best-effort extraction and note any ambiguities

## Performance Considerations

- Text extraction: Fast (~1-2 seconds per 10 pages)
- Table extraction: Moderate (~3-5 seconds per table)
- Form filling: Fast (<1 second per form)
- PDF merging: Fast (<2 seconds for 10 PDFs)

## Related Skills

- `document-converter` - Convert between document formats (PDF, Word, HTML)
- `ocr-text-extractor` - Extract text from scanned documents
- `data-visualizer` - Create charts and visualizations from extracted data
