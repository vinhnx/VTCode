# VTCode: Command Path Resolution Strategy

**Purpose**: Document how vtcode can better access and validate system commands  
**Audience**: Agent developers, security reviewers  
**Status**: Implementation Plan  

---

## Problem Statement

Currently, vtcode validates commands against static configuration lists, but doesn't:

1. **Resolve** command names to actual filesystem paths
2. **Verify** that allowed commands actually exist on the system
3. **Cache** resolution results for performance
4. **Log** which paths were used for audit purposes
5. **Discover** what tools are available in the user's environment

Example gaps:
- Config says "allow cargo" but doesn't check if cargo exists
- Agent can't tell user "cargo not installed on this system"
- Same command gets re-resolved on every execution
- No audit trail of which /usr/bin/cargo vs /opt/cargo was used

---

## Architecture Overview

### Command Resolution Pipeline

```
┌─────────────────────────────────────────────────────────────────┐
│ User Input: "cargo fmt"                                         │
└────────────────────────┬────────────────────────────────────────┘
                         │
                         ▼
        ┌────────────────────────────────────┐
        │ Check Permission Cache             │
        │ (TTL: 5 minutes)                   │
        │ Key: "cargo fmt"                   │
        └────┬──────────────────────────────┘
             │
             ├─► CACHE HIT → Return cached decision ──┐
             │                                         │
             └─► CACHE MISS ─┐                        │
                             │                        │
                             ▼                        │
        ┌────────────────────────────────────┐       │
        │ Resolve Base Command to Path       │       │
        │ Input: "cargo"                     │       │
        │ Lookup in PATH                     │       │
        │ Output: /Users/user/.cargo/bin/... │       │
        └────┬──────────────────────────────┘       │
             │                                       │
             ▼                                       │
        ┌────────────────────────────────────┐      │
        │ Policy Evaluation                  │      │
        │ • Check deny_list                  │      │
        │ • Check deny_glob                  │      │
        │ • Check allow_list/glob            │      │
        │ Decision: ALLOW / DENY / PROMPT    │      │
        └────┬──────────────────────────────┘      │
             │                                       │
             ▼                                       │
        ┌────────────────────────────────────┐      │
        │ Log to Audit                       │      │
        │ • Command                          │      │
        │ • Resolved Path                    │      │
        │ • Decision (ALLOW/DENY/PROMPT)     │      │
        │ • Reason                           │      │
        │ • Timestamp                        │      │
        └────┬──────────────────────────────┘      │
             │                                       │
             ▼                                       │
        ┌────────────────────────────────────┐      │
        │ Cache Decision                     │      │
        │ Key: "cargo fmt"                   │      │
        │ TTL: 5 minutes                     │      │
        └────┬──────────────────────────────┘      │
             │                                       │
             └──────────┬─────────────────────────┘
                        │
                        ▼
        ┌────────────────────────────────────┐
        │ Execute or Deny                    │
        │ If ALLOW: Run via shell/PTY        │
        │ If DENY: Return error              │
        │ If PROMPT: Ask user                │
        └────────────────────────────────────┘
```

### System PATH Search

When resolving "cargo":

```
$PATH = "/opt/homebrew/bin:/usr/local/bin:/usr/bin:~/.cargo/bin:..."

Search order:
  1. /opt/homebrew/bin/cargo      ✗ (not found)
  2. /usr/local/bin/cargo         ✗ (not found)  
  3. /usr/bin/cargo               ✗ (not found)
  4. ~/.cargo/bin/cargo           ✓ (FOUND!)

Result:
  Resolved: ~/.cargo/bin/cargo
  Path: /Users/vinh/.cargo/bin/cargo
```

---

## Implementation Components

### 1. CommandResolver

**Purpose**: Resolve command names to filesystem paths  
**File**: `vtcode-core/src/tools/command_resolver.rs`

```rust
pub struct CommandResolution {
    pub command: String,
    pub resolved_path: Option<PathBuf>,
    pub found: bool,
    pub search_paths: Vec<PathBuf>,
}

pub struct CommandResolver {
    cache: HashMap<String, CommandResolution>,
}

impl CommandResolver {
    pub fn resolve(&mut self, cmd: &str) -> CommandResolution {
        // Extract base command
        // Check cache
        // Search PATH using `which` crate
        // Cache result
        // Return resolution
    }
}
```

