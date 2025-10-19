# Tool Policy System Implementation

## Summary

I have successfully implemented a comprehensive tool policy system for VT Code that provides user control over which tools the agent can execute. The system stores user choices persistently in `~/.vtcode/tool-policy.json` and minimizes repeated prompts while maintaining security.

## Key Features Implemented

### 1. Core Policy System (`vtcode-core/src/tool_policy.rs`)

-   **ToolPolicy enum**: Allow, Prompt, Deny policies
-   **ToolPolicyConfig struct**: JSON-serializable configuration
-   **ToolPolicyManager**: Main management interface
-   **Persistent storage**: Automatic save/load from `~/.vtcode/tool-policy.json`

### 2. CLI Commands (`vtcode-core/src/cli/tool_policy_commands.rs`)

-   `vtcode tool-policy status` - Show current policies
-   `vtcode tool-policy allow <tool>` - Allow specific tool
-   `vtcode tool-policy deny <tool>` - Deny specific tool
-   `vtcode tool-policy prompt <tool>` - Set tool to prompt
-   `vtcode tool-policy allow-all` - Allow all tools
-   `vtcode tool-policy deny-all` - Deny all tools
-   `vtcode tool-policy reset-all` - Reset all to prompt

### 3. Integration with Tool Registry

-   Modified `ToolRegistry` to include policy checking
-   Policy validation before tool execution
-   Automatic tool list updates when tools are added/removed
-   Graceful fallback to "prompt" for unknown tools

### 4. User Experience Flow

-   **First run**: All tools prompt for user approval
-   **User choice**: Approve/deny with persistent storage
-   **Future runs**: Respect stored choices without re-prompting
-   **Dynamic updates**: New tools auto-added as "prompt", removed tools cleaned up

## Files Created/Modified

### New Files

1. `vtcode-core/src/tool_policy.rs` - Core policy management system
2. `vtcode-core/src/cli/tool_policy_commands.rs` - CLI command handlers
3. `docs/tool-policy-system.md` - Comprehensive documentation
4. `tool_policy_test/` - Standalone test demonstrating functionality

### Modified Files

1. `vtcode-core/src/lib.rs` - Added tool_policy module exports
2. `vtcode-core/src/cli/mod.rs` - Added tool_policy_commands module
3. `vtcode-core/src/cli/args.rs` - Added ToolPolicy command to CLI
4. `vtcode-core/src/tools/registry.rs` - Integrated policy checking
5. `vtcode-core/Cargo.toml` - Added dirs dependency
6. `src/main.rs` - Added ToolPolicy command handler

## Configuration Format

The system stores configuration in `~/.vtcode/tool-policy.json`:

```json
{
    "version": 1,
    "available_tools": [
        "read_file",
        "write_file",
        "list_files",
        "run_terminal_cmd",
        "grep_file"
    ],
    "policies": {
        "read_file": "allow",
        "write_file": "prompt",
        "list_files": "allow",
        "run_terminal_cmd": "deny",
        "grep_file": "allow"
    }
}
```

## User Workflow

### Initial Setup

1. User runs agent for first time
2. Agent attempts to use a tool (e.g., `read_file`)
3. System prompts: "Allow the agent to use 'read_file'? [y/N]"
4. User choice is stored permanently
5. Future uses of `read_file` execute without prompting

### Policy Management

```bash
# View current status
vtcode tool-policy status

# Allow a specific tool
vtcode tool-policy allow read_file

# Deny a dangerous tool
vtcode tool-policy deny run_terminal_cmd

# Reset everything to prompt again
vtcode tool-policy reset-all
```

## Security Benefits

### User Control

-   **Explicit consent**: No tool runs without user permission initially
-   **Persistent choices**: Decisions remembered across sessions
-   **Easy modification**: Policies can be changed anytime via CLI

### Defense in Depth

-   **Tool-level granularity**: Control individual tools, not just commands
-   **Automatic updates**: New tools require explicit approval
-   **Clean removal**: Deleted tools automatically removed from config

### Audit Trail

-   **Transparent policies**: Clear visibility into what's allowed/denied
-   **Version tracking**: Configuration versioned for future compatibility
-   **Status reporting**: Easy to review current security posture

## Technical Implementation

### Policy Checking Flow

```rust
// Before executing any tool
if !policy_manager.should_execute_tool(tool_name)? {
    return Err(anyhow!("Tool '{}' execution denied by policy", tool_name));
}

// Tool executes normally if allowed
let result = execute_tool(tool_name, args).await?;
```

### Dynamic Tool Management

-   **Addition**: New tools automatically added as "prompt"
-   **Removal**: Deleted tools automatically removed from config
-   **Updates**: Tool list synchronized on each agent startup

### Error Handling

