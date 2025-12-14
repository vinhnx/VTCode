#!/usr/bin/env python3
"""
Claude API Agent Skills Tutorial Implementation

This script follows the official Claude API tutorial exactly:
https://docs.anthropic.com/en/agents-and-tools/agent-skills/quickstart

It demonstrates the complete workflow:
1. List available Skills
2. Create documents with Skills (PowerPoint, Excel, Word, PDF)
3. Download generated files using Files API
4. Handle the 3-level progressive disclosure architecture
"""

import os
import json
from pathlib import Path
from typing import Dict, List, Optional, Any
import requests
from datetime import datetime

class ClaudeAPISkillsTutorial:
    """Complete implementation of the official Claude API Skills tutorial."""
    
    def __init__(self):
        self.api_key = os.environ.get("ANTHROPIC_API_KEY")
        if not self.api_key:
            raise ValueError("ANTHROPIC_API_KEY environment variable not set")
        
        # API endpoints
        self.base_url = "https://api.anthropic.com/v1"
        self.headers = {
            "x-api-key": self.api_key,
            "anthropic-version": "2023-06-01",
            "content-type": "application/json"
        }
        
        print(" Claude API Agent Skills Tutorial")
        print("=" * 60)
        print("Following official tutorial: https://docs.anthropic.com/en/agents-and-tools/agent-skills/quickstart")
        print()
    
    def list_available_skills(self) -> List[Dict[str, Any]]:
        """
        Step 1: List available Skills (from tutorial)
        
        This demonstrates the first level of progressive disclosure:
        - Metadata only (~50 tokens per skill)
        - Loaded at startup into system prompt
        - Claude discovers Skills without loading full instructions
        """
        
        print(" Step 1: Listing Available Skills")
        print("-" * 40)
        
        # Add required beta header for Skills API
        headers = self.headers.copy()
        headers["anthropic-beta"] = "skills-2025-10-02"
        
        # List Anthropic-managed Skills
        url = f"{self.base_url}/skills"
        params = {"source": "anthropic"}
        
        response = requests.get(url, headers=headers, params=params)
        response.raise_for_status()
        
        skills_data = response.json()
        skills = skills_data.get('data', [])
        
        print("Available Anthropic Skills:")
        for skill in skills:
            print(f"  • {skill['id']}: {skill['display_title']}")
        
        print(f"\n Found {len(skills)} Anthropic-managed skills")
        return skills
    
    def create_presentation(self, topic: str = "renewable energy", num_slides: int = 5) -> Dict[str, Any]:
        """
        Step 2: Create a presentation with PowerPoint Skill
        
        This demonstrates the second level of progressive disclosure:
        - Claude matches task to relevant Skill (PowerPoint)
        - Loads full instructions from SKILL.md
        - Executes Skill's code to create presentation
        """
        
        print(f"\n Step 2: Creating PowerPoint Presentation")
        print(f"Topic: {topic}")
        print(f"Slides: {num_slides}")
        print("-" * 40)
        
        # Prepare the API request following tutorial exactly
        url = f"{self.base_url}/messages"
        
        # Required beta headers for Skills
        headers = self.headers.copy()
        headers["anthropic-beta"] = "code-execution-2025-08-25,skills-2025-10-02"
        
        # Request payload following tutorial format
        payload = {
            "model": "claude-sonnet-4-5-20250929",  # Note: tutorial uses this model
            "max_tokens": 4096,
            "container": {
                "skills": [
                    {
                        "type": "anthropic",  # Anthropic-managed Skill
                        "skill_id": "pptx",   # PowerPoint Skill
                        "version": "latest"   # Most recent version
                    }
                ]
            },
            "messages": [{
                "role": "user",
                "content": f"Create a presentation about {topic} with {num_slides} slides"
            }],
            "tools": [{
                "type": "code_execution_20250825",  # Required for Skills
                "name": "code_execution"
            }]
        }
        
        print("Making API request with PowerPoint Skill...")
        response = requests.post(url, headers=headers, json=payload)
        response.raise_for_status()
        
        result = response.json()
        print(" API request successful")
        
        # Extract file information from response
        file_info = self.extract_file_info(result)
        
        if file_info:
            print(f" Presentation created with file ID: {file_info['file_id']}")
            print(f" Filename: {file_info.get('filename', 'unknown')}")
        else:
            print("  No file information found in response")
        
        return {
            'response': result,
            'file_info': file_info,
            'skill_used': 'pptx',
            'topic': topic,
            'num_slides': num_slides
        }
    
    def extract_file_info(self, response: Dict[str, Any]) -> Optional[Dict[str, Any]]:
        """Extract file ID and metadata from API response."""
        
        # Navigate the response structure to find file information
        for block in response.get('content', []):
            if block.get('type') == 'tool_use' and block.get('name') == 'code_execution':
                # Look in tool result content
                for result_block in block.get('content', []):
                    if result_block.get('type') == 'bash_code_execution_result':
                        # Search for file references in result content
                        for content_item in result_block.get('content', []):
                            if content_item.get('type') == 'file':
                                return {
                                    'file_id': content_item.get('file_id'),
                                    'filename': content_item.get('filename', 'generated_file'),
                                    'size': content_item.get('size_bytes', 0)
                                }
        
        return None
    
    def download_created_file(self, file_info: Dict[str, Any], output_filename: str) -> str:
        """
        Step 3: Download the created file using Files API
        
        This demonstrates accessing generated files from the code execution container.
        """
        
        print(f"\n Step 3: Downloading Created File")
        print(f"File ID: {file_info['file_id']}")
        print(f"Output: {output_filename}")
        print("-" * 40)
        
        if not file_info.get('file_id'):
            raise ValueError("No file ID provided for download")
        
        # Files API requires files-api-2025-04-14 beta header
        headers = self.headers.copy()
        headers["anthropic-beta"] = "files-api-2025-04-14"
        
        # Download file content
        url = f"{self.base_url}/files/{file_info['file_id']}/content"
        
        print("Downloading file from Files API...")
        response = requests.get(url, headers=headers)
        response.raise_for_status()
        
        # Save to disk
        output_path = Path(output_filename)
        with open(output_path, 'wb') as f:
            f.write(response.content)
        
        file_size = output_path.stat().st_size
        print(f" File saved: {output_path} ({file_size} bytes)")
        
        return str(output_path)
    
    def create_spreadsheet(self, title: str = "quarterly sales tracking") -> Dict[str, Any]:
        """Create a spreadsheet with Excel Skill (additional example)."""
        
        print(f"\n Creating Excel Spreadsheet: {title}")
        print("-" * 40)
        
        url = f"{self.base_url}/messages"
        headers = self.headers.copy()
        headers["anthropic-beta"] = "code-execution-2025-08-25,skills-2025-10-02"
        
        payload = {
            "model": "claude-sonnet-4-5-20250929",
            "max_tokens": 4096,
            "container": {
                "skills": [
                    {
                        "type": "anthropic",
                        "skill_id": "xlsx",
                        "version": "latest"
                    }
                ]
            },
            "messages": [{
                "role": "user",
                "content": f"Create a {title} spreadsheet with sample data and charts"
            }],
            "tools": [{
                "type": "code_execution_20250825",
                "name": "code_execution"
            }]
        }
        
        response = requests.post(url, headers=headers, json=payload)
        response.raise_for_status()
        
        result = response.json()
        file_info = self.extract_file_info(result)
        
        if file_info:
            print(f" Spreadsheet created: {file_info.get('filename', 'spreadsheet.xlsx')}")
        
        return {
            'response': result,
            'file_info': file_info,
            'skill_used': 'xlsx',
            'title': title
        }
    
    def demonstrate_progressive_disclosure(self) -> None:
        """Demonstrate the 3-level progressive disclosure architecture."""
        
        print(f"\n Progressive Disclosure Architecture Demonstration")
        print("=" * 60)
        print("Level 1: Metadata Discovery (Always loaded at startup)")
        print("- Skill names and descriptions (~50 tokens per skill)")
        print("- Claude knows Skills exist without loading full instructions")
        print("- Example: 'PowerPoint Skill: Create and edit presentations'")
        print()
        
        print("Level 2: Instructions Loading (When Skill is triggered)")
        print("- Full SKILL.md content loaded when task matches Skill description")
        print("- Detailed workflows, best practices, and guidance")
        print("- Example: Complete PowerPoint creation instructions")
        print()
        
        print("Level 3: Resources Access (As needed during execution)")
        print("- Additional files like examples/, scripts/, templates/")
        print("- Loaded only when referenced in instructions")
        print("- Example: Sample presentation templates, utility scripts")
        print()
        
        print(" This architecture ensures efficient context usage")
        print(" Only relevant content occupies the context window")
        print(" Skills can include extensive resources without penalty")
    
    def run_complete_tutorial(self):
        """Run the complete tutorial workflow."""
        
        print(" Claude API Agent Skills - Complete Tutorial")
        print("=" * 60)
        print("Following official tutorial step-by-step")
        print()
        
        try:
            # Step 1: List available skills
            skills = self.list_available_skills()
            
            # Demonstrate progressive disclosure architecture
            self.demonstrate_progressive_disclosure()
            
            # Step 2: Create presentation (main tutorial example)
            presentation_result = self.create_presentation(
                topic="renewable energy", 
                num_slides=5
            )
            
            # Step 3: Download the created file
            if presentation_result['file_info']:
                downloaded_file = self.download_created_file(
                    presentation_result['file_info'],
                    "renewable_energy_tutorial.pptx"
                )
                
                print(f"\n Tutorial completed successfully!")
                print(f" Presentation saved as: {downloaded_file}")
            
            # Additional examples from tutorial
            print(f"\n Additional Examples from Tutorial:")
            
            # Create spreadsheet example
            spreadsheet_result = self.create_spreadsheet("quarterly_sales_tracking")
            if spreadsheet_result['file_info']:
                spreadsheet_file = self.download_created_file(
                    spreadsheet_result['file_info'],
                    "quarterly_sales_tutorial.xlsx"
                )
                print(f" Spreadsheet saved as: {spreadsheet_file}")
            
            print(f"\n All tutorial examples completed successfully!")
            print(f"\nNext steps:")
            print(f"  • Try creating Word documents with 'docx' skill")
            print(f"  • Generate PDFs with 'pdf' skill")
            print(f"  • Explore custom Skills creation")
            print(f"  • Check out the Agent Skills Cookbook")
            
        except Exception as e:
            print(f" Tutorial failed: {e}")
            raise

if __name__ == "__main__":
    try:
        tutorial = ClaudeAPISkillsTutorial()
        tutorial.run_complete_tutorial()
    except Exception as e:
        print(f"\n Tutorial Error: {e}")
        print("\nTroubleshooting:")
        print("  • Ensure ANTHROPIC_API_KEY is set")
        print("  • Check network connectivity to api.anthropic.com")
        print("  • Verify API key has proper permissions")
        print("  • Ensure you have the required beta access")
        sys.exit(1)