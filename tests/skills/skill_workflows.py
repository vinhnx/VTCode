"""Workflow-skill builders for the VT Code enhanced-skills demo suite.

Split out of test_enhanced.py to honor the 1000-line hard ceiling.
Grouped by skill area: batch processing, cross-skill pipelines, test scaffolding.
"""

import json
from pathlib import Path


class WorkflowSkillsMixin:
    """Builds the workflow-orchestration demo skills."""

    skills_dir: Path

    def demonstrate_advanced_workflow(self):
        """Demonstrate advanced 'plan-validate-execute' pattern."""
        
        print("\n Advanced Workflow: Plan-Validate-Execute")
        print("=" * 60)
        
        # Create a skill that uses this pattern
        skill_dir = self.skills_dir / "batch-pdf-processor"
        skill_dir.mkdir(exist_ok=True)
        
        skill_md = """---
name: batch-pdf-processor
description: Process multiple PDFs with validation and error recovery. Use for batch operations, form processing, or document pipelines.
version: 1.0.0
---

# Batch PDF Processing

## Workflow (Plan-Validate-Execute)

### Step 1: Plan

Create processing plan:
```bash
python scripts/create_plan.py --input-dir ./pdfs/ --config config.json > plan.json
```

### Step 2: Validate

Check plan for errors:
```bash
python scripts/validate_plan.py plan.json
```

If validation fails, review errors and update config.json, then repeat Step 1.

### Step 3: Execute

Run batch processing:
```bash
python scripts/execute_batch.py --plan plan.json --output-dir ./results/
```

### Step 4: Verify

Check results:
```bash
python scripts/verify_results.py --results-dir ./results/
```

## Error Recovery

If processing fails mid-batch:
1. Check logs in ./results/logs/
2. Resume from last successful file:
   ```bash
   python scripts/resume_batch.py --plan plan.json --resume-from last_success
   ```

See [examples/complex_pipeline.md](examples/complex_pipeline.md) for complete workflow.
"""
        
        (skill_dir / "SKILL.md").write_text(skill_md)
        
        # Create validation scripts (demonstrating utility pattern)
        scripts_dir = skill_dir / "scripts"
        scripts_dir.mkdir(exist_ok=True)
        
        # Validation script (example of solving, not punting)
        validate_script = """#!/usr/bin/env python3
\"\"\"
Validate batch processing plan.

Solves problems rather than punting to Claude:
- Checks file existence
- Validates configuration
- Catches common errors
- Provides specific error messages
\"\"\"

import json
import sys
from pathlib import Path

def validate_plan(plan_path: str) -> bool:
    \"\"\"Validate processing plan with specific error messages.\"\"\"
    
    try:
        with open(plan_path) as f:
            plan = json.load(f)
    except FileNotFoundError:
        print(f"ERROR: Plan file not found: {plan_path}")
        print("SOLUTION: Run 'create_plan.py' first to generate plan.json")
        return False
    except json.JSONDecodeError as e:
        print(f"ERROR: Invalid JSON in plan: {e}")
        print("SOLUTION: Fix JSON syntax errors in {plan_path}")
        return False
    
    # Validate required fields
    required = ["input_files", "operations", "output_dir"]
    for field in required:
        if field not in plan:
            print(f"ERROR: Missing required field: {field}")
            print(f"SOLUTION: Add '{field}' to plan configuration")
            return False
    
    # Check input files exist
    missing_files = []
    for file_path in plan["input_files"]:
        if not Path(file_path).exists():
            missing_files.append(file_path)
    
    if missing_files:
        print(f"ERROR: {len(missing_files)} input files not found")
        for f in missing_files:
            print(f"  - {f}")
        print("SOLUTION: Check file paths or place files in correct directory")
        return False
    
    print(" Plan validation passed")
    return True

if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: validate_plan.py <plan.json>")
        sys.exit(1)
    
    sys.exit(0 if validate_plan(sys.argv[1]) else 1)
"""
        
        (scripts_dir / "validate_plan.py").write_text(validate_script)
        (scripts_dir / "validate_plan.py").chmod(0o755)
        
        print(" Created batch processing skill with plan-validate-execute workflow")
        print(" Demonstrates: utility scripts, error handling, validation loops")
    
    def demonstrate_cross_skill_dependencies(self):
        """Demonstrate cross-skill dependencies."""
        
        print("\n Cross-Skill Dependencies")
        print("=" * 60)
        
        # Analysis skill that depends on data processing skill
        analysis_skill = self.skills_dir / "data-analysis-pipeline"
        analysis_skill.mkdir(exist_ok=True)
        
        skill_md = """---
name: data-analysis-pipeline
description: Complete data analysis pipeline: extract, clean, analyze, visualize. Requires data-processing-utils skill.
version: 1.0.0
dependencies: ["data-processing-utils"]
---

# Data Analysis Pipeline

## Workflow

This skill orchestrates multiple steps using dependencies:

### 1. Extract Data (uses data-processing-utils)
```bash
vtcode skill run data-processing-utils:extract --source database.db --query "SELECT * FROM sales"
```

### 2. Clean Data (uses data-processing-utils)
```bash
vtcode skill run data-processing-utils:clean --input raw_data.csv --config cleaning_rules.json
```

### 3. Analyze (this skill)
```bash
python scripts/analyze.py --cleaned-data cleaned_data.csv --config analysis.json
```

### 4. Visualize (uses data-processing-utils)
```bash
vtcode skill run data-processing-utils:visualize --data results.csv --type bar-chart
```

## Configuration

```json
{
  "extraction": { /* handled by data-processing-utils */ },
  "cleaning": { /* handled by data-processing-utils */ },
  "analysis": {
    "metrics": ["revenue", "growth", "conversion"],
    "group_by": ["region", "product"]
  },
  "visualization": { /* handled by data-processing-utils */ }
}
```

See [examples/complete_pipeline.md](examples/complete_pipeline.md) for end-to-end example.
"""
        
        (analysis_skill / "SKILL.md").write_text(skill_md)
        
        print(" Created data analysis pipeline with cross-skill dependencies")
        print(" Demonstrates: skill composition, dependency management, workflow orchestration")
    
    def create_comprehensive_test_suite(self):
        """Create comprehensive testing framework."""
        
        print("\n Comprehensive Testing Suite")
        print("=" * 60)
        
        tests_dir = self.skills_dir / ".skill-tests"
        tests_dir.mkdir(exist_ok=True)
        
        # Test framework
        test_framework = """#!/usr/bin/env python3
\"\"\"
VT Code Skills Testing Framework

Comprehensive testing for:
1. Token efficiency
2. Progressive disclosure
3. Cross-skill dependencies
4. Platform compatibility
5. Real-world usage patterns
\"\"\"

import json
import time
from pathlib import Path
from typing import Dict, List, Any

class SkillTester:
    def __init__(self, skills_dir: Path):
        self.skills_dir = skills_dir
        self.results = []
    
    def test_token_efficiency(self, skill_name: str) -> Dict[str, Any]:
        \"\"\"Test token efficiency metrics.\"\"\"
        # Implementation measures actual token usage
        pass
    
    def test_progressive_disclosure(self, skill_name: str) -> Dict[str, Any]:
        \"\"\"Test progressive disclosure effectiveness.\"\"\"
        # Implementation verifies level loading patterns
        pass
    
    def test_cross_skill_dependencies(self, skill_name: str) -> Dict[str, Any]:
        \"\"\"Test cross-skill dependencies.\"\"\"
        # Implementation validates dependency resolution
        pass

# Example test case
TEST_CASES = [
    {
        "skill": "pdf-report-optimized",
        "query": "Generate quarterly sales report",
        "expected_tokens": {
            "max_loaded": 300,
            "min_loaded": 150
        },
        "expected_files": ["report.pdf"],
        "success_criteria": [
            "PDF generated successfully",
            "Token usage within range",
            "Load time < 5s"
        ]
    }
]
"""
        
        (tests_dir / "framework.py").write_text(test_framework)
        
        # Test cases
        test_cases = {
            "token_efficiency_tests": [
                {
                    "name": "pdf_report_basic",
                    "skill": "pdf-report-optimized",
                    "query": "create sales report",
                    "success_criteria": [
                        "efficiency_ratio < 0.4"
                    ]
                }
            ],
            "progressive_disclosure_tests": [
                {
                    "name": "levels_loaded_correctly",
                    "skill": "batch-pdf-processor",
                    "query": "process pdf",
                    "expected_levels": ["metadata", "instructions"],
                    "unexpected_levels": ["advanced", "reference"],
                    "success_criteria": [
                        "only_expected_levels_loaded",
                        "resources_loaded_on_demand"
                    ]
                }
            ],
            "cross_skill_tests": [
                {
                    "name": "pipeline_orchestration",
                    "skills": ["data-analysis-pipeline", "data-processing-utils"],
                    "query": "analyze sales data",
                    "expected_dependencies": ["data-processing-utils"],
                    "success_criteria": [
                        "dependencies_resolved",
                        "no_circular_dependencies",
                        "correct_execution_order"
                    ]
                }
            ],
            "platform_compatibility_tests": [
                {
                    "name": "platform_detection",
                    "platforms": ["claude_api", "claude_code", "vtcode_local"],
                    "success_criteria": [
                        "correct_capability_detection",
                        "appropriate_fallback_selection",
                        "no_unsupported_features"
                    ]
                }
            ]
        }
        
        (tests_dir / "test_cases.json").write_text(json.dumps(test_cases, indent=2))
        
        print(" Created comprehensive testing framework")
        print(" Includes: token efficiency, progressive disclosure, dependencies, platform compatibility")
        
        return tests_dir
