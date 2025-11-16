# Per-Call Max Tokens Support

## Overview

All tools that return large outputs now support a `max_tokens` parameter for per-call token budget limiting. This allows fine-grained control over token consumption on a tool-by-tool basis.

## Supported Tools

### File I/O Tools
- **read_file**: `max_tokens` parameter controls maximum tokens in file output
- **grep_file**: `max_tokens` parameter limits search result output
- **list_files**: `max_tokens` parameter caps directory listing output

### Terminal/PTY Tools
- **send_pty_input**: `max_tokens` parameter limits captured output from PTY session
- **read_pty_session**: `max_tokens` parameter restricts session output (screen + scrollback)
- **run_command**: `max_tokens` parameter caps stdout/stderr output from terminal commands

### Network Tools
- **web_fetch**: Implicit token limiting through content processing

## Usage Examples

### Limiting File Output

Read a large file but only include the first N tokens of output:

```json
{
  "tool_use": {
    "name": "read_file",
    "input": {
      "path": "large_log_file.txt",
      "max_tokens": 5000
    }
  }
}
```

### Limiting Terminal Output

Capture PTY session output with token constraints:

```json
{
  "tool_use": {
    "name": "read_pty_session",
    "input": {
      "session_id": "shell_123",
      "max_tokens": 10000,
      "include_screen": true,
      "include_scrollback": true
    }
  }
}
```

### Sending Commands with Output Limiting

Send input to a PTY session and limit captured output:

```json
{
  "tool_use": {
    "name": "send_pty_input",
    "input": {
      "session_id": "shell_123",
      "input": "ls -la /large_directory",
      "max_tokens": 8000,
      "wait_ms": 500
    }
  }
}
```

### Limiting Command Output

Execute a command but limit the output tokens:

```json
{
  "tool_use": {
    "name": "run_command",
    "input": {
      "command": ["find", "/large_directory", "-name", "*.log"],
      "max_tokens": 5000
    }
  }
}
```

## Token Budget Hierarchy

1. **Per-Call max_tokens** (highest priority): Specified in individual tool calls
2. **Tool-Wide Defaults**: Built-in defaults for each tool (typically 25,000 tokens)
3. **Global Context Budget**: Overall LLM context window limits

When `max_tokens` is specified, it overrides any tool-wide defaults for that specific call.

## Implementation Details

### Token Counting

The system uses both:
- **Exact Tokenization**: When available (HuggingFace tokenizers for specific models)
- **Approximation**: Fallback heuristics based on character/line/word counts

Token counts are conservative estimates. Content like code, structured output, and logs may use more tokens than prose due to higher token fragmentation.

### Truncation Strategy

When output exceeds `max_tokens`:

1. **Output is truncated** at the token boundary
2. **Metadata is preserved**: Exit codes, status indicators, command info remain intact
3. **Indicator added**: "output_truncated": true flag in response
4. **Remaining context available**: Use `read_pty_session` again to fetch more data

### Output Format

Tool responses include token information:

For file/PTY tools:
```json
{
  "success": true,
  "output": "... truncated to max_tokens ...",
  "output_truncated": true,
  "tokens_used": 5000,
  "max_tokens": 5000
}
```

For run_command tool:
```json
{
  "success": true,
  "exit_code": 0,
  "stdout": "... truncated by max_tokens ...",
  "stderr": "... truncated by max_tokens ...",
  "mode": "terminal",
  "pty_enabled": false,
  "command": "find /large_directory -name '*.log'",
  "applied_max_tokens": 5000
}
```

## Best Practices

### When to Use max_tokens

✓ Reading large log files
✓ Listing directories with thousands of files  
✓ Capturing verbose command output
✓ Batch operations producing large results

### When NOT to Use max_tokens

✗ Small outputs that naturally fit within token budget
✗ When complete output is critical (use pagination instead)
✗ Initial exploration (use reasonable defaults first)

### Recommended Token Budgets

| Operation | Recommended max_tokens |
|-----------|----------------------|
| Small file read | 2,000 |
| Medium file read | 5,000 |
| Large file read | 15,000 |
| Directory listing | 3,000 |
| Command output | 10,000 |
| Full session transcript | 25,000 |

## Configuration

### Default Tool Limits

All tools have sensible defaults:
- Default maximum per tool response: **25,000 tokens**
- Can be overridden per-call with `max_tokens`
- Can be configured globally via `vtcode.toml` future releases

### Disabling Limits

To disable token limiting for a tool (use full output):
- Omit `max_tokens` parameter
- Uses tool's default limit (typically 25,000)

## Performance Impact

- **Minimal overhead**: Token counting uses fast heuristics by default
- **Caching**: Tokenizer is initialized once per session
- **Streaming**: Large outputs can be read incrementally via multiple tool calls

## Examples in Practice

### Example 1: Reading a Large Config File

```python
# First, get just the main config structure
result = read_file({
    "path": "/etc/large_config.yaml",
    "max_tokens": 3000  # Just the beginning
})

# If needed, read a specific section
result = grep_file({
    "pattern": "database_config",
    "path": "/etc/large_config.yaml", 
    "max_tokens": 5000
})
```

### Example 2: Debugging with Terminal Output

```python
# Run command and limit output
result = send_pty_input({
    "session_id": "debug_session",
    "input": "dmesg | head -100",
    "max_tokens": 8000,
    "wait_ms": 1000
})

# Get more details if needed
if result["output_truncated"]:
    more_result = send_pty_input({
        "session_id": "debug_session",
        "input": "dmesg | tail -100",
        "max_tokens": 8000,
        "wait_ms": 1000
    })
```

### Example 3: Batch File Operations

```python
# List large directory with limit
files = list_files({
    "path": "/large_directory",
    "max_tokens": 5000,
    "mode": "recursive"
})

# Use pagination for complete results
files_page2 = list_files({
    "path": "/large_directory",
    "page": 2,
    "per_page": 50,
    "mode": "recursive"
})
```

## Troubleshooting

### "output_truncated": true but need more data

Use pagination (for list_files) or read the remaining data in subsequent calls:

```json
{
  "tool_use": {
    "name": "read_pty_session",
    "input": {
      "session_id": "your_session",
      "max_tokens": 25000
    }
  }
}
```

### Token count seems inaccurate

Token estimation uses heuristics. For exact counts:
- Use a model with native tokenizer support (Gemini 2.5, GPT-5, Claude 3+)
- Code and structured content has higher token density than prose
- ANSI color codes and control sequences are stripped before tokenization

### Performance degradation with large max_tokens

- Consider breaking work into smaller chunks
- Use pagination parameters when available  
- For terminal work, capture outputs more frequently with smaller limits

## Future Enhancements

- [ ] Global max_tokens configuration in vtcode.toml
- [ ] Streaming responses for very large outputs
- [ ] Adaptive token limiting based on remaining context budget
- [ ] Per-component token tracking (see stats with `/tokens` command)