-   **Graceful fallback**: Unknown tools default to "prompt"
-   **Clear messages**: Informative error messages for denied tools
-   **Recovery**: Automatic config recreation if file corrupted

## Testing

Created comprehensive test suite demonstrating:

-   Policy serialization/deserialization
-   Tool addition and removal
-   Policy setting and retrieval
-   Configuration persistence
-   Status display with color coding

Test results show all functionality working correctly.

## Benefits Achieved

### For Users

-   **Control**: Complete control over agent tool usage
-   **Security**: No unexpected tool execution
-   **Convenience**: Minimal prompts after initial setup
-   **Transparency**: Clear visibility into agent capabilities

### For Developers

-   **Extensible**: Easy to add new tools with automatic policy integration
-   **Maintainable**: Clean separation of concerns
-   **Robust**: Comprehensive error handling and recovery
-   **Documented**: Full documentation and examples

## Future Enhancements

The system is designed for extensibility:

-   **Time-based policies**: Allow tools for limited time periods
-   **Context-aware policies**: Different policies per project/directory
-   **Policy templates**: Predefined configurations for common use cases
-   **Integration**: External policy management system integration

## Performance and Reliability Guidelines

### Pagination Behavior

The system implements optimized pagination for handling large datasets:

-   **Default Batch Size**: `per_page` defaults to 50 items for optimal performance
-   **Automatic Adjustment**: System automatically reduces batch size for very large directories
-   **Error Recovery**: Comprehensive logging and graceful handling of pagination failures
-   **Fallback Strategy**: On timeouts or API failures, automatically retry with smaller batches (50 items)
-   **Monitoring**: All pagination operations are logged with structured data for debugging

### Tool-Specific Guidelines

-   **File Operations**: Always use pagination for directories with >100 items
-   **Search Operations**: Monitor `has_more` flag and continue pagination as needed
-   **Error Handling**: Tools log detailed error information including timestamps, request parameters, and stack traces
-   **Performance Monitoring**: Track processing time for large datasets and adjust batch sizes accordingly

### Reliability Features

-   **Structured Logging**: JSON-formatted logs for easy integration with monitoring tools
-   **Timeout Handling**: Automatic retry with exponential backoff for transient failures
-   **Data Integrity**: Validation of pagination parameters to prevent invalid requests
-   **Resource Limits**: Configurable caps on maximum items processed per operation

## Intelligent Chunking System

### Overview

VT Code implements automatic chunking strategies to handle large files and verbose outputs efficiently, preventing token limits and memory issues while preserving important content.

### Chunking Thresholds

-   **File Reading**: 2,000 lines (shows first 800 + last 800 lines)
-   **File Writing**: 500KB content (writes in 50KB chunks)
-   **Terminal Output**: 3,000 lines (shows first 1,000 + last 1,000 lines)

### Tool-Specific Chunking Rules

#### read_file Tool

-   **Automatic Detection**: Files >10,000 lines are automatically chunked
-   **Content Preservation**: Shows headers, footers, and key sections
-   **Metadata**: Returns `truncated: true`, `total_lines`, and `shown_lines`
-   **Customization**: `chunk_lines` parameter allows custom thresholds

#### write_file Tool

-   **Chunked Writing**: Large content split into 100KB chunks
-   **Atomicity**: Sequential writes ensure data integrity
-   **Progress Tracking**: Returns `chunked: true` and `chunks_written` count

#### edit_file Tool

-   **Inherited Chunking**: Uses chunked read/write for large files
-   **Context Preservation**: Maintains line numbers during edits
-   **Diff Merging**: Handles overlaps and conflicts in chunked operations

#### run_terminal_cmd Tool

-   **Output Truncation**: Verbose commands truncated to essential content
-   **Efficiency**: Uses `head`/`tail` for large output processing
-   **Summary**: Includes truncation indicators and total line counts

### Logging and Monitoring

-   **Structured Logs**: JSON-formatted chunking events with timestamps
-   **Performance Metrics**: File sizes, chunk details, and processing times
-   **Error Tracking**: Failed chunking operations with detailed context
-   **Debugging Support**: Comprehensive logging for troubleshooting

### User Experience

-   **Transparent Operation**: Users are informed when content is truncated
-   **Access to Full Content**: Tools provide metadata for requesting specific sections
-   **Performance Optimization**: Chunking prevents timeouts and memory exhaustion
-   **Backward Compatibility**: Existing tool usage remains unchanged

## Conclusion

The tool policy system successfully addresses the requirement to:

1. Prompt users for tool approval on first use
2. Remember user choices persistently
3. Minimize repeated prompts in future runs
4. Handle dynamic tool list changes
5. Provide CLI management interface
6. Store configuration in `~/.vtcode/tool-policy.json`

The implementation provides a robust, user-friendly, and secure foundation for controlling agent tool execution while maintaining a smooth user experience.
