system prompt https://github.com/Piebald-AI/claude-code-system-prompts/blob/main/system-prompts/system-prompt-main-system-prompt.md

--

https://github.com/Piebald-AI/claude-code-system-prompts/

--



--

Add LSP (Language Server Protocol) tool for code intelligence features like go-to-definition, find references, and hover documentation


--

revivew prompt caching strategy for better performance and lower latency

--

remove this for simplicity

Adaptive context trimming near budget thresholds
• Semantic compression and pruning
• Token budget enforcement

--


analyze and improve 

Now let me summarize the core agent loop analysis:\n\n## Core Agent Loop Analysis\n\nBased on my analysis of the VT Code codebase, here's a comprehensive overview of the core agent loop architecture:\n\n### Entry Points\n\n**Primary Entry**: `src/agent/agents.rs::run_single_agent_loop()`\n- Loads configuration from workspace\n- Creates provider client and tool registry\n- Initializes session components\n- Delegates to unified turn loop\n\n**Main Loop**: `src/agent/runloop/unified/turn/run_loop.rs`\n- ~3,218 lines implementing the core agent loop\n- Located in `run_single_agent_loop_unified()` function\n\n### Loop Structure\n\nThe agent loop is a nested structure:\n\n```\nsession_loop (outer)\n  └─ turn_loop (middle)\n      └─ tool_execution_loop (inner)\n```\n\n**Session Loop** (`session_loop.rs`):\n- Handles session lifecycle\n- Manages TUI initialization and cleanup\n- Supports session resumption and forking\n- Handles slash commands\n- Manages terminal state restoration\n\n**Turn Loop** (`run_loop.rs:1974`):\n- Main processing loop labeled `'outer`\n- Executes individual conversation turns\n- Manages context trimming and budget enforcement\n- Handles tool calls and responses\n- Implements self-review mechanism\n\n**Tool Execution Loop** (within turn processing):\n- Processes multiple tool calls per turn\n- Supports parallel tool execution\n- Implements tool caching\n- Handles tool permissions and approvals\n\n### Key Components\n\n**Context Management** (`context_manager.rs`):\n- Adaptive context trimming near budget thresholds\n- Semantic compression and pruning\n- Token budget enforcement\n- System prompt building with incremental updates\n\n**Tool Pipeline** (`tool_pipeline.rs`):\n- Permission checking and HITL (Human-in-the-Loop) approval\n- Tool result caching for read-only tools\n- Timeout handling with cancellation\n- Modified files tracking\n\n**Loop Detection** (`loop_detection.rs`):\n- Tracks repeated tool call signatures\n- Configurable threshold for loop detection\n- Selective signature reset capability\n- Turn balancer to prevent low-signal churn\n\n**Decision Tracking** (`decision_tracker.rs`):\n- Records all agent decisions\n- Tracks outcomes (success/failure)\n- Generates transparency reports\n- Confidence scoring\n\n**State Management** (`state.rs`):\n- `CtrlCState`: Handles cancellation and exit signals\n- `SessionStats`: Tracks tool usage across session\n\n### Core Flow\n\n1. **Session Setup** (`session_setup.rs`):\n   - Load configuration and initialize provider\n   - Build tool definitions with mode-based declarations\n   - Create context manager with trim configuration\n   - Initialize MCP (Model Context Protocol) support\n\n2. **Turn Processing**:\n   - Build system prompt with incremental updates\n   - Execute LLM request with streaming\n   - Parse response for tool calls and final text\n   - Handle tool calls through permission pipeline\n   - Cache tool results for read-only operations\n   - Update decision ledger\n\n3. **Context Management**:\n   - Adaptive trimming at budget thresholds\n   - Turn balancer triggers compaction on repeated calls\n   - Semantic pruning based on message importance\n   - Token budget enforcement\n\n4. **Error Handling**:\n   - Tool failure handling with enhanced error messages\n   - Timeout detection and cancellation\n   - Loop detection and prevention\n   - Error recovery strategies\n\n### Safety Mechanisms\n\n1. **Loop Prevention**:\n   - Repeated tool call tracking\n   - Turn balancer limits low-signal churn\n   - Configurable repeat thresholds\n\n2. **Human-in-the-Loop**:\n   - Tool permission checks\n   - Destructive operation confirmation\n   - Git diff integration for change review\n\n3. **Resource Management**:\n   - Token budget enforcement\n   - Context window limits\n   - Tool execution timeouts\n   - Ctrl+C signal handling\n\n4. **Recovery**:\n   - Error recovery manager with strategies\n   - Snapshot/checkpoint support\n   - Session resumption capability\n\n### Key Patterns\n\n**Mode-Based Execution**: 77% complexity reduction through mode-based tool selection and execution\n\n**Trait-Based Tools**: Single source of truth for content search and file operations\n\n**Incremental System Prompt**: Dynamic prompt building based on conversation state\n\n**Decision Ledger**: Comprehensive tracking of all agent actions and outcomes\n\n**Semantic Pruning**: Context compression based on message importance and age\n\nThe loop is designed to be robust, safe, and efficient while maintaining transparency through comprehensive decision tracking and reportingq
