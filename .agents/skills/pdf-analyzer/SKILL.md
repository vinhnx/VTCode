---
name: pdf-analyzer
version: 1.0.0
description: Analyze PDF, DOCX, and spreadsheet documents using vision models. Converts documents to images and extracts insights with layout preservation. Uses VT Code's native document processor (no container skills required).
author: VT Code Team
tags: [document, pdf, docx, excel, vision, vtcode-native]
vtcode-native: true
---

# PDF/DOCX/Spreadsheet Analyzer

## Overview

This skill provides OpenAI-style document processing capabilities, converting PDFs, Word documents, and spreadsheets into rendered images for vision model analysis. This approach preserves layout, formatting, tables, and visual elements that would be lost in traditional text extraction.

**Inspired by:** OpenAI's implementation in ChatGPT's Code Interpreter (per Simon Willison's blog post, Dec 2025)

## Supported Formats

- **PDF**: Multi-page documents, converted page-by-page to PNGs
- **DOCX/DOC**: Word documents with full formatting preservation
- **XLSX/XLS/CSV**: Spreadsheets rendered as visual tables
- **Images**: Direct analysis (PNG, JPG, JPEG, GIF, BMP, TIFF)

## Usage in VT Code

### Basic Document Analysis

```bash
# List available skills
vtcode list-skills

# Load the skill
vtcode skills load pdf-analyzer

# In VT Code chat:
/skills load pdf-analyzer
```

### Analyzing Documents

```
User: Analyze this PDF report and summarize the key findings
Assistant: I'll use the PDF analyzer skill to process this document with vision-based analysis.

[Skill processes document, converts to images, analyzes with vision model]

The document contains:
1. Executive summary on page 1 highlighting Q3 revenue growth
2. Detailed financial tables on pages 3-4 showing 15% YoY growth
3. Charts illustrating market expansion opportunities
4. Risk assessment section on page 7

Key insights:
- Revenue increased from $2.1M to $2.4M (14.3% growth)
- New customer acquisition up 23%
- Challenges noted in supply chain efficiency
```

### Processing Spreadsheets

```
User: Review this Excel file and identify any anomalies in the sales data
Assistant: Let me analyze the spreadsheet using vision-based processing to preserve the table structure and formatting.

[Skill renders spreadsheet as image, detects structure, analyzes values]

Findings:
- Row 42: Sales amount of $50,000 with no corresponding customer ID (potential data entry error)
- Column E: Several negative values that may indicate returns or refunds
- Week of 2025-03-15: Unusual 300% spike in orders (verify with sales team)
- Conditional formatting shows 12 overdue payments highlighted in red
```

### Document Comparison

```
User: Compare these two contract versions and highlight the changes
Assistant: I'll process both documents using vision analysis to detect textual and formatting changes.

[Skill processes both documents, compares rendered pages]

Differences detected:
- Section 3.2: Payment terms changed from "Net 30" to "Net 15"
- Clause 7.1: Liability cap increased from $100k to $250k
- Page 5: New confidentiality provision added
- Header formatting: Company logo position adjusted
```

## Technical Implementation

### Architecture

```
Document → Renderer → PNG Images → Vision Model → Analysis → Insights
```

1. **Document Loading**: Supports PDF, DOCX, XLSX, CSV, and image formats
2. **Rendering**: Converts pages/sheets to high-quality PNG images (150 DPI default)
3. **Vision Processing**: Sends images to vision-enabled LLM for analysis
4. **Text Extraction**: Optionally runs OCR for searchable text layer
5. **Insight Generation**: Combines visual analysis with extracted content

### Key Features

**Layout Preservation:**
- Tables maintain row/column structure
- Charts and graphs remain visually intact
- Formatting (bold, italic, colors) is preserved
- Page structure and flow are maintained

**Multi-page Processing:**
- Automatic pagination for long documents
- Cross-page context awareness
- Page number references in analysis

**Vision Model Integration:**
- Sends rendered images to Gemini/Claude/GPT-4 vision models
- Maintains spatial relationships in analysis
- Extracts both textual and visual information

### Token Efficiency

- Images compressed to optimal size for vision models
- Smart page selection for long documents
- Caching of rendered pages
- Configurable DPI (75/150/300) based on detail requirements

## Configuration

### Environment Variables

```bash
# Document processing
export VT_CODE_DOC_DPI=150          # Rendering DPI (default: 150)
export VT_CODE_DOC_MAX_PAGES=50     # Max pages per document (default: 50)
export VT_CODE_DOC_ENABLE_OCR=true  # Enable OCR fallback (default: true)

# Vision model selection
export VT_CODE_VISION_MODEL=gemini-2.5-pro  # Model for document analysis
```

### VTCode Configuration

```toml
[skills.pdf-analyzer]
enabled = true
dpi = 150
max_pages = 50
enable_ocr = true
vision_model = "gemini-2.5-pro"
```

## Examples

### Example 1: Financial Report Analysis

```
User: Analyze Q3_Financial_Report.pdf and give me the key metrics

Assistant: Processing the financial report using vision-based analysis...

Key Metrics Extracted:
• Total Revenue: $2.4M (Q3 2025)
• Gross Margin: 68%
• Operating Expenses: $1.2M
• Net Income: $432K (18% margin)
• Cash Flow: +$156K from operations

Trends:
• Revenue up 14.3% vs Q2
• Margin improvement of 2.1 percentage points
• Customer acquisition cost decreased 8%

[Tables and charts referenced by page number]
```

### Example 2: Contract Review

```
User: Review this NDA and highlight any unusual clauses

Assistant: Analyzing the Non-Disclosure Agreement...

Contract Review Summary:
✓ Standard confidentiality provisions
✓ Reasonable duration (2 years)
⚠️  Unusual clause found: Section 5.3 - "Perpetual confidentiality for trade secrets"
⚠️  Geographic scope: Worldwide (typical, but confirm if appropriate)
✓ Standard exclusions for public information

Recommendation: The perpetual trade secret clause is enforceable but consider:
- Defining "trade secrets" more specifically
- Adding a severability provision
```

### Example 3: Research Paper Analysis

```
User: Summarize this research paper and extract the methodology

Assistant: Processing the research paper with vision analysis...

Paper Summary:
"Advanced Techniques in Machine Learning Optimization"
Authors: Smith et al. (2025)

Abstract Summary:
The paper presents a novel optimization algorithm that improves convergence
speed by 40% compared to Adam optimizer...

Methodology (from pages 3-5):
1. Dataset: CIFAR-10, ImageNet, and custom benchmarks
2. Architecture: ResNet-50, ViT-B/16
3. Hyperparameters: Learning rate 1e-3, batch size 256
4. Novel contribution: Adaptive momentum scheduling

Results (pages 6-7):
• 40% faster convergence vs baseline
• 2.3% accuracy improvement on ImageNet
• Robust to hyperparameter variations

[Referenced figures: Fig 2 (convergence curve), Fig 4 (accuracy comparison)]
```

## Advanced Usage

### Batch Processing

```bash
# Process multiple documents
for doc in *.pdf; do
  vtcode ask "Analyze $doc and save summary to ${doc%.pdf}_summary.txt"
done
```

### Integration with CLI

```bash
# Process document and extract specific information
vtcode skills load pdf-analyzer
vtcode ask "Extract all email addresses from contract.pdf"
```

### Custom Vision Prompts

```
User: Analyze this document focusing only on financial data and risks

Assistant: I'll use a custom vision prompt to focus specifically on financial information and risk factors...

[Custom prompt guides model to focus on specific elements]
```

## Troubleshooting

### Common Issues

**Document too large:**
```
Error: Document exceeds maximum page limit (50 pages)
Solution: Split document or increase VT_CODE_DOC_MAX_PAGES
```

**Vision model unavailable:**
```
Error: Vision model not configured
Solution: Set VT_CODE_VISION_MODEL or use default Gemini vision model
```

**Rendering fails:**
```
Error: Unable to render document
Solution: Verify file format is supported and not corrupted
```

### Debug Mode

```bash
export VT_CODE_DEBUG_DOCUMENTS=true
vtcode skills load pdf-analyzer
vtcode ask "Analyze document.pdf"
# Will show detailed rendering and processing logs
```

## Performance Tips

1. **Use appropriate DPI**: 150 DPI is good for most documents
2. **Limit page range**: Process only relevant pages for large documents
3. **Batch processing**: Process multiple documents in parallel
4. **Cache results**: Reuse processed documents when possible
5. **Choose efficient vision models**: Gemini 2.5 Flash for speed, Pro for detail

## Security Considerations

- Documents processed locally, not sent to external services
- Vision model calls use standard LLM APIs with encryption
- Temporary files cleaned up after processing
- No document content stored permanently

## License

MIT License - See VTCode main repository for details.