#!/usr/bin/env python3
"""
Enhanced Claude API Skills Demo for VT Code

This demonstrates proper integration with Claude API skills following the official guide:
https://platform.claude.com/docs/en/build-with-claude/skills-guide/

Key features:
- Compatible with Claude API container skills when available
- Fallback to local implementations when API unavailable
- Proper model names and beta headers
- Environment awareness and error handling
- File verification and user feedback
"""

import os
import sys
import json
import subprocess
from datetime import datetime
from typing import Dict, List, Any, Optional

class EnhancedClaudeSkills:
    """Enhanced skills implementation following Claude API patterns."""
    
    def __init__(self):
        self.api_key = os.environ.get("ANTHROPIC_API_KEY")
        self.client = None
        self.environment = {}
        self._check_environment()
    
    def _check_environment(self):
        """Comprehensive environment check following Claude guide best practices."""
        print(" Environment Check:")
        print("-" * 50)
        
        # API Key status
        self.environment['api_key_available'] = bool(self.api_key)
        print(f"ANTHROPIC_API_KEY: {' Set' if self.api_key else ' Not set'}")
        
        # Python version
        print(f"Python version: {sys.version.split()[0]}")
        
        # Check key libraries
        libraries = {
            'anthropic': 'For Claude API calls',
            'fpdf': 'For PDF generation',
            'reportlab': 'Alternative PDF library',
            'matplotlib': 'For charts and graphs',
            'pandas': 'For data processing',
            'requests': 'For HTTP calls'
        }
        
        for lib, description in libraries.items():
            try:
                __import__(lib)
                print(f" {lib}: Available ({description})")
                self.environment[lib] = True
            except ImportError:
                print(f" {lib}: Not available ({description})")
                self.environment[lib] = False
        
        # Check network connectivity
        self.environment['network_available'] = self._check_network()
        print(f"Network access: {' Available' if self.environment['network_available'] else ' Not available'}")
        
        print("-" * 50)
    
    def _check_network(self) -> bool:
        """Check if we can reach Anthropic API."""
        try:
            import requests
            response = requests.get("https://api.anthropic.com/v1/health", timeout=5)
            return response.status_code == 200
        except:
            return False
    
    def _initialize_client(self) -> bool:
        """Initialize Anthropic client if possible."""
        if not self.api_key:
            return False
        
        try:
            import anthropic
            self.client = anthropic.Anthropic(api_key=self.api_key)
            
            # Test API access
            response = self.client.messages.create(
                model="claude-3-5-sonnet-20241022",
                max_tokens=10,
                messages=[{"role": "user", "content": "Hi"}]
            )
            print(" Anthropic API client initialized successfully")
            return True
            
        except Exception as e:
            print(f" Failed to initialize Anthropic client: {e}")
            return False
    
    def generate_pdf_report(self, specification: Dict[str, Any]) -> Dict[str, Any]:
        """Generate PDF report using Claude API skills when available."""
        
        print(f"\n Generating PDF: {specification.get('title', 'Untitled Report')}")
        print("=" * 60)
        
        # Try methods in order of preference
        methods = [
            ("Anthropic Container Skills", self._try_anthropic_container),
            ("Local FPDF Library", self._try_fpdf_local),
            ("Local ReportLab", self._try_reportlab_local),
            ("Mock PDF (Text)", self._create_mock_pdf)
        ]
        
        for method_name, method_func in methods:
            print(f"\n Trying: {method_name}")
            try:
                result = method_func(specification)
                if result.get('success'):
                    print(f" SUCCESS: {method_name}")
                    return result
                else:
                    print(f"  Failed: {result.get('reason', 'Unknown reason')}")
            except Exception as e:
                print(f" Error in {method_name}: {e}")
            
            # Continue to next method
            continue
        
        return {'success': False, 'error': 'All methods failed'}
    
    def _try_anthropic_container(self, specification: Dict[str, Any]) -> Dict[str, Any]:
        """Try using real Anthropic container skills."""
        
        if not self.client:
            if not self._initialize_client():
                return {'success': False, 'reason': 'Cannot initialize Anthropic client'}
        
        print("Using Claude API container skills...")
        
        try:
            # Build specification following Claude guide patterns
            prompt = f"""Generate a professional PDF document with the following specifications:

Title: {specification.get('title', 'Document')}
Type: {specification.get('type', 'report')}

Content Sections:
{json.dumps(specification.get('sections', {}), indent=2)}

Requirements:
- Professional business formatting
- Clear section headers and typography
- Consistent styling and colors
- Proper page layout with margins
- Include page numbers and document metadata

Use the PDF skill to create a high-quality document suitable for business use."""
            
            # Use proper Claude API format from official guide
            response = self.client.beta.messages.create(
                model="claude-3-5-sonnet-20241022",  # Correct model name from guide
                max_tokens=4096,
                tools=[{"type": "code_execution", "name": "bash"}],  # Proper tool format
                messages=[{"role": "user", "content": prompt}],
                container={
                    "type": "skills",
                    "skills": [{"type": "anthropic", "skill_id": "pdf", "version": "latest"}]
                },
                betas=["code-execution-2025-08-25", "skills-2025-10-02"]  # Required beta headers
            )
            
            print(" Claude API request successful")
            
            # Extract file references (from Claude guide)
            file_ids = []
            for item in response.content:
                if hasattr(item, 'type') and item.type == 'file':
                    file_ids.append(item.file_id)
                elif hasattr(item, 'file_id'):
                    file_ids.append(item.file_id)
            
            if file_ids:
                print(f" Generated file IDs: {file_ids}")
                
                # Download files using Files API
                downloaded_files = self._download_generated_files(file_ids)
                
                return {
                    'success': True,
                    'method': 'anthropic_container',
                    'file_ids': file_ids,
                    'downloaded_files': downloaded_files,
                    'note': 'Files generated via Claude API container skills'
                }
            else:
                # Extract text content if no files
                text_content = []
                for item in response.content:
                    if hasattr(item, 'text'):
                        text_content.append(item.text)
                
                return {
                    'success': True,
                    'method': 'anthropic_container',
                    'content': '\n'.join(text_content),
                    'note': 'Text response from Claude API'
                }
                
        except Exception as e:
            return {'success': False, 'error': f'Anthropic API error: {e}'}
    
    def _download_generated_files(self, file_ids: List[str]) -> List[str]:
        """Download files using Claude Files API."""
        downloaded_files = []
        
        try:
            for file_id in file_ids:
                # Get file metadata
                file_info = self.client.beta.files.retrieve_metadata(
                    file_id=file_id,
                    betas=["files-api-2025-04-14"]
                )
                
                # Download file content
                file_content = self.client.beta.files.download(
                    file_id=file_id,
                    betas=["files-api-2025-04-14"]
                )
                
                # Save to workspace
                output_path = f"/tmp/{file_info.filename}"
                with open(output_path, 'wb') as f:
                    f.write(file_content.read())
                
                downloaded_files.append(output_path)
                print(f" Downloaded: {output_path} ({file_info.size_bytes} bytes)")
            
        except Exception as e:
            print(f"  File download error: {e}")
        
        return downloaded_files
    
    def _try_fpdf_local(self, specification: Dict[str, Any]) -> Dict[str, Any]:
        """Local PDF generation using FPDF library."""
        
        try:
            from fpdf import FPDF
            
            print("Using FPDF library for local PDF generation...")
            
            pdf = FPDF()
            pdf.add_page()
            
            # Title page
            pdf.set_font('Arial', 'B', 20)
            pdf.cell(0, 20, specification.get('title', 'Document'), 0, 1, 'C')
            
            pdf.set_font('Arial', 'I', 10)
            pdf.cell(0, 10, f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')}", 0, 1, 'C')
            pdf.ln(10)
            
            # Content sections
            sections = specification.get('sections', {})
            for section_name, content in sections.items():
                pdf.set_font('Arial', 'B', 14)
                pdf.cell(0, 10, section_name, 0, 1)
                
                pdf.set_font('Arial', '', 11)
                if isinstance(content, dict):
                    for key, value in content.items():
                        pdf.cell(0, 8, f"• {key}: {value}", 0, 1)
                else:
                    pdf.multi_cell(0, 6, str(content))
                
                pdf.ln(5)
            
            # Save to workspace
            output_path = f"/tmp/{specification.get('filename', 'document')}.pdf"
            pdf.output(output_path)
            
            file_size = os.path.getsize(output_path)
            print(f" PDF generated: {output_path} ({file_size} bytes)")
            
            return {
                'success': True,
                'method': 'local_fpdf',
                'file': output_path,
                'size': file_size
            }
            
        except ImportError:
            return {'success': False, 'reason': 'FPDF library not available'}
        except Exception as e:
            return {'success': False, 'error': f'FPDF error: {e}'}
    
    def _try_reportlab_local(self, specification: Dict[str, Any]) -> Dict[str, Any]:
        """Local PDF generation using ReportLab."""
        
        try:
            from reportlab.lib.pagesizes import letter
            from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer
            from reportlab.lib.styles import getSampleStyleSheet
            
            print("Using ReportLab for local PDF generation...")
            
            output_path = f"/tmp/{specification.get('filename', 'document')}_reportlab.pdf"
            doc = SimpleDocTemplate(output_path, pagesize=letter)
            
            styles = getSampleStyleSheet()
            story = []
            
            # Title
            title = specification.get('title', 'Document')
            story.append(Paragraph(f"<b><font size=16>{title}</font></b>", styles['Title']))
            story.append(Spacer(1, 12))
            
            # Date
            story.append(Paragraph(f"<i>Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')}</i>", styles['Normal']))
            story.append(Spacer(1, 20))
            
            # Content sections
            sections = specification.get('sections', {})
            for section_name, content in sections.items():
                story.append(Paragraph(f"<b>{section_name}</b>", styles['Heading2']))
                
                if isinstance(content, dict):
                    for key, value in content.items():
                        story.append(Paragraph(f"• <b>{key}:</b> {value}", styles['Normal']))
                else:
                    story.append(Paragraph(str(content), styles['Normal']))
                
                story.append(Spacer(1, 12))
            
            doc.build(story)
            
            file_size = os.path.getsize(output_path)
            print(f" PDF generated: {output_path} ({file_size} bytes)")
            
            return {
                'success': True,
                'method': 'local_reportlab',
                'file': output_path,
                'size': file_size
            }
            
        except ImportError:
            return {'success': False, 'reason': 'ReportLab not available'}
        except Exception as e:
            return {'success': False, 'error': f'ReportLab error: {e}'}
    
    def _create_mock_pdf(self, specification: Dict[str, Any]) -> Dict[str, Any]:
        """Create structured text as PDF fallback."""
        
        print("Creating mock PDF representation...")
        
        content = []
        content.append("=" * 60)
        content.append(f"MOCK PDF DOCUMENT: {specification.get('title', 'Document')}")
        content.append("=" * 60)
        content.append(f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')}")
        content.append(f"Method: Mock PDF (no PDF libraries available)")
        content.append("")
        
        # Add sections
        sections = specification.get('sections', {})
        for section_name, section_content in sections.items():
            content.append(f"\n{section_name.upper()}")
            content.append("-" * 40)
            
            if isinstance(section_content, dict):
                for key, value in section_content.items():
                    content.append(f"• {key}: {value}")
            else:
                content.append(str(section_content))
        
        content.append("\n" + "=" * 60)
        content.append("Note: This is a mock PDF representation.")
        content.append("Install fpdf or reportlab for actual PDF generation:")
        content.append("  pip install fpdf2")
        content.append("  pip install reportlab")
        content.append("=" * 60)
        
        # Save as text file
        output_path = f"/tmp/{specification.get('filename', 'document')}.txt"
        with open(output_path, 'w', encoding='utf-8') as f:
            f.write('\n'.join(content))
        
        file_size = os.path.getsize(output_path)
        print(f" Mock PDF saved: {output_path} ({file_size} bytes)")
        
        return {
            'success': True,
            'method': 'mock_pdf',
            'file': output_path,
            'size': file_size,
            'note': 'Mock generation - no PDF libraries available'
        }

def main():
    """Main demonstration function."""
    
    print(" Enhanced Claude API Skills Demo for VT Code")
    print("=" * 60)
    print("This demo follows the official Claude API skills guide:")
    print("https://platform.claude.com/docs/en/build-with-claude/skills-guide/")
    print()
    
    # Initialize enhanced skills
    skills = EnhancedClaudeSkills()
    
    # Example 1: Monthly Sales Report
    sales_spec = {
        'title': 'Monthly Sales Report - December 2024',
        'type': 'financial_report',
        'filename': 'monthly_sales_dec2024',
        'sections': {
            'Executive Summary': {
                'Revenue Growth': '+15% vs November 2024',
                'Total Sales': '$125,000',
                'Units Sold': '1,250 units',
                'Key Insight': 'Strong performance in North region driven by Product A'
            },
            'Sales by Region': {
                'North Region': '$45,000 (36%)',
                'South Region': '$32,000 (26%)',
                'East Region': '$28,000 (22%)',
                'West Region': '$20,000 (16%)'
            },
            'Top Products': {
                'Product A': '$35,000 (28%)',
                'Product B': '$28,000 (22%)',
                'Product C': '$22,000 (18%)',
                'Product D': '$18,000 (14%)',
                'Product E': '$22,000 (18%)'
            },
            'Monthly Trends': {
                'October 2024': '$110,000',
                'November 2024': '$108,000',
                'December 2024': '$125,000',
                'Growth vs Nov': '+15.7%'
            },
            'Recommendations': 'Focus Q1 2025 marketing efforts on underperforming West region. Increase Product C inventory in North region due to high demand.'
        }
    }
    
    # Generate first report
    result1 = skills.generate_pdf_report(sales_spec)
    
    print("\n" + "=" * 60)
    
    # Example 2: Project Status Report
    project_spec = {
        'title': 'VT Code Skills Enhancement Project - Status Report',
        'type': 'project_report',
        'filename': 'vtcode_skills_status',
        'sections': {
            'Project Overview': {
                'Project Name': 'VT Code Skills Enhancement',
                'Status': 'In Progress',
                'Completion': '85%',
                'Start Date': '2024-12-14',
                'Expected Completion': '2024-12-20',
                'Project Manager': 'Development Team'
            },
            'Key Milestones': {
                'Environment Analysis': ' Complete',
                'Claude API Integration': ' Complete',
                'Local Fallback Implementation': ' Complete',
                'Documentation': ' In Progress',
                'Testing & Validation': '⏳ Pending',
                'Deployment': '⏳ Pending'
            },
            'Technical Achievements': {
                'Multi-Method PDF Generation': 'Implemented 4 different methods',
                'API Compatibility': 'Follows official Claude API patterns',
                'Error Handling': 'Comprehensive fallback strategies',
                'User Experience': 'Clear feedback and verification'
            },
            'Current Issues': {
                'High Priority': 'None identified',
                'Medium Priority': 'Performance optimization for large documents',
                'Low Priority': 'Additional styling options for PDF output'
            },
            'Next Steps': 'Complete documentation, conduct thorough testing, and prepare for production deployment.'
        }
    }
    
    # Generate second report
    result2 = skills.generate_pdf_report(project_spec)
    
    # Summary
    print("\n" + "=" * 60)
    print(" Generation Summary:")
    print(f"Report 1: {result1['method']} - Success: {result1['success']}")
    if result1.get('file'):
        print(f"   File: {result1['file']}")
    if result1.get('file_ids'):
        print(f"   File IDs: {result1['file_ids']}")
    
    print(f"Report 2: {result2['method']} - Success: {result2['success']}")
    if result2.get('file'):
        print(f"   File: {result2['file']}")
    if result2.get('file_ids'):
        print(f"   File IDs: {result2['file_ids']}")
    
    # List all generated files
    print("\n All Generated Files:")
    import glob
    files = glob.glob("/tmp/*_dec2024.*") + glob.glob("/tmp/*_status.*")
    for file in sorted(files):
        size = os.path.getsize(file)
        print(f"  • {file} ({size} bytes)")
    
    print("\n Enhanced Claude API skills demo completed!")
    print("\nKey improvements demonstrated:")
    print("  • Claude API container skills integration (when available)")
    print("  • Multiple fallback implementation methods")
    print("  • Proper model names and beta headers")
    print("  • Environment awareness and library checking")
    print("  • Comprehensive error handling")
    print("  • File verification and user feedback")
    print("  • Following official Claude API guide patterns")

if __name__ == "__main__":
    main()