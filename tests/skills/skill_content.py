"""Content-skill builders for the VT Code enhanced-skills demo suite.

Split out of test_enhanced.py to honor the 1000-line hard ceiling.
Grouped by skill area: PDF reporting, Excel analysis, token-optimization demo.
"""

from pathlib import Path


class ContentSkillsMixin:
    """Builds the content-creation demo skills (PDF, Excel, optimization demo)."""

    skills_dir: Path

    def create_optimized_pdf_skill(self):
        """Create optimized PDF skill following best practices."""
        
        skill_dir = self.skills_dir / "pdf-report-optimized"
        skill_dir.mkdir(exist_ok=True)
        
        # Token-optimized SKILL.md (target: 30-40 lines vs 60+)
        skill_md = """---
name: pdf-report-optimized
description: Generate PDF reports with charts and business formatting. Use for financial reports, presentations, invoices. Use when user mentions PDF, report, or document generation.
version: 2.0.0
author: VT Code Team
category: content_creation
tags: [pdf, reports, business, charts]
estimated_tokens: 240
critical_tokens: 120
complexity: moderate
---

# PDF Report Generation

## Quick Start

Use one of these methods based on available libraries:

```bash
# Method 1: Container Skills (requires API key)
claude_api_skills --skill pdf --spec report.json

# Method 2: Local FPDF (pip install fpdf2)
python -m vtcode.skills.pdf.local_fpdf --spec report.json

# Method 3: ReportLab (pip install reportlab)
python -m vtcode.skills.pdf.reportlab --spec report.json

# Method 4: System Pandoc
python -m vtcode.skills.pdf.pandoc --spec report.json

# Method 5: Mock (always works)
python -m vtcode.skills.pdf.mock --spec report.json
```

## Specification Format

```json
{
  "title": "Report Title",
  "filename": "output_name",
  "sections": {
    "Section Name": {"key": "value"}
  }
}
```

## Advanced Features

See [advanced/README.md](advanced/) for:
- Custom styling and branding
- Complex charts and visualizations
- Multi-page document layouts
- Conditional content generation

## Examples

See [examples/basic_report.md](examples/basic_report.md) for complete examples.

## Troubleshooting

If PDF generation fails:
1. Check library availability with `--check-env`
2. Use verbose mode: `--verbose`
3. Try mock generation first: `--method mock"
"""
        
        (skill_dir / "SKILL.md").write_text(skill_md)
        
        # Create optimized directory structure
        (skill_dir / "examples").mkdir(exist_ok=True)
        (skill_dir / "advanced").mkdir(exist_ok=True)
        (skill_dir / "reference").mkdir(exist_ok=True)
        
        # Minimal essential files only
        self.create_concise_examples(skill_dir / "examples")
        self.create_advanced_guide(skill_dir / "advanced")
        
        print(f" Created optimized PDF skill (240 tokens vs 500+ previously)")
    
    def create_concise_examples(self, examples_dir: Path):
        """Create concise examples following best practices."""
        
        # Single comprehensive example instead of multiple small ones
        example_content = """# Basic Report Example

## Specification

```json
{
  "title": "Q4 Sales Report",
  "filename": "q4_sales",
  "sections": {
    "Executive Summary": {
      "Revenue": "$2.5M",
      "Growth": "+18%",
      "Key Insight": "Enterprise segment driving growth"
    },
    "Regional Performance": {
      "North America": "$1.2M (48%)",
      "Europe": "$800K (32%)",
      "APAC": "$500K (20%)"
    }
  }
}
```

## Expected Output

Professional PDF with:
- Title page with company branding
- Executive summary section
- Regional performance table
- Growth chart visualization
- Page numbers and footer

## Command

```bash
python -m vtcode.skills.pdf.generate --spec q4_sales.json --output ./reports/
```

## Customization

Modify the specification to include:
- Additional sections
- Custom branding
- More complex charts
- Multiple pages
"""
        
        (examples_dir / "basic_report.md").write_text(example_content)
    
    def create_advanced_guide(self, advanced_dir: Path):
        """Create advanced guide with progressive disclosure."""
        
        # Reference file - loaded only when needed
        (advanced_dir / "README.md").write_text("""# Advanced PDF Features

## Custom Styling

```json
{
  "title": "Report",
  "style": {
    "primary_color": "#003366",
    "font_family": "Arial",
    "header_size": 16
  }
}
```

See [styling_guide.md](styling_guide.md) for complete options.

## Complex Charts

Use matplotlib for advanced visualizations:

```python
import matplotlib.pyplot as plt

# Chart configuration in spec
"chart_config": {
  "type": "bar",
  "data": [10, 20, 30],
  "colors": ["#003366", "#0066CC", "#0099FF"]
}
```
""")
    
    def create_optimized_excel_skill(self):
        """Create optimized Excel skill with token efficiency."""
        
        skill_dir = self.skills_dir / "excel-analysis-optimized"
        skill_dir.mkdir(exist_ok=True)
        
        # Ultra-concise SKILL.md (target: 25-30 lines)
        skill_md = """---
name: excel-analysis-optimized
description: Analyze Excel spreadsheets, create pivot tables, generate charts. Use for sales data, financial reports, data analysis. Use when user mentions Excel, spreadsheet, or .xlsx files.
version: 2.0.0
author: VT Code Team
category: data_analysis
tags: [excel, spreadsheets, data, charts]
estimated_tokens: 180
critical_tokens: 90
complexity: moderate
---

# Excel Analysis

## Quick Start

```bash
# Create analysis
python -m vtcode.skills.excel.analyze --file data.xlsx --config analysis.json

# Generate chart
python -m vtcode.skills.excel.chart --file data.xlsx --type bar --output chart.png

# Create pivot table
python -m vtcode.skills.excel.pivot --file data.xlsx --config pivot.json
```

## Configuration

```json
{
  "file": "data.xlsx",
  "sheet": "Sheet1",
  "analysis": {
    "type": "pivot",
    "rows": ["Region"],
    "columns": ["Product"],
    "values": ["Sales"]
  }
}
```

See [examples/](examples/) for common analysis patterns.
"""
        
        (skill_dir / "SKILL.md").write_text(skill_md)
        print(f" Created optimized Excel skill (180 tokens vs 350+ previously)")
    
    def create_optimization_demo_skill(self):
        """Create a skill that demonstrates token optimization."""
        
        skill_dir = self.skills_dir / "token-optimization-demo"
        skill_dir.mkdir(exist_ok=True)
        
        # Create two versions to demonstrate optimization
        
        # Version 1: Unoptimized (verbose, like old VT Code skills)
        unoptimized = """---
name: token-demo-unoptimized
description: This is a demonstration skill that shows what NOT to do. This skill is designed to help you understand why conciseness matters and how to optimize your skills for token efficiency. Use this skill to learn about best practices for skill authoring and token optimization. This skill should only be used for educational purposes to demonstrate the difference between optimized and unoptimized skill patterns.
version: 1.0.0
---

# Token Optimization Demonstration (UNOPTIMIZED - Do Not Use)

## Introduction

This skill demonstrates poor authoring practices. You should NOT use this as a template for your own skills. Instead, use this to understand what NOT to do.

### Why This Is Bad

This skill is WAY too verbose. Let me explain why that's a problem:

1. **Token Waste**: Every word in this skill consumes tokens in Claude's context window. The context window is like a public good - it's shared with the system prompt, conversation history, other skills, and your actual request. So when you make a skill verbose, you're taking up space that could be used for more important things.

2. **Clutter**: When Claude loads this skill (which happens when you ask for something that matches the description), all of this text gets added to the context. That makes it harder for Claude to find the ACTUALLY important information.

3. **Redundancy**: Claude is already very smart! It doesn't need you to explain basic concepts. For example, you don't need to explain what a PDF is, or what tokenization is, or why context windows matter. Claude already knows all of that.

4. **Poor Structure**: This skill doesn't use progressive disclosure effectively. It puts everything in one big file instead of splitting it into modular pieces that can be loaded as needed.

### What You Should Do Instead

Instead of writing verbose skills like this one, you should:

1. Be concise: Get to the point quickly
2. Assume Claude is smart: Don't explain obvious things
3. Use progressive disclosure: Split into multiple files
4. Focus on what's unique: Don't document general knowledge
5. Test and iterate: See what actually gets used

### More Explanation

Let me elaborate on each of these points in more detail...

[Continues for 200+ more lines]
"""
        
        # Version 2: Optimized (concise, following best practices)
        optimized = """---
name: token-demo-optimized
description: Learn token optimization by comparing this skill to the unoptimized version. Use when learning about skill authoring best practices.
version: 2.0.0
---

# Token Optimization Demonstration

## Overview

This skill demonstrates token optimization best practices. Compare with `token-demo-unoptimized` to see the difference.

## Best Practices

1. **Be concise**: Get to the point quickly
2. **Assume Claude is smart**: Don't explain obvious concepts
3. **Use progressive disclosure**: Split content across files
4. **Focus on unique knowledge**: Don't document general information
5. **Test and iterate**: Measure what actually gets used

## Quick Example

**Unoptimized** (150 tokens):
```
This skill helps you create PDF documents. PDF stands for Portable Document Format...
```

**Optimized** (30 tokens):
```
Create PDFs with charts and tables. Use for reports and invoices.
```

## Resources

- [optimization_guide.md](optimization_guide.md) - Detailed optimization techniques
- [examples/](examples/) - Before/after comparisons
- [metrics/](metrics/) - Token usage measurements

**Token savings: 85% (30 vs 200+ tokens)**
"""
        
        (skill_dir / "unoptimized_version.md").write_text(unoptimized)
        (skill_dir / "optimized_version.md").write_text(optimized)
        
        # Create detailed optimization guide (loaded only when needed)
        guide = """# Token Optimization Guide

## Why Conciseness Matters

**Context window is shared**: Skills compete with conversation history, other skills, and system prompt for limited space.

**Loading is on-demand**: Once loaded, every token competes for context.

**Claude is already smart**: Assumes knowledge of general concepts.

## Optimization Techniques

### 1. Remove Redundancy

**Before** (40 tokens):
```
PDF (Portable Document Format) is a file format created by Adobe in 1993...
```

**After** (8 tokens):
```
Generate PDF documents.
```

### 2. Use Progressive Disclosure

**Before** (single 300-line file):
- Everything loaded at once
- Most content not needed
- Wasted tokens

**After** (structured directory):
```
SKILL.md          # 30 tokens (always loaded)
examples/         # 150 tokens (loaded when needed)
advanced/         # 200 tokens (rarely loaded)
```

### 3. Focus on Unique Knowledge

**Don't document**: General programming, common libraries, basic concepts
**Do document**: Your specific workflows, business logic, unique requirements

## Measurement

Track these metrics:
- Metadata tokens (always loaded)
- Instruction tokens (loaded on demand)
- Resource tokens (loaded as needed)
- Efficiency ratio: loaded / available

## Real Example

This demonstration skill:
- Unoptimized: 850+ tokens
- Optimized: 125 tokens
- **Savings: 85%**
"""
        
        (skill_dir / "optimization_guide.md").write_text(guide)
        
        print(f" Created token optimization demo (85% token savings)")
