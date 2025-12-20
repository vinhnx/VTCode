# Vibe Coding (Experimental)

**Status:** Experimental, disabled by default  
**Stability:** Under active development (behavior may change)

## Overview

Vibe Coding is an experimental entity-aware context enrichment system that tracks variable references, workspace state changes, and conversation memory to provide more contextually-aware LLM responses.

## Features

### 1. Entity Resolution
- Tracks variable and function references across the codebase
- Builds an entity index of important symbols
- Allows pronouns like "it" and "that" to be resolved to specific entities
- Reduces ambiguity in code-related queries

### 2. Workspace State Tracking
- Monitors recently modified files
- Tracks current working directory and project context
- Provides location awareness to the LLM
- Helps with relative references ("the file we just edited")

### 3. Conversation Memory
- Retains context across multiple turns
- Remembers previous decisions and changes
- Enables multi-turn workflows without context loss
- Configurable memory window

### 4. Pronoun Resolution
- Understands pronouns in prompts ("Fix that function", "Update it")
- Resolves pronouns to their actual code references
- Reduces need for explicit entity naming

### 5. Relative Value Inference
- Estimates importance of different code elements
- Prioritizes frequently-used functions/variables
- Helps LLM focus on high-value context

## Enabling Vibe Coding

To enable, set in `vtcode.toml`:

```toml
[agent.vibe_coding]
enabled = true
```

This activates all sub-features with their default settings.

## Configuration Options

```toml
[agent.vibe_coding]
# Master enable/disable switch
enabled = false

# Minimum prompt requirements (prevents false positives)
min_prompt_length = 5           # Characters
min_prompt_words = 2            # Words

# Entity Resolution
enable_entity_resolution = true
entity_index_cache = ".vtcode/entity_index.json"
max_entity_matches = 5          # Results per query

# Workspace State
track_workspace_state = true
max_recent_files = 20           # Files to track

# Conversation Memory
track_value_history = true      # Track change history
enable_conversation_memory = true
max_memory_turns = 50           # Turns to retain

# Pronoun & Context
enable_pronoun_resolution = true
enable_proactive_context = true
max_context_files = 3           # Files to include
max_context_snippets_per_file = 20

# Search
max_search_results = 5

# Advanced
enable_relative_value_inference = true
```

## Performance Implications

### Memory Usage
- Entity index caching: +5-20 MB (depending on codebase size)
- Conversation history: +1-5 MB (50 turns)
- Total: ~10-30 MB additional memory

### Processing Time
- Entity indexing: +500ms-1s on startup
- Per-query entity resolution: +100-300ms
- Workspace state tracking: +50-100ms per query
- Overall impact: 10-15% slower responses

### Recommended For
- Large projects (100k+ lines of code)
- Long coding sessions (50+ turns)
- Complex codebases with many cross-references
- Workflows involving pronouns ("fix it", "update that")

### Not Recommended For
- Embedded/IoT systems (memory constraints)
- CI/CD environments (overhead not needed)
- One-shot queries (context not needed)
- Resource-constrained environments

## Known Limitations

### Entity Resolution
- Can produce false positives in ambiguous code
- May match irrelevant entities with similar names
- Doesn't understand semantic relationships (only textual)

### Pronoun Resolution
- Works best in English
- May fail with complex nested pronouns
- Requires explicit context hints for clarity

### Workspace Tracking
- Doesn't track files in .gitignore
- May miss changes outside current session
- Best effort only (not guaranteed accurate)

### Memory
- Conversation memory is session-scoped only
- Lost when session ends (not persisted)
- Max 50 turns may be insufficient for very long sessions

## Examples

### Example 1: Using Entity Resolution

Without Vibe Coding:
```
User: Fix the login function
VT Code: Which login function? (user_login, admin_login, oauth_login?)
```

With Vibe Coding:
```
User: Fix the login function
VT Code: [Resolves to user_login from entity index] Fixing user_login...
```

### Example 2: Using Pronoun Resolution

Without Vibe Coding:
```
User: Update the cache manager. Also, optimize it.
VT Code: Optimize what? (unclear referent)
```

With Vibe Coding:
```
User: Update the cache manager. Also, optimize it.
VT Code: [Resolves "it" to cache_manager] Optimizing cache manager...
```

### Example 3: Using Workspace State

Without Vibe Coding:
```
User: What did we change?
VT Code: (Generic response, doesn't know context)
```

With Vibe Coding:
```
User: What did we change?
VT Code: [Knows we edited auth.rs and cache.rs] 
You modified auth.rs (login logic) and cache.rs (TTL handling)...
```

## Disabling Individual Features

If Vibe Coding overall is enabled but you want to disable specific features:

```toml
[agent.vibe_coding]
enabled = true
enable_entity_resolution = false        # Disable entity tracking
enable_pronoun_resolution = false       # Disable pronoun resolution
track_workspace_state = false           # Disable workspace tracking
enable_conversation_memory = false      # Disable memory across turns
```

## Cache Management

The entity index cache is stored at `.vtcode/entity_index.json`:

**Clearing the cache:**
```bash
rm .vtcode/entity_index.json
```

The index will be rebuilt on next session start.

**Cache size control:**
```toml
[agent.vibe_coding]
max_entity_matches = 3          # Reduce to keep fewer entities
max_recent_files = 10           # Reduce to track fewer files
```

## Testing with Vibe Coding

To verify Vibe Coding is working:

```bash
# Enable in vtcode.toml
enabled = true

# Start a session and try:
# 1. Entity resolution: Ask about specific functions
#    "What does the main function do?"
# 2. Pronoun resolution: Use pronouns
#    "Update the cache. Make it thread-safe."
# 3. Workspace awareness: Ask about recent changes
#    "What files did we edit?"
```

Watch for:
- Faster entity recognition (no need to specify which one)
- Correct pronoun resolution (references matched correctly)
- Awareness of recent files and changes

## Troubleshooting

**Entity index not updating:**
- Delete `.vtcode/entity_index.json` and restart
- Check file permissions in workspace
- Verify `enable_entity_resolution = true`

**Slow responses with Vibe Coding enabled:**
- Reduce `max_entity_matches` (fewer results)
- Reduce `max_recent_files` (fewer files to track)
- Increase `min_prompt_length` (skip small queries)
- Disable workspace tracking if not needed

**Incorrect pronoun resolution:**
- Provide more context in prompts
- Use explicit entity names instead of pronouns
- Consider disabling `enable_pronoun_resolution`

**High memory usage:**
- Reduce `max_memory_turns` (fewer turns retained)
- Disable `track_value_history`
- Clear entity index periodically

## Advanced: Custom Entity Index

For advanced users, you can manually edit `.vtcode/entity_index.json`:

```json
{
  "entities": [
    {
      "name": "main",
      "type": "function",
      "file": "src/main.rs",
      "line": 10,
      "frequency": 15
    }
  ]
}
```

**Warning:** Manual edits may cause indexing inconsistencies. Use with caution.

## Future Plans

Vibe Coding development roadmap:
- [ ] Semantic entity relationships (understanding inheritance, trait impl)
- [ ] Multi-session memory (persistent conversation history)
- [ ] Language-specific entity parsing (better than text matching)
- [ ] Cross-file reference tracking
- [ ] Performance optimizations (reduce memory overhead)

## Feedback

If you encounter issues or have suggestions for Vibe Coding:
1. Note the specific behavior that failed
2. Check if it reproduces consistently
3. Report with context: codebase size, number of entities, prompt
4. Include `.vtcode/entity_index.json` if applicable
