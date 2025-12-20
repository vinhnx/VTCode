# PDF Processor Configuration Guide

## Python Dependencies

Install required packages:

```bash
pip install PyPDF2 pdfplumber pandas openpyxl
```

## Script Configuration

### extract_text.py

- Supports PyPDF2 and pdfplumber backends
- pdfplumber recommended for better accuracy
- Default: Extracts first 500 chars per page

### extract_tables.py

- Uses pdfplumber for table detection
- Outputs CSV, JSON, or Excel format
- Handles merged cells with best-effort approach

### fill_form.py

- Requires reportlab for PDF generation
- Supports text fields, checkboxes, radio buttons
- Template-based form filling

### merge_pdfs.py

- Merges multiple PDFs in order
- Preserves bookmarks and metadata
- Handles password-protected PDFs

## Performance Tuning

- Large PDFs (>100 pages): Use streaming mode
- Batch processing: Process files in parallel
- Memory: Monitor usage with very large files

## Common Configuration Issues

**Issue**: PDF extraction returns garbled text
**Solution**: Try alternative backend (pdfplumber → PyPDF2 or vice versa)

**Issue**: Tables not detected correctly
**Solution**: Adjust table extraction parameters in script

**Issue**: Form filling fails
**Solution**: Verify PDF has fillable form fields
