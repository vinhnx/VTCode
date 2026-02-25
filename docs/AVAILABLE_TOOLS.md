# VT Code Available Tools Reference

This document provides a comprehensive list of all tools available to the VT Code agent, organized by category and usage patterns. Tools are defined in the tool registry and exposed to LLMs via the system prompt.

## Tool Categories

### 1. Core Unified Tools (Primary Interface for Agents)

The agent should primarily use these 3 unified tools, which handle multiple actions through parameters:

#### **unified_search** (Search & Discovery)
- **Purpose**: Unified search, discovery, and code intelligence
- **Primary Actions**:
  - `action='grep'` - Regex/literal search in files
  - `action='list'` - List files and directories
  - `action='tools'` - Discover available tools
  - `action='errors'` - Get session errors
  - `action='agent'` - Agent diagnostics
  - `action='web'` - Fetch web content
  - `action='skill'` - Load or interact with skills
- **Aliases** (all map to `unified_search`):
  - `grep_file` - Search files
  - `list_files` - List directory contents
  - `agent_info` - Agent status/diagnostics
  - `web_fetch` - Fetch URLs
  - `search_tools` - Find tools
  - `search`, `find` - Generic search

#### **unified_exec** (Shell Execution & Code Execution)
- **Purpose**: Execute shell commands and code
- **Primary Actions**:
  - `action='run'` - Execute shell commands (one-off)
  - `action='code'` - Execute Python or JavaScript
  - `action='write'` - Write to stdin of running session
  - `action='poll'` - Read output from running session
  - `action='list'` - List PTY sessions
  - `action='close'` - Close PTY session
- **Aliases** (all map to `unified_exec`):
  - `run_pty_cmd` - Run command with PTY
  - `execute_code` - Execute code
  - `create_pty_session` - Create interactive session
  - `list_pty_sessions` - List sessions
  - `close_pty_session` - Close session
  - `send_pty_input` - Write to session stdin
  - `read_pty_session` - Read session output
  - `exec_pty_cmd`, `exec`, `shell` - Shell execution

#### **unified_file** (File Operations)
- **Purpose**: Unified file reading, writing, and modification
- **Primary Actions**:
  - `action='read'` - Read file contents
  - `action='write'` - Write entire file
  - `action='edit'` - Surgical text replacement
  - `action='patch'` - Apply unified diff patches
  - `action='delete'` - Delete file/directory
  - `action='move'` - Move/rename file
  - `action='copy'` - Copy file
- **Aliases** (all map to `unified_file`):
  - `read_file` - Read file
  - `write_file` - Write file
  - `edit_file` - Edit file
  - `apply_patch` - Apply patch
  - `delete_file` - Delete file
  - `create_file` - Create new file
  - `move_file` - Move file
  - `copy_file` - Copy file
  - `file_op` - Generic file operation

---

### 2. Skill Management Tools (Progressive Disclosure)

Skills are dormant by default and activated on-demand to save tokens:

#### **list_skills**
- **Purpose**: List all available skills (local and dormant system utilities)
- **Parameters**:
  - `query` (optional) - Filter by name (case-insensitive)
  - `variety` (optional) - Filter by type: `"agent_skill"`, `"system_utility"`, `"built_in"`
- **Returns**: Grouped list of skills with metadata
- **Status**: Active (always available in registry)

#### **load_skill**
- **Purpose**: Load detailed instructions for a skill and activate its tools
- **Parameters**:
  - `name` (required) - The name of the skill to load
- **Returns**: Full `SKILL.md` content, activation status, and available resources
- **Status**: Active (always available in registry)
- **Effect**: Tools associated with the skill become available for execution

#### **load_skill_resource**
- **Purpose**: Access Level 3 resources (scripts, templates, docs) from a skill
- **Parameters**:
  - `skill_name` (required) - The name of the skill
  - `resource_path` (required) - Relative path (e.g., `scripts/helper.py`)
- **Returns**: File contents
- **Status**: Active (always available in registry)

---

### 3. Agent Control Tools (Delegation & Autonomy)

