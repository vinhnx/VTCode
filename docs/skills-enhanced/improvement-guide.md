# VT Code Skills Improvement Implementation Guide

##  Quick Start: Fix SKILL.md Conciseness

### **Step 1: Update SKILL.md Templates**

Replace the verbose template in `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/templates/traditional/SKILL.md.template`:

```markdown
---
name: {{skill_name}}
description: {{description}}
version: {{version}}
author: {{author}}
tags: utility,general
---

# {{skill_name}}

{{description}}

## Quick Start

```bash
# Most common usage pattern
echo "Replace with actual command"
```

## Common Patterns

- **Pattern 1**: Brief description + example
- **Pattern 2**: Brief description + example  
- **Pattern 3**: Brief description + example

## Dependencies

- List required tools/libraries
- Installation commands if needed

[See ADVANCED.md for complex scenarios]
```

### **Step 2: Create ADVANCED.md Template**

Create `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/templates/traditional/ADVANCED.md.template`:

```markdown
# {{skill_name}} - Advanced Usage

Detailed guidance for complex scenarios and advanced features.

## Advanced Patterns

### Complex Scenario 1
Detailed explanation with examples.

### Complex Scenario 2  
Detailed explanation with examples.

## Best Practices

- Detailed best practice 1
- Detailed best practice 2

## Troubleshooting

### Common Issues

**Issue 1**: Description
**Solution**: Step-by-step resolution

**Issue 2**: Description  
**Solution**: Step-by-step resolution

## Integration Examples

How to integrate with other tools and workflows.
```

##  Progressive Disclosure Implementation

### **Step 3: Update Context Manager**

Enhance `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/skills/context_manager.rs`:

```rust
impl ContextManager {
    /// Load skill with progressive disclosure
    pub async fn load_skill_progressive(&mut self, skill_name: &str, complexity_level: ComplexityLevel) -> Result<()> {
        match complexity_level {
            ComplexityLevel::Basic => {
                // Load only SKILL.md (max 30 lines)
                self.load_skill_basic(skill_name).await?;
            }
            ComplexityLevel::Advanced => {
                // Load SKILL.md + ADVANCED.md
                self.load_skill_basic(skill_name).await?;
                self.load_skill_advanced(skill_name).await?;
            }
            ComplexityLevel::Full => {
                // Load all resources
                self.load_skill_basic(skill_name).await?;
                self.load_skill_advanced(skill_name).await?;
                self.load_skill_resources(skill_name).await?;
            }
        }
        Ok(())
    }
    
    /// Load basic skill instructions (SKILL.md only)
    async fn load_skill_basic(&mut self, skill_name: &str) -> Result<()> {
        let skill_path = self.find_skill_path(skill_name)?;
        let skill_content = self.read_skill_md(&skill_path)?;
        
        // Truncate to first 30 lines for basic loading
        let basic_content = skill_content
            .lines()
            .take(30)
            .collect::<Vec<_>>()
            .join("\n");
        
        self.add_to_context(skill_name, &basic_content, ContextLevel::Basic)?;
        Ok(())
    }
    
    /// Load advanced instructions (ADVANCED.md)
    async fn load_skill_advanced(&mut self, skill_name: &str) -> Result<()> {
        let skill_path = self.find_skill_path(skill_name)?;
        let advanced_path = skill_path.join("ADVANCED.md");
        
        if advanced_path.exists() {
            let advanced_content = std::fs::read_to_string(&advanced_path)?;
            self.add_to_context(skill_name, &advanced_content, ContextLevel::Advanced)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ComplexityLevel {
    Basic,      // SKILL.md only (30 lines max)
    Advanced,   // SKILL.md + ADVANCED.md
    Full,       // All resources
}
```

### **Step 4: Add Token Efficiency Validator**

Create `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/skills/token_validator.rs`:

