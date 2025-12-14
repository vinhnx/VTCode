#!/usr/bin/env python3
"""
FastAPI application demonstrating enhanced VT Code skills

This creates a simple FastAPI app that uses the enhanced skills
to generate a PDF report via multiple methods (container, local, mock).
"""

from fastapi import FastAPI, HTTPException
from pydantic import BaseModel
from typing import Dict, List, Any, Optional
import json
from pathlib import Path

app = FastAPI(
    title="VT Code Enhanced Skills Demo",
    description="Demonstrates token efficiency and multi-method PDF generation"
)

class ReportSpec(BaseModel):
    title: str
    sections: Dict[str, Any]
    method: Optional[str] = "auto"

@app.get("/")
async def root():
    return {
        "message": "VT Code Enhanced Skills Demo",
        "endpoints": {
            "generate_pdf": "/generate-pdf",
            "token_efficiency": "/token-efficiency",
            "skills_info": "/skills-info"
        }
    }

@app.post("/generate-pdf")
async def generate_pdf(report: ReportSpec):
    """Generate PDF report using enhanced skills."""
    
    from vtcode_skills_production import ProductionSkillDemo
    
    demo = ProductionSkillDemo()
    
    # Try container skills first, fallback to local
    if report.method == "container":
        result = demo.try_anthropic_container(report.dict())
    elif report.method == "local":
        result = demo.try_local_implementation(report.dict())
    else:
        # Auto-select best available method
        result = demo.generate_with_fallbacks(report.dict())
    
    return {
        "success": result.get("success", False),
        "method_used": result.get("method_used", "unknown"),
        "file": result.get("file"),
        "metrics": result.get("metrics", {})
    }

@app.get("/token-efficiency")
async def token_efficiency():
    """Calculate and return token efficiency metrics."""
    
    # Example comparing unoptimized vs optimized
    unoptimized = {
        "title": "Monthly Sales Report",
        "description": "This is a comprehensive monthly sales report that includes all relevant data and analysis for the entire sales team across all regions. It contains detailed breakdowns, charts, tables, and executive summaries. Use this report for quarterly business reviews and strategic planning sessions.",
        "sections": {
            "Executive Summary": "This section provides a high-level overview...",
            "Regional Performance": "Detailed breakdown by region...",
            "Product Mix": "Analysis of product categories..."
        }
    }
    
    optimized = {
        "title": "Sales Report",
        "description": "Monthly sales analysis with regional breakdowns.",
        "sections": {
            "Summary": "Revenue: $125K, Growth: +15%",
            "Regions": "North: $45K, South: $32K, East: $28K, West: $20K",
            "Top Products": "A: $35K, B: $28K, C: $22K"
        }
    }
    
    unopt_tokens = len(str(unoptimized)) // 4
    opt_tokens = len(str(optimized)) // 4
    savings = (unopt_tokens - opt_tokens) / unopt_tokens
    
    return {
        "unoptimized_tokens": unopt_tokens,
        "optimized_tokens": opt_tokens,
        "savings_percent": round(savings * 100, 1),
        "improvement": f"{savings * 100:.1f}% token reduction"
    }

@app.get("/skills-info")
async def skills_info():
    """Get information about available skills."""
    
    from vtcode_skills_production import VTCodeEnhancedSkills
    
    skills = VTCodeEnhancedSkills()
    available_skills = skills.discover_skills()
    
    return {
        "skills_count": len(available_skills),
        "skills": [
            {
                "name": skill["name"],
                "description": skill["description"],
                "token_efficiency": skill.get("token_efficiency", "unknown")
            }
            for skill in available_skills
        ]
    }

@app.get("/cross-skill-demo")
async def cross_skill_demo():
    """Demonstrate cross-skill dependencies."""
    
    return {
        "workflow": [
            "1. data-processing-utils:extract -> raw data",
            "2. data-processing-utils:clean -> cleaned data",
            "3. data-analysis-pipeline:analyze -> results",
            "4. pdf-report-optimized:generate -> final PDF"
        ],
        "benefits": [
            "Reusability: Utility skills used by multiple pipelines",
            "Modularity: Each skill has clear responsibility",
            "Maintainability: Changes benefit all dependents",
            "Composability: Complex workflows from simple parts"
        ]
    }

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=8000)