**Key Features**:
- Uses `which` crate to find commands in PATH
- Caches results to avoid repeated PATH searches
- Tracks search paths for debugging
- Handles command not found gracefully

### 2. PermissionAuditLog

**Purpose**: Record all permission decisions for audit trail  
**File**: `vtcode-core/src/audit/permission_log.rs`

```rust
pub struct PermissionEvent {
    pub timestamp: DateTime<Local>,
    pub subject: String,
    pub event_type: PermissionEventType,
    pub decision: PermissionDecision,
    pub reason: String,
    pub resolved_path: Option<PathBuf>,
    pub requested_by: String,
}

pub struct PermissionAuditLog {
    log_path: PathBuf,
    writer: BufWriter<File>,
}

impl PermissionAuditLog {
    pub fn record(&mut self, event: PermissionEvent) -> Result<()> {
        // Serialize to JSON
        // Write to file
        // Flush
    }
}
```

**Storage**:
- Location: `~/.vtcode/audit/permissions-{YYYY-MM-DD}.log`
- Format: JSON Lines (one event per line)
- Retention: 30 days default
- Rotation: Daily

**Event Structure**:
```json
{
  "timestamp": "2025-11-09T14:22:33.123456Z",
  "subject": "cargo fmt",
  "event_type": "CommandExecution",
  "decision": "Allowed",
  "reason": "allow_glob match: 'cargo *'",
  "resolved_path": "/Users/user/.cargo/bin/cargo",
  "requested_by": "CommandPolicyEvaluator"
}
```

### 3. PermissionCache

**Purpose**: Avoid re-evaluating same commands within TTL  
**File**: `vtcode-core/src/tools/command_cache.rs`

```rust
pub struct PermissionCache {
    entries: HashMap<String, CacheEntry>,
    ttl: Duration,  // Default: 5 minutes
}

impl PermissionCache {
    pub fn get(&self, command: &str) -> Option<bool> {
        // Check if cached
        // Verify not expired
        // Return cached decision
    }
    
    pub fn put(&mut self, command: &str, allowed: bool, reason: &str) {
        // Store decision with timestamp
    }
}
```

**Cache Behavior**:
- Key: Command text (e.g., "cargo fmt")
- Value: (allowed: bool, reason: &str, timestamp: Instant)
- TTL: 5 minutes (configurable)
- Expires: Based on timestamp, not LRU

---

## Data Flow Examples

### Example 1: Allowed Command (Cache Hit)

```
User: "cargo fmt"

Session History:
  [14:20:33] cargo fmt → Resolved to ~/.cargo/bin/cargo
             → Policy: ALLOWED (cargo * glob match)
             → Logged to audit
             → Cached: ALLOWED

  [14:22:15] cargo test  
             (5 seconds later, same session)
             
Flow:
  1. Check cache for "cargo test"
  2. Cache MISS (different command)
  3. Resolve: cargo → ~/.cargo/bin/cargo
  4. Policy: ALLOWED (cargo * glob match)
  5. Log event
  6. Cache result
  
  [14:24:20] cargo fmt (again)
  
Flow:
  1. Check cache for "cargo fmt"
  2. Cache HIT (still within 5 min TTL)
  3. Return ALLOWED immediately
  4. No resolution needed
  5. No logging (just counters)
  6. Execute
```

### Example 2: Denied Command (No Caching)

```
User: "rm -rf *.rs"

Flow:
  1. Check cache: MISS
  2. Resolve: rm → /bin/rm
  3. Policy evaluation:
     - Check deny_glob: "rm *" → MATCH!
     - Decision: DENIED
  4. Log event with reason: "deny_glob match: 'rm *'"
  5. Cache result (even deny decisions)
  6. Return error to user
  
Audit Log:
{
  "timestamp": "2025-11-09T14:25:00Z",
  "subject": "rm -rf *.rs",
  "event_type": "CommandExecution",
  "decision": "Denied",
  "reason": "deny_glob match: 'rm *'",
  "resolved_path": "/bin/rm",
  "requested_by": "CommandPolicyEvaluator"
}
```

### Example 3: User-Prompted Command (Manual Allow)

```
User: "docker run -it ubuntu bash"

Flow:
  1. Check cache: MISS
  2. Resolve: docker → /opt/homebrew/bin/docker
  3. Policy: Not in allow_list, not in allow_glob
     Decision: PROMPT (default_policy = "prompt")
  4. Show prompt to user:
     "Allow execution of: docker run -it ubuntu bash?"
     [Allow] [Deny] [Block]
  5. User clicks [Allow]
  6. Log event with reason: "user_approved"
  7. Cache: ALLOWED (for 5 min)
  8. Execute command

Next execution (within 5 min):
  1. Check cache: HIT
  2. Return ALLOWED immediately
  3. Execute without prompting
```

