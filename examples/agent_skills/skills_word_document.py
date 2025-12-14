#!/usr/bin/env python3
"""
Example: Create a Word document with Anthropic Agent Skills

This example demonstrates how to use the Word (docx) Skill with Claude API
to generate documents programmatically.

Requirements:
    - anthropic >= 0.18.0
    - python >= 3.7

Usage:
    export ANTHROPIC_API_KEY=your-key-here
    python examples/skills_word_document.py
"""

import anthropic
import os
import sys


def create_word_document():
    """
    Create a Word document with formatted content using Agent Skills.
    """

    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("Error: ANTHROPIC_API_KEY environment variable not set")
        sys.exit(1)

    client = anthropic.Anthropic(api_key=api_key)

    print("Creating Word document with Agent Skills...")
    print("-" * 60)

    response = client.messages.create(
        model="claude-4-5-sonnet",
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
                "content": """Create a professional Word document with the following structure:

Title: "Quarterly Business Report - Q4 2024"

1. Executive Summary (paragraph)
   - Brief overview of key metrics and achievements

2. Performance Metrics (section with bullet points)
   - Revenue growth: 15% YoY
   - Customer satisfaction: 94%
   - Market share: Up 2.5%
   - New customers: 125

3. Key Achievements (section with numbered list)
   1. Launched new product line
   2. Expanded to 3 new markets
   3. Increased team by 20%
   4. Improved efficiency by 30%

4. Financial Summary (with a table)
   - Include columns: Quarter, Revenue, Expenses, Net Profit
   - Include 4 rows of data

5. Recommendations (bullet points)
   - Invest in R&D
   - Expand marketing efforts
   - Hire additional staff

6. Conclusion (paragraph)

Add professional formatting:
- Use Heading styles
- Bold important metrics
- Format currency values
- Add page breaks between major sections"""
            }
        ],
        betas=[
            "code-execution-2025-08-25",
            "skills-2025-10-02"
        ]
    )

    print("\nWord Document Created:")
    print("-" * 60)

    # Print response content
    for block in response.content:
        if hasattr(block, 'text'):
            print(block.text)
        elif hasattr(block, 'type'):
            print(f"[{block.type}]")

    return response


def create_meeting_minutes():
    """
    Create a meeting minutes document.
    """

    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        print("Error: ANTHROPIC_API_KEY environment variable not set")
        sys.exit(1)

    client = anthropic.Anthropic(api_key=api_key)

    print("\nCreating meeting minutes document...")
    print("-" * 60)

    response = client.messages.create(
        model="claude-4-5-sonnet",
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
                "content": """Create a Word document for meeting minutes:

Header Information:
- Date: December 13, 2024
- Time: 2:00 PM - 3:30 PM
- Location: Conference Room B
- Attendees: John Smith, Sarah Johnson, Mike Chen, Lisa Anderson

Content:
1. Welcome & Opening Remarks (paragraph)

2. Agenda Items:
   Item 1: Project Status Update (Sarah Johnson)
   - Timeline on track
   - Budget 95% allocated
   - Next milestone: January 15, 2025

   Item 2: Q1 Planning (John Smith)
   - Three strategic initiatives
   - Resource allocation discussion
   - Timeline development

   Item 3: Team Building Event (Lisa Anderson)
   - Proposed dates: January 20-22
   - Budget request: $5,000
   - Off-site location selected

3. Action Items:
   - Sarah: Finalize project documentation (Due: Dec 20)
   - Mike: Schedule resource planning meeting (Due: Dec 14)
   - John: Prepare Q1 budget proposal (Due: Dec 18)

4. Next Meeting:
   - Date: January 10, 2025
   - Venue: TBD

Format with:
- Professional heading styles
- Organized sections
- Clear action item table
- Signature line at bottom"""
            }
        ],
        betas=[
            "code-execution-2025-08-25",
            "skills-2025-10-02"
        ]
    )

    print("\nMeeting Minutes Created:")
    for block in response.content:
        if hasattr(block, 'text'):
            # Truncate long output
            text = block.text
            if len(text) > 500:
                print(text[:500] + "...")
            else:
                print(text)
        elif hasattr(block, 'type'):
            print(f"[{block.type}]")

    return response


if __name__ == "__main__":
    try:
        # Create main report
        create_word_document()

        # Create meeting minutes
        create_meeting_minutes()

    except anthropic.APIError as e:
        print(f"API Error: {e}")
        sys.exit(1)
    except Exception as e:
        print(f"Error: {e}")
        sys.exit(1)
