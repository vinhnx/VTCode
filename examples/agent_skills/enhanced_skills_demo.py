#!/usr/bin/env python3
"""
Enhanced Skills Usage Demo for VT Code

This demonstrates proper implementation of skills following the Claude skills guide:
https://platform.claude.com/docs/en/build-with-claude/skills-guide/

Key improvements:
1. Skills provide instructions, not automatic execution
2. Use execute_code instead of run_pty_cmd for Python
3. Environment awareness and fallback implementations
4. Proper error handling and verification
5. Follow existing VT Code patterns
"""

import os
import sys
import json
from datetime import datetime

def check_environment():
    """Check what libraries and tools are available."""
    print(" Environment Check:")
    print("-" * 50)
    
    # Check Python version
    print(f"Python version: {sys.version}")
    
    # Check for key libraries
    libraries = {
        'anthropic': 'For API calls',
        'fpdf': 'For PDF generation',
        'reportlab': 'Alternative PDF library',
        'matplotlib': 'For charts',
        'pandas': 'For data processing'
    }
    
    available = {}
    for lib, description in libraries.items():
        try:
            __import__(lib)
            print(f" {lib}: Available ({description})")
            available[lib] = True
        except ImportError:
            print(f" {lib}: Not available ({description})")
            available[lib] = False
    
    print("-" * 50)
    return available

def generate_pdf_with_fallback(content_spec):
    """
    Generate PDF using available libraries with fallback options.
    Follows Claude skills guide best practices.
    """
    print(f" Generating PDF: {content_spec.get('title', 'Untitled')}")
    
    # Check environment first
    env = check_environment()
    
    try:
        if env.get('anthropic'):
            return generate_pdf_with_anthropic(content_spec)
        elif env.get('fpdf'):
            return generate_pdf_with_fpdf(content_spec)
        elif env.get('reportlab'):
            return generate_pdf_with_reportlab(content_spec)
        else:
            return generate_pdf_mock(content_spec)
    except Exception as e:
        print(f" PDF generation failed: {e}")
        return generate_pdf_mock(content_spec)

def generate_pdf_with_anthropic(content_spec):
    """Use Anthropic API with Skills for PDF generation."""
    print("Using Anthropic Skills for professional PDF generation...")
    
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("  ANTHROPIC_API_KEY not found, falling back to mock generation")
        return generate_pdf_mock(content_spec)
    
    try:
        import anthropic
        
        client = anthropic.Anthropic(api_key=api_key)
        
        # Build specification from content_spec
        spec = f"""
        Generate a professional PDF document with:
        Title: {content_spec.get('title', 'Document')}
        Type: {content_spec.get('type', 'report')}
        
        Content:
        {json.dumps(content_spec.get('content', {}), indent=2)}
        
        Requirements:
        - Professional formatting
        - Clear sections and headers
        - Proper typography
        - Business-appropriate styling
        """
        
        response = client.messages.create(
            model="claude-4-5-sonnet",
            max_tokens=4096,
            tools=[{"type": "code_execution", "name": "bash"}],
            messages=[{"role": "user", "content": spec}],
            container={
                "type": "skills",
                "skills": [{"type": "anthropic", "skill_id": "pdf", "version": "latest"}]
            },
            betas=["code-execution-2025-08-25", "skills-2025-10-02"]
        )
        
        # Extract file references
        files = []
        for block in response.content:
            if hasattr(block, 'type') and block.type == 'file':
                files.append(block.file_id)
        
        print(f" PDF generated via Anthropic Skills")
        print(f" File IDs: {files}")
        
        return {
            'success': True,
            'method': 'anthropic_skills',
            'files': files,
            'response': response
        }
        
    except Exception as e:
        print(f"  Anthropic Skills failed: {e}")
        return generate_pdf_mock(content_spec)

