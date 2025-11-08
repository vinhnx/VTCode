# execute_code Tool - Usage Guide

The `execute_code` tool allows agents to write and execute Python3 or JavaScript code in a sandboxed environment with full access to MCP tools as library functions.

## Basic Syntax

```json
{
  "code": "your code here",
  "language": "python3 or javascript",
  "timeout_secs": 30  // optional, default is 30
}
```

## Python Examples

### Example 1: List and Filter Files

```python
code = '''
files = list_files(path="/workspace", recursive=True)
rs_files = [f for f in files if f.endswith(".rs")]
test_files = [f for f in rs_files if "test" in f]
result = {
    "total_rs_files": len(rs_files),
    "test_files": len(test_files),
    "sample": test_files[:5]
}
'''

execute_code({"code": code, "language": "python3"})
```

**Response**:
```json
{
  "exit_code": 0,
  "duration_ms": 245,
  "stdout": "",
  "stderr": "",
  "result": {
    "total_rs_files": 145,
    "test_files": 32,
    "sample": ["tests/test_main.rs", "tests/test_utils.rs", ...]
  }
}
```

### Example 2: Search and Aggregate

```python
code = '''
import json
results = grep_file(pattern="TODO", path="/workspace")
by_file = {}
for item in results:
    file = item.get("file", "unknown")
    if file not in by_file:
        by_file[file] = 0
    by_file[file] += 1

result = {
    "total_todos": len(results),
    "files_with_todos": len(by_file),
    "summary": by_file
}
'''

execute_code({"code": code, "language": "python3"})
```

### Example 3: Data Transformation

```python
code = '''
files = list_files(path="/workspace/src")
info = {
    "timestamp": "2025-11-08",
    "files": files,
    "count": len(files)
}
result = info
'''

execute_code({"code": code, "language": "python3"})
```

## JavaScript Examples

### Example 1: Async Tool Calls

```javascript
code = `
const tools = await search_tools({keyword: "file", detail_level: "name-only"});
const fileTools = tools.results || [];
result = {
  available_file_tools: fileTools.length,
  tools: fileTools
};
`

execute_code({"code": code, "language": "javascript"})
```

### Example 2: Multiple Async Operations

```javascript
code = `
async function processDirectory(dir) {
  const files = await list_files({path: dir});
  return {
    directory: dir,
    file_count: files.length,
    sample: files.slice(0, 3)
  };
}

const dirs = ["src", "tests"];
const results = [];
for (const dir of dirs) {
  results.push(await processDirectory(dir));
}
result = {
  analysis: results,
  total_dirs: dirs.length
};
`

execute_code({"code": code, "language": "javascript"})
```

### Example 3: Error Handling

```javascript
code = `
try {
  const content = await read_file({path: "/workspace/README.md"});
  const lines = content.split("\\n").length;
  result = {
    success: true,
    lines: lines,
    content_size: content.length
  };
} catch (e) {
  result = {
    success: false,
    error: e.message
  };
}
`

execute_code({"code": code, "language": "javascript"})
```

## Available MCP Tools in Code

Inside your code, you can call any registered MCP tool as a function. Common tools include:

### File Operations
- `read_file(path)` - Read file contents
- `write_file(path, content)` - Write file
- `create_file(path, content)` - Create new file
- `list_files(path, recursive)` - List directory contents
- `delete_file(path)` - Delete file

### Search
- `grep_file(pattern, path, case_sensitive)` - Search with regex
- `ast_grep_search(pattern, path)` - AST-based search

### Tools Discovery
- `search_tools(keyword, detail_level)` - Find available MCP tools

### Version Control
- `git_diff()` - Get git diff

## Response Structure

Every `execute_code` call returns:

```json
{
  "exit_code": 0,  // 0 = success, non-zero = error
  "duration_ms": 245,  // Execution time in milliseconds
  "stdout": "captured output",  // Any print statements
  "stderr": "errors if any",  // Any error output
  "result": {  // JSON result (optional, if `result = {...}` was set)
    "custom": "data"
  }
}
```

## Return Value Convention

To return structured data from your code, assign it to a `result` variable:

**Python**:
```python
result = {"key": "value"}  # Must be serializable to JSON
```

**JavaScript**:
```javascript
result = {key: "value"};  // Must be serializable to JSON
```

The tool will parse and return this as the `result` field in the response.

## Timeout Configuration

By default, code execution times out after 30 seconds. Customize with `timeout_secs`:

```json
execute_code({
  "code": "...",
  "language": "python3",
  "timeout_secs": 60
})
```

**Valid range**: 1-300 seconds

## Output Capture

- **stdout**: Captured from `print()` (Python) or `console.log()` (JavaScript)
- **stderr**: Captured from errors and exceptions
- **result**: JSON extracted from `result = {...}` assignment

All three are included in the response, even if empty.

## Error Handling

If your code raises an exception:

```python
result = {
  "exit_code": 1,
  "stderr": "Traceback (most recent call last)...",
  "result": None  // Not present if no result was set
}
```

The tool always returns, even on errors - check `exit_code` and `stderr` for diagnostics.

## Performance Tips

1. **Filter in code**: Don't return 10k items to the model, filter to top 10
2. **Aggregate data**: Sum, count, and group in code before returning
3. **Use loops**: Call tools in loops rather than sequential tool calls
4. **Error handling**: Implement retry logic in code, not via repeated agent calls

## Example: Real-World Usage Pattern

```python
code = '''
# Find all test files
files = list_files(path="/workspace", recursive=True)
test_files = [f for f in files if "test" in f and f.endswith(".rs")]

# Search for failing tests
failures = []
for test_file in test_files[:10]:  # Limit to first 10
  results = grep_file(pattern="TODO|FIXME", path=test_file)
  if results:
    failures.append({
      "file": test_file,
      "issues": len(results),
      "sample": results[0] if results else None
    })

# Return summary
result = {
  "total_test_files": len(test_files),
  "analyzed": min(10, len(test_files)),
  "files_with_issues": len(failures),
  "details": failures
}
'''

response = execute_code({
  "code": code,
  "language": "python3",
  "timeout_secs": 45
})

# Use response["result"] for structured output
# Check response["exit_code"] for errors
# Check response["stderr"] for error details
```

## When to Use execute_code

✅ **Good Use Cases**:
- Filtering large result sets
- Aggregating data from multiple tool calls
- Complex control flow (loops, conditionals)
- Data transformation and cleaning
- Error handling and retries

❌ **Not Ideal**:
- Simple single tool calls (use tools directly)
- Simple conditionals (use tool parameters instead)
- Small data sets (direct tool response is fine)

## Debugging

If your code fails:

1. Check `exit_code` (non-zero = error)
2. Read `stderr` for error message
3. Check `stdout` for print debug output
4. Verify tool names match (use `search_tools` first)
5. Ensure parameters match tool schema

Example debugging code:

```python
code = '''
try:
  # Test tool availability
  tools = search_tools(keyword="list_files", detail_level="full")
  print("Found tools:", tools)
  
  # Try the operation
  files = list_files(path="/workspace")
  print(f"Got {len(files)} files")
  
  result = {"success": True, "count": len(files)}
except Exception as e:
  result = {"success": False, "error": str(e)}
'''
```

## Limitations

- **Network**: No network access by default
- **Duration**: Default 30s timeout (configurable up to 300s)
- **Memory**: 256MB default limit
- **Output**: 10MB max output capture
- **File System**: Limited to workspace directory
