# VT Code Claude API Skills Integration Guide

## Overview

This guide aligns VT Code's skills implementation with the official Claude API patterns while adapting to VT Code's local execution model.

## Key Differences from Claude API

### Claude API Approach
- **Remote execution**: Skills execute on Anthropic's servers
- **API key required**: `ANTHROPIC_API_KEY` environment variable
- **Container-based**: Skills run in isolated containers
- **Automatic file handling**: Files managed by Anthropic infrastructure

### VT Code Adaptation
- **Local execution**: Skills provide instructions for local implementation
- **No API key required**: Works offline with local tools
- **Tool-based**: Uses `execute_code`, `write_file`, etc.
- **Manual file handling**: Direct filesystem operations

## Enhanced Implementation Pattern

### 1. Proper Skills Loading
```rust
// Load skill instructions (existing pattern - correct)
skill(name="pdf-report-generator")

// Skill provides instructions like:
"""
1. Check environment for PDF libraries
2. Use execute_code with fallback implementations
3. Verify file creation
4. Handle errors gracefully
"""
```

### 2. Environment-Aware Implementation
```python
def implement_pdf_skill(specification):
    """Implement PDF generation following skill instructions."""
    
    # Step 1: Environment check (from Claude guide best practices)
    env_check = check_environment()
    
    # Step 2: Try multiple implementation methods
    methods = [
        ('anthropic_container', try_anthropic_container),
        ('local_fpdf', try_fpdf_implementation),
        ('local_reportlab', try_reportlab_implementation),
        ('mock_pdf', create_mock_pdf)
    ]
    
    for method_name, method_func in methods:
        try:
            result = method_func(specification)
            if result['success']:
                return result
        except Exception as e:
            print(f"{method_name} failed: {e}")
            continue
    
    return {'success': False, 'error': 'All methods failed'}
```

### 3. Container Skills Integration (When API Available)
```python
def try_anthropic_container(specification):
    """Try using real Anthropic container skills if API available."""
    
    # Check for API key (Claude API requirement)
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        return {'success': False, 'reason': 'API key not found'}
    
    try:
        import anthropic
        client = anthropic.Anthropic(api_key=api_key)
        
        # Use proper Claude API format from guide
        response = client.beta.messages.create(
            model="claude-3-5-sonnet-20241022",  # Correct model name
            max_tokens=4096,
            tools=[{"type": "code_execution", "name": "bash"}],  # Proper tool format
            messages=[{
                "role": "user", 
                "content": f"Generate PDF with: {specification}"
            }],
            container={
                "type": "skills",
                "skills": [{"type": "anthropic", "skill_id": "pdf", "version": "latest"}]
            },
            betas=["code-execution-2025-08-25", "skills-2025-10-02"]  # Required beta headers
        )
        
        # Extract file references (from Claude guide)
        file_ids = []
        for item in response.content:
            if hasattr(item, 'type') and item.type == 'file':
                file_ids.append(item.file_id)
        
        return {
            'success': True,
            'method': 'anthropic_container',
            'file_ids': file_ids,
            'response': response
        }
        
    except Exception as e:
        return {'success': False, 'error': str(e)}
```

### 4. Local Implementation Fallbacks
```python
def try_fpdf_implementation(specification):
    """Local PDF generation using FPDF library."""
    try:
        from fpdf import FPDF
        
        pdf = FPDF()
        pdf.add_page()
        
        # Implementation following Claude guide patterns
        pdf.set_font('Arial', 'B', 16)
        pdf.cell(0, 10, specification.get('title', 'Document'), 0, 1, 'C')
        
        # Add content sections
        for section, content in specification.get('sections', {}).items():
            pdf.set_font('Arial', 'B', 12)
            pdf.cell(0, 8, section, 0, 1)
            pdf.set_font('Arial', '', 11)
            pdf.multi_cell(0, 6, str(content))
        
        # Save to workspace
        output_path = f"/tmp/{specification.get('filename', 'document')}.pdf"
        pdf.output(output_path)
        
        return {
            'success': True,
            'method': 'local_fpdf',
            'file': output_path,
            'size': os.path.getsize(output_path)
        }
        
    except ImportError:
        return {'success': False, 'reason': 'FPDF not available'}
```