def generate_pdf_with_fpdf(content_spec):
    """Use FPDF library for PDF generation."""
    print("Using FPDF library for PDF generation...")
    
    from fpdf import FPDF
    
    pdf = FPDF()
    pdf.add_page()
    
    # Title
    pdf.set_font('Arial', 'B', 20)
    pdf.cell(0, 15, content_spec.get('title', 'Document'), 0, 1, 'C')
    
    # Date
    pdf.set_font('Arial', 'I', 10)
    pdf.cell(0, 10, f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')}", 0, 1, 'C')
    pdf.ln(10)
    
    # Content sections
    content = content_spec.get('content', {})
    
    for section, data in content.items():
        pdf.set_font('Arial', 'B', 14)
        pdf.cell(0, 10, section, 0, 1)
        
        pdf.set_font('Arial', '', 11)
        if isinstance(data, dict):
            for key, value in data.items():
                pdf.cell(0, 8, f"• {key}: {value}", 0, 1)
        else:
            pdf.multi_cell(0, 8, str(data))
        
        pdf.ln(5)
    
    # Save file
    filename = f"/tmp/{content_spec.get('filename', 'document')}.pdf"
    pdf.output(filename)
    
    print(f" PDF generated with FPDF: {filename}")
    
    return {
        'success': True,
        'method': 'fpdf',
        'file': filename,
        'size': os.path.getsize(filename)
    }

def generate_pdf_mock(content_spec):
    """Mock PDF generation when no libraries are available."""
    print("Using mock PDF generation (no PDF libraries available)...")
    
    # Create structured text content
    content = []
    content.append("=" * 60)
    content.append(f"MOCK PDF: {content_spec.get('title', 'Document')}")
    content.append("=" * 60)
    content.append(f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')}")
    content.append("")
    
    # Add sections
    spec_content = content_spec.get('content', {})
    for section, data in spec_content.items():
        content.append(f"\n{section.upper()}")
        content.append("-" * 40)
        if isinstance(data, dict):
            for key, value in data.items():
                content.append(f"• {key}: {value}")
        else:
            content.append(str(data))
    
    content.append("
" + "=" * 60)
    content.append("Note: This is a mock PDF representation.")
    content.append("Install fpdf or reportlab for actual PDF generation.")
    content.append("=" * 60)
    
    # Save as text file
    filename = f"/tmp/{content_spec.get('filename', 'document')}.txt"
    with open(filename, 'w') as f:
        f.write('\n'.join(content))
    
    print(f" Mock PDF saved as text: {filename}")
    
    return {
        'success': True,
        'method': 'mock',
        'file': filename,
        'size': os.path.getsize(filename),
        'note': 'Mock generation - no PDF libraries available'
    }

def main():
    """Main function demonstrating enhanced skills usage."""
    print(" Enhanced Skills Usage Demo")
    print("=" * 60)
    
    # Example 1: Monthly Sales Report
    sales_report = {
        'title': 'Monthly Sales Report',
        'type': 'financial_report',
        'filename': 'monthly_sales_report',
        'content': {
            'Executive Summary': {
                'Revenue Growth': '+15% vs last month',
                'Total Sales': '$125,000',
                'Key Insight': 'Strong performance in North region'
            },
            'Sales by Region': {
                'North': '$45,000',
                'South': '$32,000', 
                'East': '$28,000',
                'West': '$20,000'
            },
            'Top Products': {
                'Product A': '$35,000',
                'Product B': '$28,000',
                'Product C': '$22,000'
            },
            'Recommendations': 'Focus marketing efforts on underperforming regions'
        }
    }
    
    # Generate the report
    result1 = generate_pdf_with_fallback(sales_report)
    
    print("\n" + "=" * 60)
    
    # Example 2: Project Status Report
    project_report = {
        'title': 'Project Status Report',
        'type': 'project_report',
        'filename': 'project_status_report',
        'content': {
            'Project Overview': {
                'Name': 'VT Code Enhancement',
                'Status': 'In Progress',
                'Completion': '75%',
                'Deadline': '2024-12-31'
            },
            'Key Milestones': {
                'Skills Integration': 'Complete',
                'Error Handling': 'Complete',
                'Documentation': 'In Progress',
                'Testing': 'Pending'
            },
            'Issues & Risks': {
                'High Priority': 'None',
                'Medium Priority': 'Performance optimization needed',
                'Low Priority': 'UI polish items'
            }
        }
    }
    
    result2 = generate_pdf_with_fallback(project_report)
    
    # Summary
    print("\n" + "=" * 60)
    print(" Generation Summary:")
    print(f"Report 1: {result1['method']} - Success: {result1['success']}")
    print(f"Report 2: {result2['method']} - Success: {result2['success']}")
    
    # List generated files
    print("\n Generated Files:")
    import glob
    files = glob.glob("/tmp/*_report.*")
    for file in files:
        size = os.path.getsize(file)
        print(f"  • {file} ({size} bytes)")
    
    print("\n Enhanced skills usage demo completed!")
    print("\nKey improvements implemented:")
    print("  • Environment awareness and fallback handling")
    print("  • Proper error handling and verification")
    print("  • Multiple implementation methods (Anthropic, FPDF, mock)")
    print("  • Clear user feedback and file verification")
    print("  • Following Claude skills guide best practices")

if __name__ == "__main__":
    main()