```rust
use anyhow::Result;
use std::collections::HashMap;

/// Validates skill instructions for token efficiency
pub struct TokenEfficiencyValidator {
    max_skill_md_lines: usize,
    redundancy_patterns: Vec<String>,
    verbosity_indicators: Vec<String>,
}

impl Default for TokenEfficiencyValidator {
    fn default() -> Self {
        Self {
            max_skill_md_lines: 30,
            redundancy_patterns: vec![
                "this skill provides".to_string(),
                "this skill handles".to_string(),
                "this skill integrates".to_string(),
                "when this skill is active".to_string(),
                "you can use".to_string(),
            ],
            verbosity_indicators: vec![
                "## Overview".to_string(),
                "## Capabilities".to_string(),
                "## Available Tools".to_string(),
                "## Best Practices".to_string(),
                "## Error Handling".to_string(),
                "## Integration".to_string(),
            ],
        }
    }
}

impl TokenEfficiencyValidator {
    /// Validate skill content for token efficiency
    pub fn validate(&self, content: &str, skill_name: &str) -> TokenEfficiencyReport {
        let lines: Vec<&str> = content.lines().collect();
        let line_count = lines.len();
        
        let mut redundancy_score = 0;
        let mut verbosity_score = 0;
        let mut issues = Vec::new();
        
        // Check line count
        if line_count > self.max_skill_md_lines {
            issues.push(format!(
                "SKILL.md has {} lines (recommended max: {})",
                line_count, self.max_skill_md_lines
            ));
        }
        
        // Check for redundancy patterns
        for pattern in &self.redundancy_patterns {
            if content.to_lowercase().contains(&pattern.to_lowercase()) {
                redundancy_score += 1;
                issues.push(format!("Redundant phrase detected: '{}'", pattern));
            }
        }
        
        // Check for verbosity indicators
        for indicator in &self.verbosity_indicators {
            if content.contains(indicator) {
                verbosity_score += 1;
                issues.push(format!("Verbose section detected: '{}'", indicator));
            }
        }
        
        // Calculate efficiency score
        let efficiency_score = self.calculate_efficiency_score(
            line_count,
            redundancy_score,
            verbosity_score,
        );
        
        TokenEfficiencyReport {
            skill_name: skill_name.to_string(),
            line_count,
            redundancy_score,
            verbosity_score,
            efficiency_score,
            issues,
            recommendations: self.generate_recommendations(&issues),
        }
    }
    
    fn calculate_efficiency_score(&self, lines: usize, redundancy: usize, verbosity: usize) -> f32 {
        let base_score = 100.0;
        let line_penalty = (lines.saturating_sub(self.max_skill_md_lines) as f32) * 2.0;
        let redundancy_penalty = redundancy as f32 * 10.0;
        let verbosity_penalty = verbosity as f32 * 15.0;
        
        (base_score - line_penalty - redundancy_penalty - verbosity_penalty).max(0.0)
    }
    
    fn generate_recommendations(&self, issues: &[String]) -> Vec<String> {
        let mut recommendations = Vec::new();
        
        for issue in issues {
            if issue.contains("lines") {
                recommendations.push("Reduce SKILL.md to maximum 30 lines".to_string());
                recommendations.push("Move detailed content to ADVANCED.md".to_string());
            } else if issue.contains("Redundant") {
                recommendations.push("Remove redundant introductory phrases".to_string());
                recommendations.push("Start with immediate usage instructions".to_string());
            } else if issue.contains("Verbose") {
                recommendations.push("Remove unnecessary section headers".to_string());
                recommendations.push("Focus on practical examples".to_string());
            }
        }
        
        if recommendations.is_empty() {
            recommendations.push("SKILL.md is well-optimized for token efficiency".to_string());
        }
        
        recommendations
    }
}

#[derive(Debug, Clone)]
pub struct TokenEfficiencyReport {
    pub skill_name: String,
    pub line_count: usize,
    pub redundancy_score: usize,
    pub verbosity_score: usize,
    pub efficiency_score: f32,
    pub issues: Vec<String>,
    pub recommendations: Vec<String>,
}

impl TokenEfficiencyReport {
    pub fn format_report(&self) -> String {
        let mut report = format!(
            "Token Efficiency Report for '{}':\n",
            self.skill_name
        );
        report.push_str(&format!("  Lines: {} (max recommended: 30)\n", self.line_count));
        report.push_str(&format!("  Efficiency Score: {:.1}/100\n", self.efficiency_score));
        
        if !self.issues.is_empty() {
            report.push_str("\n  Issues Found:\n");
            for issue in &self.issues {
                report.push_str(&format!("    • {}\n", issue));
            }
        }
        
        if !self.recommendations.is_empty() {
            report.push_str("\n  Recommendations:\n");
            for rec in &self.recommendations {
                report.push_str(&format!("    • {}\n", rec));
            }
        }
        
        report
    }
    
    pub fn is_efficient(&self) -> bool {
        self.efficiency_score >= 80.0 && self.issues.is_empty()
    }
}
```