### 5. Mock Implementation (Always Available)
```python
def create_mock_pdf(specification):
    """Create structured text as PDF fallback."""
    
    content = []
    content.append("=" * 60)
    content.append(f"MOCK PDF: {specification.get('title', 'Document')}")
    content.append("=" * 60)
    content.append(f"Generated: {datetime.now().strftime('%Y-%m-%d %H:%M')}")
    content.append("")
    
    # Add sections
    for section, data in specification.get('sections', {}).items():
        content.append(f"\n{section.upper()}")
        content.append("-" * 40)
        content.append(str(data))
    
    content.append("\n" + "=" * 60)
    content.append("Note: This is a mock PDF representation.")
    content.append("Install fpdf or reportlab for actual PDF generation.")
    content.append("=" * 60)
    
    # Save as text file
    output_path = f"/tmp/{specification.get('filename', 'document')}.txt"
    with open(output_path, 'w') as f:
        f.write('\n'.join(content))
    
    return {
        'success': True,
        'method': 'mock_pdf',
        'file': output_path,
        'size': os.path.getsize(output_path),
        'note': 'Mock generation - no PDF libraries available'
    }
```

## Enhanced VT Code Usage Pattern

### 1. Load Skill
```rust
// Load skill instructions
skill(name="pdf-report-generator")
```

### 2. Check Environment
```rust
execute_code(
    language="python3",
    code="""
import os
print(" Environment Check:")
print(f"ANTHROPIC_API_KEY: {'Set' if os.environ.get('ANTHROPIC_API_KEY') else 'Not set'}")

# Check available libraries
libraries = ['anthropic', 'fpdf', 'reportlab']
for lib in libraries:
    try:
        __import__(lib)
        print(f" {lib}: Available")
    except ImportError:
        print(f" {lib}: Not available")
"""
)
```

### 3. Implement with Fallbacks
```rust
execute_code(
    language="python3",
    code="""
# Enhanced implementation following Claude guide patterns
spec = {
    'title': 'Monthly Sales Report',
    'filename': 'sales_report',
    'sections': {
        'Executive Summary': 'Revenue up 15% this month',
        'Sales by Region': 'North: $45k, South: $32k, East: $28k, West: $20k',
        'Recommendations': 'Focus marketing on underperforming regions'
    }
}

# Try implementation methods in order
result = implement_pdf_skill(spec)
print(f"Result: {result}")
"""
)
```

### 4. Verify Results
```rust
list_files(path="/tmp", pattern="*sales_report.*")
```

## Key Improvements from Claude Guide

### 1. **Proper Model Names**
- Use `claude-3-5-sonnet-20241022` instead of `claude-4-5-sonnet`
- Follow official model naming conventions

### 2. **Beta Headers**
- Include `code-execution-2025-08-25` and `skills-2025-10-02`
- Required for container skills functionality

### 3. **Tool Format**
- Use `tools=[{"type": "code_execution", "name": "bash"}]`
- Follow exact Claude API specification

### 4. **File Handling**
- Extract file IDs from response: `block.file_id`
- Handle Files API for downloads when using container skills

### 5. **Error Handling**
- Graceful fallback when API unavailable
- Clear user communication about method used

### 6. **Environment Awareness**
- Check library availability before use
- Provide multiple implementation paths
- Always have fallback option

## Benefits of Enhanced Approach

1. **Claude API Compatible**: Follows official patterns when API available
2. **Offline Capable**: Works without API key or network
3. **Transparent**: Clear feedback about implementation method
4. **Robust**: Multiple fallback strategies
5. **User-Friendly**: Clear error messages and guidance
6. **Future-Proof**: Ready for real container integration

This approach bridges the gap between VT Code's local execution model and the official Claude API container skills pattern, providing the best of both worlds.