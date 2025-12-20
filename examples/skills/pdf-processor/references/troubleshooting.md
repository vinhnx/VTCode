# PDF Processor Troubleshooting

## Common Issues and Solutions

### 1. PDF is Password Protected

**Error**: `PyPDF2.errors.FileNotDecrypted`

**Solutions**:
- Request password from user
- Skip processing and inform user
- Note: Scripts don't support password cracking

### 2. PDF Contains Scanned Images (No Text)

**Symptom**: Extraction returns empty or very little text

**Solutions**:
- Inform user that text extraction isn't possible
- Suggest using OCR tools instead
- Check if PDF contains selectable text first

### 3. Garbled or Encoded Text

**Symptom**: Extracted text shows symbols/encoding issues

**Solutions**:
- Try switching extraction backend (pdfplumber ↔ PyPDF2)
- Check PDF encoding and fonts
- Some PDFs use non-standard encoding

### 4. Tables Not Detected

**Symptom**: Table extraction finds no tables or incomplete data

**Solutions**:
- Adjust table detection parameters in script
- Some PDFs use images for tables (not extractable)
- Check table lines are clearly defined in PDF

### 5. Form Fields Not Fillable

**Symptom**: Form filling script fails or does nothing

**Solutions**:
- Verify PDF actually has fillable form fields
- Check field names match data keys
- Some PDFs have flattened forms (not fillable)

### 6. Large PDF Memory Issues

**Symptom**: Process runs out of memory on large PDFs

**Solutions**:
- Process in chunks (page ranges)
- Use streaming extraction mode
- Increase system memory or use smaller PDFs

### 7. Special Characters Lost

**Symptom**: Accented characters or non-ASCII text not preserved

**Solutions**:
- Ensure proper encoding handling in scripts
- Use UTF-8 for output files
- Some fonts may not map correctly

### 8. PDF Merge Loses Bookmarks

**Symptom**: Merged PDF loses navigation/bookmarks

**Solutions**:
- Check merge script preserves document structure
- Some PDF readers handle bookmarks differently
- May need to manually recreate bookmarks

## Getting Help

If you encounter issues not listed here:
1. Check the PDF with multiple tools to verify it's valid
2. Try the operation with a different PDF to isolate the issue
3. Check the script logs for specific error messages
4. Verify all dependencies are correctly installed