---

## Configuration

### Default Settings

```toml
[permissions]
# Enable command resolution to filesystem paths
resolve_commands = true

# Enable audit logging
audit_enabled = true
audit_directory = "~/.vtcode/audit"

# What to log
log_allowed_commands = true
log_denied_commands = true
log_permission_prompts = true
log_sandbox_events = true

# Cache settings
cache_ttl_seconds = 300        # 5 minutes
cache_enabled = true
show_cache_stats = false       # Show cache hits/misses in debug logs

# Path resolution
resolve_timeout_ms = 100       # Don't wait too long for PATH search
cache_resolved_commands = true # Cache the PATH lookups
```

### Advanced Settings

```toml
# You can override cache TTL per tool
[permissions.tool_cache_ttl]
"cargo *" = 600              # 10 minutes for cargo
"git *" = 300                # 5 minutes for git
"docker *" = 1200            # 20 minutes for docker
```

---

## Audit Log Analysis

### Viewing Logs

```bash
# View today's log
cat ~/.vtcode/audit/permissions-2025-11-09.log

# View last 10 decisions
tail -10 ~/.vtcode/audit/permissions-2025-11-09.log

# Filter for denied commands
grep '"decision":"Denied"' ~/.vtcode/audit/permissions-*.log

# Filter for specific command
grep "cargo fmt" ~/.vtcode/audit/permissions-*.log

# Pretty print with jq
cat ~/.vtcode/audit/permissions-2025-11-09.log | jq .
```

### Sample Audit Log

```json
{"timestamp":"2025-11-09T14:20:33.123456Z","subject":"cargo fmt","event_type":"CommandExecution","decision":"Allowed","reason":"allow_glob match: 'cargo *'","resolved_path":"/Users/user/.cargo/bin/cargo","requested_by":"CommandPolicyEvaluator"}

{"timestamp":"2025-11-09T14:22:15.234567Z","subject":"cargo test","event_type":"CommandExecution","decision":"Allowed","reason":"allow_glob match: 'cargo *'","resolved_path":"/Users/user/.cargo/bin/cargo","requested_by":"CommandPolicyEvaluator"}

{"timestamp":"2025-11-09T14:25:00.345678Z","subject":"rm -rf *.rs","event_type":"CommandExecution","decision":"Denied","reason":"deny_glob match: 'rm *'","resolved_path":"/bin/rm","requested_by":"CommandPolicyEvaluator"}

{"timestamp":"2025-11-09T14:28:45.456789Z","subject":"docker run -it ubuntu bash","event_type":"CommandExecution","decision":"Prompted","reason":"user_approved_override","resolved_path":"/opt/homebrew/bin/docker","requested_by":"CommandPolicyEvaluator"}
```

---

## Performance Characteristics

### Resolution Performance

```
Cold Path Lookup (no cache):
  - Resolve "cargo" → 15-20ms (depends on PATH length)
  - Typical PATH has 10-20 directories
  - Using optimized `which` crate

Cached Lookup:
  - Cache hit → <1ms (HashMap lookup)
  - Typical session: 80%+ cache hit ratio

Example Session (100 commands):
  - First 5 commands: ~100ms total (cold)
  - Next 95 commands: ~5ms total (cache)
  - Total improvement: ~95ms saved per 100 commands
```

### Memory Usage

```
Per-Session Cache:
  - Typical entries: 20-30 unique commands
  - Memory per entry: ~200 bytes
  - Total cache size: ~5KB (negligible)

Audit Log (Daily):
  - ~1000 events per day (typical)
  - ~100 bytes per event (JSON)
  - Total: ~100KB per day
  - 30-day retention: ~3MB
```

---

## Security Considerations

### What This Doesn't Change

✓ Deny list still blocks dangerous commands  
✓ Sandbox still restricts path access  
✓ WorkspaceGuardPolicy still prevents traversal  
✓ Tool policies still gate execution  
✓ No new vectors for privilege escalation  

### What This Improves

✓ Visibility: Audit log shows what happened  
✓ Validation: Confirm command exists before allowing  
✓ Accountability: Track who/what made decisions  
✓ Debugging: Understand why permission was granted  

