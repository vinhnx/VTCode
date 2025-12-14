# Code Execution for Agents - Quick Guide

This guide teaches AI agents how to use vtcode's code execution system for efficient task solving.

Based on [Anthropic's Code execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp), vtcode implements a code-centric agent design that reduces token usage by 90-98% for complex operations.

## When to Use Code Execution

Use `execute_code` when you need to:

1. **Loop over results** without repeated model calls
   ```python
   files = list_files(path="/workspace", recursive=True)
   test_files = [f for f in files if "test" in f and f.endswith(".rs")]
   result = {"count": len(test_files), "files": test_files[:10]}
   ```

2. **Filter large datasets** locally (1000+ items)
   ```python
   all_matches = grep_file(pattern="TODO", path="src")
   grouped = {}
   for match in all_matches.get("matches", []):
       file = match["file"]
       if file not in grouped:
           grouped[file] = []
       grouped[file].append(match["line"])
   result = {"files": len(grouped), "total": len(all_matches.get("matches", []))}
   ```

3. **Transform data** before returning to model
   ```python
   data = read_file(path="config.json")
   lines = data.split("\n")
   config = [l for l in lines if not l.startswith("#")]
   result = {"line_count": len(config), "sample": config[:5]}
   ```

4. **Complex control flow** (conditionals, error handling)
   ```python
   try:
       result = process_data(input)
   except ValueError as e:
       result = {"error": str(e), "retry_count": 3}
   except Exception:
       result = {"error": "unknown", "fallback": True}
   ```

5. **Compose tools together** (chained operations)
   ```python
   files = list_files(path="/src", recursive=True)
   py_files = [f for f in files if f.endswith(".py")]
   
   total_lines = 0
   for file in py_files[:10]:  # Check first 10
       content = read_file(path=file)
       total_lines += len(content.split("\n"))
   
   result = {"files_checked": 10, "total_lines": total_lines, "avg_per_file": total_lines // 10}
   ```

## Step-by-Step: How to Write Code for Execution

### Step 1: Discover Tools

First, find what tools are available:

```python
# Minimal context: just tool names
tools = search_tools(keyword="file", detail_level="name-only")
# Returns: ["list_files", "read_file", "write_file"]

# When ready to use, get full schema
full_tools = search_tools(keyword="read_file", detail_level="full")
```

### Step 2: Write Code

Write Python or JavaScript that:
- Calls MCP tools as library functions
- Filters/transforms results locally
- Returns JSON in a `result = {...}` assignment

```python
# Python example
files = list_files(path="/workspace", recursive=True)
filtered = [f for f in files if "test" in f and f.endswith(".rs")]
result = {"count": len(filtered), "files": filtered[:5]}
```

```javascript
// JavaScript example
const files = await list_files({path: "/workspace", recursive: true});
const filtered = files.filter(f => f.includes("test") && f.endsWith(".rs"));
result = { count: filtered.length, files: filtered.slice(0, 5) };
```

### Step 3: Execute Code

```python
code = '''
files = list_files(path="/workspace", recursive=True)
test_files = [f for f in files if "test" in f]
result = {"count": len(test_files), "files": test_files[:10]}
'''

execute_code(code=code, language="python3")
```

### Step 4: Parse Results

```python
# Returns JSON response with:
{
  "exit_code": 0,
  "stdout": "",
  "stderr": "",
  "duration_ms": 145,
  "result": {
    "count": 42,
    "files": ["tests/test_a.rs", "tests/test_b.rs", ...]
  }
}
```

## Code Patterns

### Pattern 1: Filter Large Results

**Problem**: Tool returns 10,000 items, model context would explode

**Solution**: Filter in code

```python
all_items = expensive_tool(path="/workspace")
filtered = [
    item for item in all_items 
    if item["priority"] > 5 and item["status"] == "active"
]
aggregated = {
    "total": len(all_items),
    "filtered": len(filtered),
    "sample": filtered[:10]
}
result = aggregated
```

**Token Savings**: 90-95% reduction

### Pattern 2: Aggregate Data

**Problem**: Need summary of large dataset

**Solution**: Aggregate in code

```python
files = list_files(path="/workspace", recursive=True)

stats = {
    "total": len(files),
    "by_ext": {},
    "by_dir": {}
}

for f in files:
    ext = f.split(".")[-1]
    dir = f.rsplit("/", 1)[0] if "/" in f else "."
    
    stats["by_ext"][ext] = stats["by_ext"].get(ext, 0) + 1
    stats["by_dir"][dir] = stats["by_dir"].get(dir, 0) + 1

result = {
    "total_files": stats["total"],
    "extensions": len(stats["by_ext"]),
    "directories": len(stats["by_dir"]),
    "top_ext": max(stats["by_ext"], key=stats["by_ext"].get)
}
```

**Token Savings**: 80-85% reduction

### Pattern 3: Conditional Tool Use

**Problem**: Need to call different tools based on conditions

**Solution**: Conditionals in code

```python
files = list_files(path="/workspace", recursive=True)
py_files = [f for f in files if f.endswith(".py")]

result = {}
if len(py_files) > 100:
    # Sample if too many
    sample = py_files[::10]  # Every 10th file
    result["sampled"] = True
    result["files"] = sample
else:
    # Use all if manageable
    result["sampled"] = False
    result["files"] = py_files
```

### Pattern 4: Error Recovery

**Problem**: Tool might fail, but want to continue

**Solution**: Error handling in code

```python
files = list_files(path="/workspace", recursive=True)
results = []

for file in files[:100]:  # Limit iterations
    try:
        content = read_file(path=file)
        results.append({"file": file, "status": "ok", "lines": len(content.split("\n"))})
    except Exception as e:
        results.append({"file": file, "status": "error", "error": str(e)})

result = {
    "processed": len(results),
    "successful": sum(1 for r in results if r["status"] == "ok"),
    "failed": sum(1 for r in results if r["status"] == "error"),
    "sample": results[:5]
}
```

### Pattern 5: Save Reusable Skills

**Problem**: Writing same filtering code repeatedly

**Solution**: Save as skill, reuse later

```python
# First time: develop and save
code = '''
def find_test_files(path, ext=".rs"):
    files = list_files(path=path, recursive=True)
    return [f for f in files if "test" in f and f.endswith(ext)]
'''

save_skill(
    name="find_test_files",
    code=code,
    language="python3",
    description="Find test files by extension",
    inputs=[
        {"name": "path", "type": "str", "required": True},
        {"name": "ext", "type": "str", "required": False}
    ],
    output="List of test file paths",
    tags=["files", "testing"],
    examples=["find_test_files('/src', '.rs')"]
)

# Later: load and use
skill = load_skill("find_test_files")
new_code = skill.code + "\nresult = find_test_files('/workspace')"
execute_code(code=new_code, language="python3")
```

**Token Savings**: 50-80% (reuse reduces new code needed)

## PII Protection

When code processes sensitive data, protect it with tokenization:

```python
# Data with PII
user_data = read_file(path="users.json")

# Tokenize sensitive patterns
tokenized = tokenize_pii(user_data)  # Built-in in code executor

# Work with tokenized version
lines = tokenized.split("\n")
emails_found = sum(1 for l in lines if "__PII_email_" in l)

# Result doesn't leak PII
result = {"pii_patterns_found": emails_found, "status": "safe"}
```

PII is automatically detected and tokenized:
- Email addresses
- Social security numbers
- Credit card numbers
- API keys and tokens
- Phone numbers

The original data never reaches the model.

## Language Support

### Python 3

```python
# All standard library available
import json
import re
from collections import Counter

def analyze(data):
    items = json.loads(data)
    return {k: v for k, v in items.items() if v > 0}

result = analyze('{"a": 1, "b": -1, "c": 3}')
```

### JavaScript (Node.js)

```javascript
// Modern JavaScript (ES2020+)
const data = [1, 2, 3, 4, 5];
const filtered = data
    .filter(x => x > 2)
    .map(x => ({ value: x, doubled: x * 2 }));

result = {
    count: filtered.length,
    items: filtered
};
```

## Performance Expectations

| Operation | Time | Notes |
|-----------|------|-------|
| Cold start (first run) | 500-2000ms | Python/JS interpreter startup |
| Warm execution | 50-500ms | Subsequent runs faster |
| Tool discovery | 10-50ms | Keyword matching |
| SDK generation | 20-100ms | IPC handler creation |
| Result extraction | < 10ms | JSON parsing |

## Troubleshooting

### Code times out

**Problem**: `error: timeout after X seconds`

**Solution**: 
- Break into smaller operations
- Reduce dataset size in loops
- Use `timeout_secs` parameter (max 30s)

```python
#   Slow: processes 100k items in loop
for item in all_items:
    result = slow_tool(item)

#   Fast: batch or sample
for item in all_items[::10]:  # Every 10th
    result = slow_tool(item)
```

### Tool not found in code

**Problem**: `NameError: name 'list_files' is not defined`

**Solution**: Tools are auto-injected by executor
- Make sure `execute_code` is called, not `execute_python`
- Verify tool name is correct with `search_tools()`
- Check tool is available for your language (Python/JS)

### Result is empty

**Problem**: `result = None` or missing

**Solution**: 
- Always assign `result = {...}` at end of code
- Result must be valid JSON (dicts/lists/strings/numbers)
- Don't return file handles or objects

```python
#   Wrong
return {"data": some_file_handle}

#   Right  
file_data = read_file(path="...")
result = {"lines": len(file_data.split("\n"))}
```

## Best Practices

1. **Always filter data in code** - Don't return all 10k results

2. **Use sampling for previews** - Show first 5-10 items, not 1000

3. **Aggregate statistics** - Count, sum, group rather than list details

4. **Handle errors gracefully** - Try/catch so execution continues

5. **Keep code simple** - Complex logic in model, filtering in code

6. **Reuse with skills** - Save patterns for reuse

7. **Respect timeouts** - Optimize code that runs > 2 seconds

8. **Protect sensitive data** - Use tokenization for PII

## Quick Examples

### Example 1: Count TODOs

```python
matches = grep_file(pattern="TODO:", path="src")
todos = {}
for m in matches.get("matches", []):
    f = m["file"]
    todos[f] = todos.get(f, 0) + 1
result = {
    "files_with_todos": len(todos),
    "total_todos": len(matches.get("matches", [])),
    "most_common": max(todos, key=todos.get)
}
```

### Example 2: Code Size Stats

```python
files = list_files(path="src", recursive=True)
stats = {"total": 0, "by_type": {}, "largest": {"file": "", "lines": 0}}
for f in files:
    if not any(f.endswith(ext) for ext in [".rs", ".py", ".js"]):
        continue
    try:
        content = read_file(path=f)
        lines = len(content.split("\n"))
        ext = f.split(".")[-1]
        stats["total"] += lines
        stats["by_type"][ext] = stats["by_type"].get(ext, 0) + lines
        if lines > stats["largest"]["lines"]:
            stats["largest"] = {"file": f, "lines": lines}
    except:
        pass
result = stats
```

### Example 3: Find Broken Imports

```python
files = list_files(path="src", recursive=True)
py_files = [f for f in files if f.endswith(".py")]
broken = []
for f in py_files[:20]:  # Check first 20
    try:
        content = read_file(path=f)
        imports = [l for l in content.split("\n") if l.startswith("import ") or l.startswith("from ")]
        # Simple check: look for unused imports
        for imp in imports:
            module = imp.split()[1].split(".")[0]
            if module not in content:
                broken.append({"file": f, "import": imp})
    except:
        pass
result = {"broken_imports": broken[:10], "total_checked": min(20, len(py_files))}
```

## See Also

- [MCP Code Execution Architecture](./mcp_code_execution.md)
- [Integration Testing Guide](./MCP_INTEGRATION_TESTING.md)
- [Tool Discovery](./tools/)
- [Skill Management](./EXECUTE_CODE_USAGE.md)