##  Integration Steps

### **Step 5: Update Skill Validation**

Enhance `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/vtcode-core/src/skills/validation.rs`:

```rust
use crate::skills::token_validator::TokenEfficiencyValidator;

impl SkillValidator {
    /// Validate skill for token efficiency
    pub fn validate_token_efficiency(&self, skill_path: &Path) -> Result<TokenEfficiencyReport> {
        let validator = TokenEfficiencyValidator::default();
        let skill_content = std::fs::read_to_string(skill_path.join("SKILL.md"))?;
        let (_, instructions) = crate::skills::manifest::parse_skill_content(&skill_content)?;
        
        let skill_name = skill_path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        
        Ok(validator.validate(&instructions, skill_name))
    }
    
    /// Run comprehensive validation including token efficiency
    pub async fn validate_comprehensive(&self, skill_path: &Path) -> Result<ComprehensiveValidationReport> {
        let mut report = ComprehensiveValidationReport::new();
        
        // Existing validation checks
        report.manifest_validation = self.validate_manifest(skill_path)?;
        report.security_validation = self.validate_security(skill_path)?;
        report.performance_validation = self.validate_performance(skill_path)?;
        
        // New token efficiency validation
        report.token_efficiency = self.validate_token_efficiency(skill_path)?;
        
        Ok(report)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComprehensiveValidationReport {
    pub manifest_validation: ValidationReport,
    pub security_validation: SecurityReport,
    pub performance_validation: PerformanceReport,
    pub token_efficiency: TokenEfficiencyReport,
    pub overall_score: f32,
}

impl ComprehensiveValidationReport {
    pub fn new() -> Self {
        Self {
            manifest_validation: ValidationReport::default(),
            security_validation: SecurityReport::default(),
            performance_validation: PerformanceReport::default(),
            token_efficiency: TokenEfficiencyReport {
                skill_name: String::new(),
                line_count: 0,
                redundancy_score: 0,
                verbosity_score: 0,
                efficiency_score: 0.0,
                issues: Vec::new(),
                recommendations: Vec::new(),
            },
            overall_score: 0.0,
        }
    }
    
    pub fn calculate_overall_score(&mut self) {
        let token_score = self.token_efficiency.efficiency_score;
        let manifest_score = if self.manifest_validation.status == ValidationStatus::Pass { 100.0 } else { 0.0 };
        let security_score = if self.security_validation.status == ValidationStatus::Pass { 100.0 } else { 0.0 };
        
        self.overall_score = (token_score + manifest_score + security_score) / 3.0;
    }
}
```

### **Step 6: Update CLI Commands**

Enhance `/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/src/cli/skills.rs`:

```rust
/// Validate skill with token efficiency checking
pub async fn handle_skills_validate_efficient(skill_path: &PathBuf) -> Result<()> {
    use vtcode_core::skills::validation::SkillValidator;
    
    println!("Validating skill for token efficiency: {}...", skill_path.display());
    
    let validator = SkillValidator::new(ValidationConfig::default());
    let comprehensive_report = validator.validate_comprehensive(skill_path).await?;
    
    // Print token efficiency report
    println!("\n{}", comprehensive_report.token_efficiency.format_report());
    
    // Print overall validation summary
    println!("\nOverall Validation Score: {:.1}/100", comprehensive_report.overall_score);
    
    if comprehensive_report.token_efficiency.is_efficient() {
        println!(" Skill is optimized for token efficiency");
    } else {
        println!("  Skill needs optimization for token efficiency");
    }
    
    Ok(())
}

/// Optimize skill for token efficiency
pub async fn handle_skills_optimize(skill_path: &PathBuf) -> Result<()> {
    println!("Optimizing skill for token efficiency: {}...", skill_path.display());
    
    let validator = SkillValidator::new(ValidationConfig::default());
    let report = validator.validate_token_efficiency(skill_path)?;
    
    if report.is_efficient() {
        println!(" Skill is already optimized");
        return Ok(());
    }
    
    println!("{}", report.format_report());
    
    if report.line_count > 30 {
        println!("\n Auto-optimization suggestions:");
        println!("  1. Move content after line 30 to ADVANCED.md");
        println!("  2. Remove redundant sections (Overview, Capabilities, etc.)");
        println!("  3. Focus on Quick Start and Common Patterns");
        println!("  4. Use concise language and practical examples");
    }
    
    Ok(())
}
```

