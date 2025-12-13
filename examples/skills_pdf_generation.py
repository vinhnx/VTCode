#!/usr/bin/env python3
"""
Example: Generate PDFs with Anthropic Agent Skills

This example demonstrates how to use the PDF Skill with Claude API
to generate PDF documents programmatically.

Requirements:
    - anthropic >= 0.18.0
    - python >= 3.7

Usage:
    export ANTHROPIC_API_KEY=your-key-here
    python examples/skills_pdf_generation.py
"""

import anthropic
import os
import sys


def generate_invoice_pdf():
    """
    Generate an invoice PDF using Agent Skills.
    """
    
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("Error: ANTHROPIC_API_KEY environment variable not set")
        sys.exit(1)
    
    client = anthropic.Anthropic(api_key=api_key)
    
    print("Generating invoice PDF with Agent Skills...")
    print("-" * 60)
    
    response = client.messages.create(
        model="claude-3-5-sonnet-20241022",
        max_tokens=4096,
        tools=[
            {
                "type": "code_execution",
                "name": "bash",
                "description": "Execute bash commands"
            }
        ],
        messages=[
            {
                "role": "user",
                "content": """Generate a professional PDF invoice with:

Header:
- Company: TechCore Solutions
- Invoice #: INV-2024-1847
- Date: December 13, 2024
- Due Date: December 27, 2024

Bill To:
- Customer: Acme Corporation
- Address: 123 Business St, New York, NY 10001
- Contact: John Doe, john@acme.com

Items:
1. Cloud Infrastructure Setup - $2,500.00
2. Security Audit & Compliance Review - $1,800.00
3. Staff Training (40 hours @ $75/hr) - $3,000.00
4. Monthly Support & Maintenance - $1,200.00

Summary:
- Subtotal: $8,500.00
- Tax (10%): $850.00
- Total Due: $9,350.00

Footer:
- Bank details for payment
- Terms: Net 14 days
- Thank you message

Formatting:
- Professional header with logo space
- Clear itemization
- Proper currency formatting
- Professional styling and colors"""
            }
        ],
        betas=[
            "code-execution-2025-08-25",
            "skills-2025-10-02"
        ]
    )
    
    print("\nInvoice PDF Generated:")
    print("-" * 60)
    
    for block in response.content:
        if hasattr(block, 'text'):
            print(block.text)
        elif hasattr(block, 'type'):
            print(f"[{block.type}]")
    
    return response


def generate_report_pdf():
    """
    Generate a data report PDF with charts and analysis.
    """
    
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("Error: ANTHROPIC_API_KEY environment variable not set")
        sys.exit(1)
    
    client = anthropic.Anthropic(api_key=api_key)
    
    print("\nGenerating report PDF...")
    print("-" * 60)
    
    response = client.messages.create(
        model="claude-3-5-sonnet-20241022",
        max_tokens=4096,
        tools=[
            {
                "type": "code_execution",
                "name": "bash",
                "description": "Execute bash commands"
            }
        ],
        messages=[
            {
                "role": "user",
                "content": """Create a professional PDF report with:

Title Page:
- Title: "Annual Performance Report 2024"
- Subtitle: "Financial Analysis & Market Trends"
- Date: December 2024

Executive Summary:
- Key finding: Revenue up 28% year-over-year
- Market expansion into 3 new regions
- Customer satisfaction at all-time high (96%)

Section 1: Revenue Analysis
- 2024 monthly revenue data (Jan-Dec)
- Include a line chart showing growth trend
- Comparison with 2023 performance

Section 2: Regional Performance
- Sales by region (North, South, East, West)
- Include a bar chart
- Top performing markets

Section 3: Customer Metrics
- Customer acquisition: 450 new customers
- Retention rate: 92%
- Average customer lifetime value: $15,000
- Include metrics dashboard

Section 4: Strategic Recommendations
- Focus on high-growth markets
- Increase R&D investment
- Expand sales team
- Improve customer experience initiatives

Appendix:
- Detailed monthly financial data table
- Market research methodology

Styling:
- Professional layout
- Page numbers and headers
- Color-coded sections
- Proper spacing and alignment
- Professional fonts"""
            }
        ],
        betas=[
            "code-execution-2025-08-25",
            "skills-2025-10-02"
        ]
    )
    
    print("\nReport PDF Generated:")
    text_blocks = [block for block in response.content if hasattr(block, 'text')]
    if text_blocks:
        text = text_blocks[0].text
        if len(text) > 500:
            print(text[:500] + "...")
        else:
            print(text)
    
    # Print file references
    print("\nGenerated Files:")
    for block in response.content:
        if hasattr(block, 'type') and block.type == 'file':
            print(f"  - File ID: {block.file_id}")
    
    return response


if __name__ == "__main__":
    try:
        # Generate invoice PDF
        generate_invoice_pdf()
        
        # Generate report PDF
        generate_report_pdf()
        
    except anthropic.APIError as e:
        print(f"API Error: {e}")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