#### **spawn_subagent**
- **Purpose**: Delegate specialized tasks to dedicated subagents
- **Built-in Subagent Types**:
  - `explore` - Lightweight read-only exploration (haiku model)
  - `plan` - Research and planning (sonnet model)
  - `general` - Full-capability general-purpose agent (sonnet model)
  - `code-reviewer` - Code review and quality analysis
  - `debugger` - Debugging and issue diagnosis
- **Parameters**:
  - `prompt` (required) - Task to delegate
  - `subagent_type` (optional) - Which subagent to use
  - `resume` (optional) - Resume previous work
  - `thoroughness` (optional) - Analysis depth
  - `parent_context` (optional) - Context from parent agent
- **Status**: Available when enabled in `vtcode.toml` (`[subagents] enabled = true`)
- **Custom Agents**: Can be defined in `.vtcode/agents/` (project) or `~/.vtcode/agents/` (user)

---

### 4. Legacy/Internal Tools (Mostly Hidden)

These tools are registered but hidden from LLM by default (legacy support):

- `read_file` - Internal implementation (use `unified_file` action='read')
- `write_file` - Internal implementation (use `unified_file` action='write')
- `edit_file` - Internal implementation (use `unified_file` action='edit')
- `grep_file` - Internal implementation (use `unified_search` action='grep')
- `list_files` - Internal implementation (use `unified_search` action='list')
- `run_pty_cmd` - Internal implementation (use `unified_exec` action='run')
- `send_pty_input` - Internal implementation (use `unified_exec` action='write')
- `read_pty_session` - Internal implementation (use `unified_exec` action='poll')
- `list_pty_sessions` - Internal implementation (use `unified_exec` action='list')
- `close_pty_session` - Internal implementation (use `unified_exec` action='close')

---

## Tool Discovery & Registration

### Tool Registry
The `ToolRegistry` in `vtcode-core/src/tools/registry/mod.rs` manages:
- **Tool Registration**: Each tool is registered with a name, capability level, and executor
- **Alias Mapping**: Aliases are mapped to canonical tool names
- **LLM Visibility**: Tools can be hidden or visible to LLMs
- **Execution Dispatch**: Routes tool calls to correct executors

### Tool Inventory
The `ToolInventory` in `vtcode-core/src/tools/registry/inventory.rs` maintains:
- **Builtin Tools**: Core tools defined in `builtins.rs`
- **Available Tools**: List of tools available in current session
- **Tool Metrics**: Usage tracking and performance metrics
- **Alias Resolution**: Maps aliases to canonical names

### Tool Constants
Tool names are defined in `vtcode-config/src/constants.rs`:
```rust
pub mod tools {
    pub const UNIFIED_SEARCH: &str = "unified_search";
    pub const UNIFIED_EXEC: &str = "unified_exec";
    pub const UNIFIED_FILE: &str = "unified_file";
    pub const LIST_SKILLS: &str = "list_skills";
    pub const LOAD_SKILL: &str = "load_skill";
    pub const LOAD_SKILL_RESOURCE: &str = "load_skill_resource";
    pub const SPAWN_SUBAGENT: &str = "spawn_subagent";
    // ... and aliases
}
```

---

## Tool Execution Flow

```
User Input
    ↓
LLM (with system prompt listing available tools)
    ↓
Tool Call (e.g., "unified_search" with action="grep")
    ↓
Tool Registry.execute() → lookup in inventory
    ↓
Executor (e.g., ToolRegistry::unified_search_executor)
    ↓
Tool Implementation (grep_file, list_files, etc.)
    ↓
Result → back to LLM context
```

---

## System Prompt Integration

The system prompt (in `vtcode-core/src/prompts/system.rs`) instructs agents to:

### Primary Tools (Always Available)
```
**Search**: `unified_search` for all discovery (grep, list)
**Modify**: `unified_file` for all file operations (read, write, edit, patch, delete)
**Execute**: `unified_exec` for all shell commands and code execution
**Discover**: `list_skills` and `load_skill` to find/activate dormant tools
```

