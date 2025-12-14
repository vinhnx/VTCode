# VT Code Skills Usage Best Practices

## Core Principle: Skills are Instructions, Not Execution

Skills in VT Code provide **guidance and instructions** - they don't execute automatically. Think of them as detailed recipes that you must follow using available tools.

## Correct Skill Usage Pattern

### 1. Load Skill Once
```rust
// Load skill instructions into context
skill(name="pdf-report-generator")
```

### 2. Read and Follow Instructions
The skill provides specific implementation guidance. Follow it exactly using appropriate tools.

### 3. Implement Using Available Tools
Use `execute_code`, `write_file`, or other tools to implement the skill's instructions.

## Enhanced PDF Generation Example

Following the Claude skills guide, here's the corrected approach:

```rust
// Step 1: Load skill (once only)
skill(name="pdf-report-generator")

// Step 2: Use execute_code for Python implementation
execute_code(
    language="python3",
    code="""
import os
import json

# Check if we have the required environment
print("Checking environment...")
print(f"Python version: {__import__('sys').version}")

# Since we may not have anthropic library, let's create a mock PDF generation
# This demonstrates proper skill implementation pattern

# Create a simple PDF using available libraries
try:
    from fpdf import FPDF
    
    # Generate PDF following the skill's guidance
    pdf = FPDF()
    pdf.add_page()
    
    # Title page
    pdf.set_font('Arial', 'B', 24)
    pdf.cell(0, 20, 'Monthly Sales Report', 0, 1, 'C')
    
    pdf.set_font('Arial', '', 12)
    pdf.cell(0, 10, 'Generated using VT Code Skills', 0, 1, 'C')
    pdf.ln(20)
    
    # Executive Summary section
    pdf.set_font('Arial', 'B', 16)
    pdf.cell(0, 10, 'Executive Summary', 0, 1)
    pdf.set_font('Arial', '', 12)
    pdf.multi_cell(0, 10, 'This report provides an overview of monthly sales performance and key metrics.')
    pdf.ln(10)
    
    # Sales Figures section
    pdf.set_font('Arial', 'B', 16)
    pdf.cell(0, 10, 'Sales Figures', 0, 1)
    pdf.set_font('Arial', '', 12)
    
    # Simple table
    pdf.cell(60, 10, 'Region', 1)
    pdf.cell(60, 10, 'Sales', 1)
    pdf.cell(60, 10, 'Growth', 1)
    pdf.ln()
    
    pdf.cell(60, 10, 'North', 1)
    pdf.cell(60, 10, '$150,000', 1)
    pdf.cell(60, 10, '+12%', 1)
    pdf.ln()
    
    pdf.cell(60, 10, 'South', 1)
    pdf.cell(60, 10, '$120,000', 1)
    pdf.cell(60, 10, '+8%', 1)
    pdf.ln()
    
    # Conclusions
    pdf.ln(10)
    pdf.set_font('Arial', 'B', 16)
    pdf.cell(0, 10, 'Conclusions', 0, 1)
    pdf.set_font('Arial', '', 12)
    pdf.multi_cell(0, 10, 'Overall performance shows positive growth across all regions.')
    
    # Save to workspace
    output_path = '/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/monthly_sales_report.pdf'
    pdf.output(output_path)
    
    print(f"PDF generated successfully: {output_path}")
    print(f"File size: {os.path.getsize(output_path)} bytes")
    
except ImportError:
    print("FPDF not available, creating mock PDF structure")
    # Create mock PDF content
    pdf_content = """
MONTHLY SALES REPORT

EXECUTIVE SUMMARY
This report provides an overview of monthly sales performance.

SALES FIGURES
Region    Sales      Growth
North     $150,000   +12%
South     $120,000   +8%

CONCLUSIONS
Overall performance shows positive growth.
"""
    
    output_path = '/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/monthly_sales_report.txt'
    with open(output_path, 'w') as f:
        f.write(pdf_content)
    
    print(f"Mock PDF content saved: {output_path}")
"""
)

// Step 3: Verify file creation
list_files(path="/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode", pattern="*.pdf")
```

## Key Improvements from Claude Skills Guide

### 1. **Environment Awareness**
- Check for available libraries before use
- Provide fallback implementations
- Handle missing dependencies gracefully

### 2. **Proper Tool Selection**
- Use `execute_code` instead of `run_pty_cmd` for Python
- Choose appropriate language (python3 vs python)
- Leverage workspace file system

### 3. **Verification and Validation**
- Always verify file creation
- Check file sizes and locations
- Provide meaningful error messages

### 4. **Skill Implementation Pattern**
```rust
1. Load skill instructions
2. Read and understand requirements
3. Choose appropriate implementation approach
4. Use execute_code with proper error handling
5. Verify results
6. Provide clear output to user
```

## Common Pitfalls to Avoid

1. **Don't expect automatic execution** - Skills provide guidance only
2. **Don't use run_pty_cmd for Python** - Use execute_code instead
3. **Don't assume library availability** - Check and provide fallbacks
4. **Don't claim success without verification** - Always check file existence
5. **Don't reload skills repeatedly** - Load once per session

## Enhanced Error Handling

```rust
// Better error handling pattern
execute_code(
    language="python3",
    code="""
try:
    # Main implementation
    result = generate_pdf()
    print(f"SUCCESS: PDF generated at {result}")
except ImportError as e:
    print(f"LIBRARY_MISSING: {e}")
    # Provide alternative approach
except Exception as e:
    print(f"ERROR: {type(e).__name__}: {e}")
    # Provide helpful guidance
"""
)
```

This approach follows the Claude skills guide by being more robust, environment-aware, and user-friendly.