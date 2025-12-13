#!/usr/bin/env python3
"""
Example: Create a spreadsheet with Anthropic Agent Skills

This example demonstrates how to use the Excel (xlsx) Skill with Claude API
to generate spreadsheets programmatically.

Requirements:
    - anthropic >= 0.18.0
    - python >= 3.7

Usage:
    export ANTHROPIC_API_KEY=your-key-here
    python examples/skills_spreadsheet.py
"""

import anthropic
import os
import sys


def create_spreadsheet_example():
    """
    Create an Excel spreadsheet with climate data using Agent Skills.
    
    This example shows:
    - Enabling Agent Skills in the API
    - Using code execution with skills
    - Handling file outputs from skills
    """
    
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("Error: ANTHROPIC_API_KEY environment variable not set")
        sys.exit(1)
    
    client = anthropic.Anthropic(api_key=api_key)
    
    print("Creating spreadsheet with Agent Skills...")
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
                "content": """Create an Excel spreadsheet with the following structure:
                
Sheet 1 - Climate Data:
- Column A: City names (New York, London, Tokyo, Sydney, Dubai)
- Column B: Average Temperature (°C)
- Column C: Humidity (%)
- Column D: Annual Precipitation (mm)
- Column E: Climate Zone

Add realistic data for each city and format the header row with bold text.
Include a summary sheet with statistics.

Make the spreadsheet professional-looking with proper column widths."""
            }
        ],
        betas=[
            "code-execution-2025-08-25",
            "skills-2025-10-02"
        ]
    )
    
    print("\nResponse from Claude:")
    print("-" * 60)
    
    # Print all content blocks
    for block in response.content:
        if hasattr(block, 'text'):
            print(block.text)
        elif hasattr(block, 'type'):
            print(f"[{block.type}]")
    
    # Look for file references
    print("\n" + "-" * 60)
    print("File References:")
    for block in response.content:
        if hasattr(block, 'type') and block.type == 'file':
            print(f"  File ID: {block.file_id}")
            print(f"  MIME Type: {block.mime_type if hasattr(block, 'mime_type') else 'N/A'}")
    
    return response


def create_financial_spreadsheet():
    """
    Create a financial spreadsheet with formulas and charts.
    """
    
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("Error: ANTHROPIC_API_KEY environment variable not set")
        sys.exit(1)
    
    client = anthropic.Anthropic(api_key=api_key)
    
    print("\nCreating financial spreadsheet...")
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
                "content": """Create an Excel spreadsheet for Q4 2024 financial analysis:

Quarterly Revenue:
- Q1 2024: $450,000
- Q2 2024: $520,000
- Q3 2024: $580,000
- Q4 2024: $720,000

Expenses by Category:
- Salaries: 40% of revenue
- Operations: 25% of revenue
- Marketing: 20% of revenue
- Other: 15% of revenue

Include:
1. Revenue table with growth percentage column
2. Expense breakdown table
3. Net profit calculations
4. A summary dashboard with key metrics
5. Proper formatting and colors"""
            }
        ],
        betas=[
            "code-execution-2025-08-25",
            "skills-2025-10-02"
        ]
    )
    
    print("\nFinancial Spreadsheet Response:")
    for block in response.content:
        if hasattr(block, 'text'):
            print(block.text[:500] + "...")
        elif hasattr(block, 'type'):
            print(f"[{block.type}]")
    
    return response


if __name__ == "__main__":
    # Run first example
    try:
        create_spreadsheet_example()
    except anthropic.APIError as e:
        print(f"API Error: {e}")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
    
    # Run second example
    try:
        create_financial_spreadsheet()
    except anthropic.APIError as e:
        print(f"API Error: {e}")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