##  Testing Your Implementation

### **Step 7: Create Test Skills**

Create test skills to validate the improvements:

```bash
# Create a test skill directory
mkdir -p /tmp/test-skill
cd /tmp/test-skill

# Create optimized SKILL.md
cat > SKILL.md << 'EOF'
---
name: test-pdf-generator
description: Generate PDF documents from data and templates
tags: pdf,document,generation
---

# Test PDF Generator

Generate professional PDF documents using Python libraries.

## Quick Start

```python
from reportlab.pdfgen import canvas
c = canvas.Canvas("output.pdf")
c.drawString(100, 750, "Hello World")
c.save()
```

## Common Patterns

- **Reports**: Use reportlab for complex layouts
- **Invoices**: Combine templates with data
- **Charts**: Generate with matplotlib, export to PDF

## Dependencies

- reportlab (install: pip install reportlab)
- matplotlib (optional, for charts)

[See ADVANCED.md for complex scenarios]
EOF

# Create ADVANCED.md
cat > ADVANCED.md << 'EOF'
# Test PDF Generator - Advanced Usage

## Complex Layouts

For multi-page documents with headers, footers, and complex formatting:

```python
from reportlab.lib.pagesizes import letter
from reportlab.platypus import SimpleDocTemplate, Paragraph, Spacer
from reportlab.lib.styles import getSampleStyleSheet

doc = SimpleDocTemplate("complex.pdf", pagesize=letter)
styles = getSampleStyleSheet()
story = []

# Add content
story.append(Paragraph("Complex Document Title", styles['Title']))
story.append(Spacer(1, 12))
story.append(Paragraph("This is a complex paragraph...", styles['Normal']))

doc.build(story)
```

## Template Integration

For generating PDFs from templates:

```python
from reportlab.lib.pagesizes import letter
from reportlab.pdfgen import canvas

def generate_from_template(template_path, data):
    c = canvas.Canvas("output.pdf", pagesize=letter)
    # Template processing logic here
    return c
```
EOF
```

### **Step 8: Test Validation**

```bash
# Test the new validation
cargo run -- skills validate-efficient /tmp/test-skill

# Expected output should show high efficiency score
# Test token efficiency specifically
cargo run -- skills optimize /tmp/test-skill

# Should show the skill is already optimized
```

### **Step 9: Compare with Old Template**

```bash
# Create skill using old template
cargo run -- skills create /tmp/old-skill

# Compare validation results
cargo run -- skills validate-efficient /tmp/old-skill
cargo run -- skills validate-efficient /tmp/test-skill

# The new skill should have significantly better efficiency score
```

##  Expected Results

### **Before Optimization**
- SKILL.md: 60+ lines
- Token efficiency score: 30-50/100
- Redundant sections: 6-8
- Loading time: 5-10 seconds

### **After Optimization**
- SKILL.md: 20-30 lines
- Token efficiency score: 80-95/100
- Redundant sections: 0-1
- Loading time: 2-4 seconds

### **Progressive Disclosure Benefits**
- **Level 1 (Basic)**: 80% of queries resolved with 30-line SKILL.md
- **Level 2 (Advanced)**: 15% of queries need ADVANCED.md
- **Level 3 (Full)**: 5% of queries need full resources
- **Token savings**: 70% reduction in context usage

##  Monitoring & Maintenance

### **Step 10: Add Metrics Collection**

```rust
pub struct SkillMetrics {
    pub skill_name: String,
    pub complexity_level_used: ComplexityLevel,
    pub token_usage: usize,
    pub loading_time_ms: u64,
    pub success_rate: f32,
}

impl SkillMetrics {
    pub fn track_usage(&mut self, level: ComplexityLevel, tokens: usize, time_ms: u64, success: bool) {
        self.complexity_level_used = level;
        self.token_usage = tokens;
        self.loading_time_ms = time_ms;
        
        // Update success rate (moving average)
        let success_int = if success { 1.0 } else { 0.0 };
        self.success_rate = (self.success_rate * 0.9) + (success_int * 0.1);
    }
}
```

This implementation guide provides concrete steps to bring VT Code's skills authoring in line with Claude API best practices, focusing on **conciseness**, **progressive disclosure**, and **token efficiency**.