### Delegation Pattern
```
- Use `spawn_subagent` for specialized tasks (explore/plan/general/code-reviewer/debugger)
- Relay findings back; decide next steps
```

### Progressive Disclosure Pattern
```
1. **Discovery**: `list_skills` or `list_skills(query="...")` to find available tools
2. **Activation**: `load_skill` to inject tool definitions and instructions
3. **Usage**: Only after activation can you use the tool's specialized capabilities
4. **Resources**: `load_skill_resource` for referenced files (scripts/docs)
```

---

## Tool Policies & Visibility

### Capability Levels (from `vtcode-core/src/config/types.rs`)
- **Basic**: Available tools like `unified_search`, skill management
- **CodeSearch**: Code-related search operations
- **Editing**: File modification operations
- **Bash**: Shell command execution

### LLM Visibility
- **Public Tools**: Visible in `ToolDefinition` sent to LLM
  - `unified_search`, `unified_exec`, `unified_file`
  - `list_skills`, `load_skill`, `load_skill_resource`
  - `spawn_subagent`
  
- **Hidden Tools**: Registered but not exposed to LLM
  - Legacy aliases (`grep_file`, `read_file`, etc.)
  - Internal implementations
  - Tools that are only used by unified tools

---

## MCP Tools (Model Context Protocol)

MCP tools from external servers are also available:
- Prefixed with `mcp_` when exposed to agent
- Listed via `unified_search` action='tools'
- Managed by `McpClient` in `vtcode-core/src/mcp/`
- Require `.mcp.json` configuration

---

## Recent Changes (Skills Tool Fix)

**Status**: ✅ Fixed in current version

The skill tools (`list_skills`, `load_skill`, `load_skill_resource`) were previously not being registered properly in the tool registry due to silent error ignoring. This has been fixed:

- **File**: `src/agent/runloop/unified/session_setup.rs`
- **Change**: Tool registration errors now properly propagated with `.await.context(...)?`
- **Result**: Skill tools now properly available for agent use

See `SKILLS_TOOL_FIX.md` for details.

---

## Tool Usage Best Practices

### Do's ✅
- Use `unified_search` for all code discovery and searching
- Use `unified_file` for all file operations (read, write, edit)
- Use `unified_exec` for all shell commands
- Use `list_skills` to discover dormant tools before using them
- Use `spawn_subagent` for specialized tasks you delegate
- Chain tools efficiently: search → read → modify → validate

### Don'ts ❌
- Don't use legacy aliases directly (e.g., `grep_file`, `read_file`)
- Don't ignore tool registration errors
- Don't execute commands without understanding them
- Don't load skills you won't use (saves tokens)
- Don't assume tools are available without checking `list_skills`

---

## Adding New Tools

To add a new tool:

1. **Define Tool Name**: Add constant to `vtcode-config/src/constants.rs`
   ```rust
   pub const MY_TOOL: &str = "my_tool";
   ```

2. **Create Registration**: Add to `builtin_tool_registrations()` in `vtcode-core/src/tools/registry/builtins.rs`
   ```rust
   ToolRegistration::new(
       tools::MY_TOOL,
       CapabilityLevel::Basic,
       false,
       ToolRegistry::my_tool_executor,
   )
   ```

3. **Implement Executor**: Add handler in `vtcode-core/src/tools/registry/mod.rs`
   ```rust
   pub async fn my_tool_executor(/* ... */) -> Result<Value> {
       // Implementation
   }
   ```

4. **Update System Prompt**: Add guidance to `vtcode-core/src/prompts/system.rs`

5. **Document**: Add entry to this file

---

## Related Documentation

- `docs/ARCHITECTURE.md` - System design overview
- `docs/tools/TOOL_REGISTRY.md` - Detailed tool registry architecture
- `SKILLS_TOOL_FIX.md` - Recent skills tool registration fix
- `AGENTS.md` - Agent workspace guidelines
- `docs/subagents/SUBAGENTS.md` - Subagent system documentation