### Potential Concerns

| Concern | Mitigation | Impact |
|---------|-----------|--------|
| Audit log disk space | Auto-rotation + compression | Low |
| Cache age issues | Explicit TTL + cleanup | Low |
| Symlink attacks | PATH search uses `which` safely | Low |
| Timing attacks | Cache TTL is constant, not data-dependent | Low |

---

## Integration Points

### 1. Tool Registration

When a tool is registered, it gets access to resolver:

```rust
let resolver = CommandResolver::new();
tool_registry.set_command_resolver(resolver);
```

### 2. Policy Evaluation

Policy evaluator uses resolver when checking commands:

```rust
pub fn allows_text(&mut self, cmd: &str) -> (bool, Option<PathBuf>) {
    let base_cmd = cmd.split_whitespace().next().unwrap_or(cmd);
    
    // Resolve to path
    let resolution = self.resolver.resolve(base_cmd);
    
    // Check policy
    let allowed = /* evaluation logic */;
    
    (allowed, resolution.resolved_path)
}
```

### 3. Agent Session

Session owns the audit log and passes decisions to it:

```rust
pub struct AgentSession {
    audit_log: PermissionAuditLog,
    permission_cache: PermissionCache,
    command_resolver: CommandResolver,
}

impl AgentSession {
    async fn execute_command(&mut self, cmd: &str) -> Result<()> {
        // ... permission checks ...
        
        // Log the decision
        self.audit_log.log_command_decision(
            cmd,
            decision,
            &reason,
            resolved_path,
        )?;
        
        // Execute
    }
}
```

---

## Troubleshooting

### Command Resolution Not Working

**Symptom**: Allowed command says "not found"

**Diagnosis**:
```bash
# Check if PATH is set
echo $PATH

# Check if command exists manually
which cargo

# Check resolver cache
# Look in ~/.vtcode/debug.log for resolver output
```

**Fix**:
1. Ensure PATH includes command location
2. Verify command is executable: `ls -l ~/.cargo/bin/cargo`
3. Clear cache: `/permissions clear-cache`

### Audit Logs Growing Too Large

**Symptom**: `~/.vtcode/audit/` taking up space

**Diagnosis**:
```bash
du -sh ~/.vtcode/audit/
ls -la ~/.vtcode/audit/ | wc -l
```

**Fix**:
1. Enable log rotation in config: `rotate_after_days = 7`
2. Enable compression: `compress_after_days = 7`
3. Reduce retention: `retention_days = 14`

### Cache Not Improving Performance

**Symptom**: Slow command execution despite cache

**Diagnosis**:
```bash
# Check cache stats (if enabled)
# Look for cache hit ratio in debug logs
```

**Fix**:
1. Check cache TTL is reasonable (5 min default)
2. Verify cache is actually enabled in config
3. Clear cache and retry: `/permissions clear-cache`

---

## Future Enhancements

### Phase 2

- [ ] Environment profile discovery (what's installed?)
- [ ] Command availability status in agent prompts
- [ ] Bulk path whitelisting (e.g., "allow all ~/.local/bin")
- [ ] Permission summary on session end

### Phase 3

- [ ] Hook-based permission chains (A allows → check B)
- [ ] Time-based permissions (allow 9-5 only)
- [ ] Tool-specific path allowlists
- [ ] Network-based permission decisions (ask central server)

---

## Checklist for Implementers

- [ ] Create CommandResolver module and tests
- [ ] Create PermissionAuditLog module and tests  
- [ ] Create PermissionCache module and tests
- [ ] Integrate into CommandPolicyEvaluator
- [ ] Wire up to AgentSession initialization
- [ ] Add config options to vtcode.toml
- [ ] Test end-to-end command execution
- [ ] Verify audit logs are created
- [ ] Verify cache reduces redundancy
- [ ] Run full test suite: `cargo test`
- [ ] Run linter: `cargo clippy`
- [ ] Format code: `cargo fmt`
- [ ] Update AGENTS.md documentation

---

## References

- [which crate](https://docs.rs/which/latest/which/)
- [std::path::Path](https://doc.rust-lang.org/std/path/struct.Path.html)
- [Filesystem Paths on Unix](https://en.wikipedia.org/wiki/PATH_(variable))
- [Audit Logging Best Practices](https://cheatsheetseries.owasp.org/cheatsheets/Logging_Cheat_Sheet.html)
