# VTCode: PATH and Permission Handling Analysis & Improvements

**Date**: November 9, 2025  
**Status**: Current Review & Enhancement Plan  
**Last Updated**: Comprehensive Analysis

---

## Executive Summary

VTCode has a multi-layered permission and path security system, but there are improvements to be made in:

1. **PATH Command Resolution**: Currently relies on config-based allow/deny lists; doesn't resolve command paths dynamically
2. **Permission Caching**: No caching of policy decisions, causing redundant evaluations
3. **Path Whitelisting**: Lacks a centralized registry of system paths that tools can access
4. **Documentation**: Permission system is complex but underdocumented
5. **Audit Trail**: Limited logging of what permissions were granted and why

---

## Current Architecture

### 1. Multi-Layer Permission System

#### Layer 1: Tool-Level Policies (`vtcode-core/src/tools/registry/policy.rs`)
- **Enum**: `ToolPolicy { Allow, Prompt, Deny }`
- **Scope**: Individual tools (read_file, run_terminal_cmd, etc.)
- **Config Location**: `[tools.policies]` in vtcode.toml
- **Default**: `tools.default_policy = "prompt"`

**Example Config**:
```toml
[tools.policies]
read_file = "allow"
write_file = "prompt"
run_terminal_cmd = "prompt"
```

#### Layer 2: Command Policy (`vtcode-core/src/tools/command_policy.rs`)
- **Type**: `CommandPolicyEvaluator`
- **Supports**: Prefix matching, regex patterns, glob patterns
- **Precedence**: Deny > Allow (deny takes priority)
- **Config Location**: `[commands]` in vtcode.toml

**Config Structure**:
```toml
[commands]
allow_list = ["ls", "pwd", "git *", "cargo *", ...]
deny_list = ["rm -rf /", "sudo *", "reboot", ...]
allow_glob = ["git *", "cargo *", "python *", ...]
deny_glob = ["rm *", "sudo *", "chmod *", ...]
allow_regex = [r"^git \w+", ...]
deny_regex = [r"rm\s+(-rf|--force)", ...]
```

**Default Allow Commands** (99 total):
- Basic: ls, pwd, cat, grep, find, echo, date, etc.
- Build Tools: cargo, rustc, make, cmake, ninja
- Version Control: git (status, diff, log, show, branch, remote)
- Package Managers: npm, pip, yarn, pnpm, bun, npx
- Languages: python, node, go, gcc, g++, javac, gradle
- Containers: docker, docker-compose

**Default Deny Commands** (79 total):
- System: reboot, shutdown, halt, poweroff, systemctl
- Privileges: sudo, su, chmod, chown, chgrp
- Destructive: rm -rf /, mkfs, fdisk, dd
- Security: eval, source ~/.bashrc, cat /etc/shadow
- Shell Access: nohup bash -i, exec bash -i

#### Layer 3: Sandbox & Path Restrictions (`src/agent/runloop/sandbox.rs`)
- **Type**: `SandboxEnvironment` + `SandboxProfile`
- **Scope**: Filesystem and network access for bash commands
- **States**: Enabled/Disabled
- **Features**:
  - Filesystem allowlist (`allow_path`)
  - Network allowlist (`allow_domain`)
  - Protected paths (cannot remove)
  - Event logging

**Slash Commands for Management**:
```
/sandbox                    # Toggle sandboxing
/sandbox status            # Show current config
/sandbox enable/disable    # Explicit control
/sandbox allow-domain <d>  # Allow network access
/sandbox allow-path <p>    # Allow filesystem access
/sandbox remove-path <p>   # Revoke filesystem access
/sandbox list-paths        # Show allowed paths
```

#### Layer 4: Workspace Guard (`vtcode-bash-runner/src/policy.rs`)
- **Type**: `WorkspaceGuardPolicy`
- **Validates**: All executed paths stay within workspace root
- **Features**: Optional command category filtering
- **Prevents**: Path traversal attacks (../../etc/passwd)

#### Layer 5: Lifecycle Hooks (`src/hooks/lifecycle.rs`)
- **Type**: `PreToolHookDecision { Allow, Deny, Ask, Continue }`
- **When**: Pre-tool execution hooks
- **Capability**: Custom validation before tool use
- **Example**: Security checks, licensing validation

---

## Current Path & Command Resolution Flow

### Command Execution Flow:
```
User Input (bash command)
    ↓
CommandPolicyEvaluator::allows(cmd)
    ├─ Check deny_list → if match: DENY
    ├─ Check deny_regex → if match: DENY
    ├─ Check deny_glob → if match: DENY
    ├─ If no allow rules: ALLOW (default allow)
    └─ Check allow_list/regex/glob → ALLOW or DENY
    ↓
[If allowed] Run command via shell/PTY
    ├─ Shell resolves command in PATH
    ├─ WorkspaceGuardPolicy validates paths
    └─ SandboxProfile applies restrictions
```

### Key Issue: Static Path Configuration

**Current Limitation**: Commands are validated against static allow/deny lists in config:

```rust
// From CommandPolicyEvaluator
pub fn allows_text(&self, command_text: &str) -> bool {
    let cmd = command_text.trim();
    
    // Only matches against configured patterns
    self.matches_prefix(cmd, &self.allow_prefixes)
        || Self::matches_any(&self.allow_regexes, cmd)
        || Self::matches_any(&self.allow_glob_regexes, cmd)
}
```

**What's Missing**:
1. No resolution of command paths from system PATH
2. No caching of policy decisions
3. No centralized registry of "known safe" paths
4. No validation that allowed commands actually exist
5. No audit trail of permission grants

---

## System Paths That Need Management

### Critical Command Locations (macOS/Linux):

```
System Tools:
  /usr/bin          - Core utilities (ls, cat, grep, echo, etc.)
  /usr/local/bin    - User-installed tools (homebrew, manual installs)
  /bin              - Essential utilities (bash, sh, etc.)
  /sbin             - System binaries (ifconfig, etc.)

Development Tools:
  ~/.cargo/bin      - Rust tools (cargo, rustc)
  ~/.local/bin      - Python (pipx, poetry), other languages
  ~/.bun/bin        - Bun package manager
  ~/.deno/bin       - Deno runtime
  /usr/local/go/bin - Go installations
  
Package Manager Paths:
  /opt/homebrew/bin - macOS Homebrew (ARM)
  /opt/homebrew/opt/*/bin - Homebrew casks
  /snap/bin         - Snap packages

Node/NPM Paths:
  ~/.npm/_npx/      - Cached npx commands
  ~/.nvm/versions   - Node version manager
  
Docker/Containers:
  /var/run/docker.sock - Docker socket

Git/Version Control:
  ~/.ssh            - SSH keys
```

---

## Identified Problems & Improvements

### Problem 1: No PATH Resolution

**Current State**: Commands are pattern-matched, but don't check if they exist.

**Example**:
```toml
allow_glob = ["cargo *"]  # Allows "cargo", but what's the full path?
```

**Issue**: If cargo is in multiple locations, no way to know which one runs.

**Improvement**:
```rust
pub struct CommandResolution {
    command: String,
    resolved_path: Option<PathBuf>,  // Full path if found in PATH
    found_in_system: bool,           // Actually exists on system
    allow_all_variants: bool,        // Allow any variant of command
}

pub async fn resolve_command(cmd: &str) -> CommandResolution {
    // Try to find command in system PATH
    if let Ok(path) = which::which(cmd) {
        CommandResolution {
            command: cmd.to_string(),
            resolved_path: Some(path),
            found_in_system: true,
            allow_all_variants: true,
        }
    } else {
        // Not found, but pattern might still allow it
        CommandResolution {
            command: cmd.to_string(),
            resolved_path: None,
            found_in_system: false,
            allow_all_variants: false,
        }
    }
}
```

### Problem 2: No Permission Caching

**Current State**: Every command execution re-evaluates the full policy tree.

**Issue**: Inefficient for repeated commands; no audit trail.

**Improvement**:
```rust
pub struct PermissionCache {
    decisions: Arc<Mutex<HashMap<String, CachedDecision>>>,
    ttl: Duration,
}

pub struct CachedDecision {
    command: String,
    decision: PermissionDecision,
    granted_at: SystemTime,
    granted_by: String,  // "config", "user_prompt", "hook"
    context: Option<String>,  // Why was it granted?
}

impl PermissionCache {
    pub fn get(&self, command: &str) -> Option<CachedDecision> {
        // Check if cached and not expired
    }
    
    pub fn record(&self, decision: CachedDecision) {
        // Store with TTL
    }
    
    pub fn audit_log(&self) -> Vec<CachedDecision> {
        // All decisions made in this session
    }
}
```

### Problem 3: No Centralized Path Whitelist

**Current State**: Paths are validated but not registered anywhere.

**Issue**: 
- Hard to audit what paths tools can access
- No way to bulk-allow related paths
- Each tool has to validate paths independently

**Improvement**:
```rust
pub struct SystemPathRegistry {
    whitelist: Vec<PathMetadata>,
    deny_patterns: Vec<PathPattern>,
    protected_paths: Vec<PathBuf>,
}

pub struct PathMetadata {
    path: PathBuf,
    category: PathCategory,  // Development, System, User, Temporary
    tools_allowed: Vec<String>,  // Which tools can access?
    recursive: bool,         // Allow subdirectories?
    reason: String,          // Why whitelisted?
}

pub enum PathCategory {
    SystemBinary,       // /usr/bin, /bin
    Development,        // ~/.cargo/bin, ~/.local/bin
    ProjectWorkspace,   // Workspace root
    Temporary,          // /tmp, $TMPDIR
    VCS,               // .git, .hg
    Hidden,            // .env, secrets
}

impl SystemPathRegistry {
    pub fn is_path_allowed(&self, path: &Path, tool: &str) -> bool {
        // Check whitelist + deny patterns + tool access
    }
    
    pub fn get_allowed_paths(&self, tool: &str) -> Vec<PathBuf> {
        // Return all paths accessible to a tool
    }
}
```

### Problem 4: Insufficient Logging

**Current State**: Permissions are checked but not logged.

**Issue**: Can't audit what happened or debug permission issues.

**Improvement**:
```rust
pub enum PermissionEvent {
    CommandAllowed {
        command: String,
        reason: AllowReason,
        timestamp: SystemTime,
    },
    CommandDenied {
        command: String,
        reason: DenyReason,
        timestamp: SystemTime,
    },
    PermissionPrompted {
        tool: String,
        decision: UserDecision,
        timestamp: SystemTime,
    },
    PathAccessGranted {
        path: PathBuf,
        tool: String,
        timestamp: SystemTime,
    },
    SandboxViolation {
        attempt: String,
        tool: String,
        timestamp: SystemTime,
    },
}

pub enum AllowReason {
    ConfigureAllowList,
    ConfigureAllowGlob,
    ConfigureAllowRegex,
    DefaultAllow,
}

pub enum DenyReason {
    ConfigureDenyList,
    ConfigureDenyGlob,
    ConfigureDenyRegex,
    SandboxViolation,
    PathTraversal,
}

pub struct PermissionAuditLog {
    events: Vec<PermissionEvent>,
    session_start: SystemTime,
    written_to_file: PathBuf,
}

impl PermissionAuditLog {
    pub fn record(&mut self, event: PermissionEvent) {
        // Log with timestamp
        // Write to ~/.vtcode/audit/permissions.log
    }
    
    pub fn summary(&self) -> PermissionSummary {
        // Commands allowed, denied, prompted
    }
}
```

### Problem 5: Tool Discovery & Documentation

**Current State**: Tools don't know what commands/paths are available.

**Issue**: Agent can't inform user about available tools for their environment.

**Improvement**:
```rust
pub struct EnvironmentProfile {
    available_commands: HashMap<String, CommandInfo>,
    available_paths: Vec<PathInfo>,
    unsafe_commands: Vec<String>,
    environment_vars: HashMap<String, String>,
}

pub struct CommandInfo {
    name: String,
    path: PathBuf,
    version: Option<String>,
    safety_level: SafetyLevel,  // Safe, Warning, Dangerous
}

impl EnvironmentProfile {
    pub async fn scan() -> Result<Self> {
        // Discover what's actually available on system
        // Check for: cargo, python, node, npm, git, etc.
        // Get versions for context window
    }
    
    pub fn describe_to_agent(&self) -> String {
        // Generate human-readable summary for LLM
        // "Available: cargo (1.75.0), python (3.11.6), node (18.17.0)"
    }
}
```

---

## Recommended Improvements (Priority Order)

### P0: Immediate (High Impact, Low Risk)

1. **Add Command Resolution**
   - Resolve commands to actual paths using `which`
   - Log resolved paths in permission decisions
   - File: `vtcode-core/src/tools/command_resolver.rs`

2. **Implement Permission Audit Log**
   - Record all permission decisions to `~/.vtcode/audit/`
   - Include reason, timestamp, user input
   - Implement `/audit` slash command to view logs
   - File: `vtcode-core/src/audit/permission_log.rs`

3. **Add Cache Layer to CommandPolicyEvaluator**
   - Cache allow/deny decisions for 5 minutes
   - Track "granted by config" vs "granted by user prompt"
   - File: `vtcode-core/src/tools/command_cache.rs`

### P1: Short-term (Medium Impact, Medium Risk)

4. **Create SystemPathRegistry**
   - Define allowed paths categorized by purpose
   - Integrate with sandbox and workspace guard
   - Support bulk allow rules (e.g., "allow all ~/.local/bin")
   - File: `vtcode-core/src/paths/registry.rs`

5. **Enhance Sandbox Logging**
   - Log all path access attempts
   - Log network access denials
   - Provide `/sandbox audit` command
   - File: Enhanced `src/agent/runloop/sandbox.rs`

6. **Environment Profile Discovery**
   - Scan PATH on startup
   - Detect installed tools (cargo, python, node, etc.)
   - Include availability summary in onboarding
   - File: `vtcode-core/src/environment/profile.rs`

### P2: Long-term (Lower Priority, Higher Effort)

7. **Implement Lifecycle Hook Audit**
   - Track what hooks executed and why
   - Support hook-level allow/deny decisions
   - File: Enhanced `src/hooks/lifecycle.rs`

8. **Multi-tenant Session Isolation**
   - Separate permission contexts per session
   - Support session-specific allow/deny overrides
   - File: `vtcode-core/src/sessions/permission_context.rs`

9. **Permission Request Batching**
   - Group multiple permission prompts into one UI
   - Support "Allow all" for this session
   - File: `src/agent/runloop/permission_batch.rs`

---

## Implementation Plan

### Phase 1: Command Resolution (2-3 hours)

**File**: `vtcode-core/src/tools/command_resolver.rs`

```rust
use which::which;
use std::path::PathBuf;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub struct CommandResolution {
    pub command: String,
    pub resolved_path: Option<PathBuf>,
    pub found: bool,
}

pub struct CommandResolver {
    cache: HashMap<String, CommandResolution>,
}

impl CommandResolver {
    pub async fn resolve(&mut self, cmd: &str) -> CommandResolution {
        // Get base command (e.g., "cargo" from "cargo fmt")
        let base = cmd.split_whitespace().next().unwrap_or(cmd);
        
        if let Some(cached) = self.cache.get(base) {
            return cached.clone();
        }
        
        let resolution = CommandResolution {
            command: base.to_string(),
            resolved_path: which(base).ok(),
            found: which(base).is_ok(),
        };
        
        self.cache.insert(base.to_string(), resolution.clone());
        resolution
    }
}
```

**Integration Points**:
- Modify `CommandPolicyEvaluator::allows_text()` to log resolved path
- Pass to `PermissionAuditLog` for recording
- Return in error messages when command not found

### Phase 2: Audit Logging (1-2 hours)

**File**: `vtcode-core/src/audit/permission_log.rs`

```rust
use chrono::DateTime;
use std::fs::OpenOptions;
use std::path::PathBuf;

pub struct PermissionAuditLog {
    path: PathBuf,
    writer: BufWriter<File>,
}

impl PermissionAuditLog {
    pub fn record(&mut self, event: PermissionEvent) -> Result<()> {
        let json = serde_json::to_string(&event)?;
        writeln!(self.writer, "{}", json)?;
        self.writer.flush()?;
        Ok(())
    }
}
```

**Files Generated**:
- `~/.vtcode/audit/permissions-{date}.log` (one per session)
- Rotate daily
- Compact after 7 days

### Phase 3: Permission Cache (1 hour)

**Modify**: `vtcode-core/src/tools/command_policy.rs`

Add memoization layer:

```rust
impl CommandPolicyEvaluator {
    cache: HashMap<String, bool>,
    
    pub fn allows_text(&self, command_text: &str) -> bool {
        if let Some(&cached) = self.cache.get(command_text) {
            return cached;
        }
        // Evaluate...
        // Cache result...
    }
    
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }
}
```

---

## Configuration Examples

### Enhanced vtcode.toml with Path Registry

```toml
# Path whitelisting and access control
[paths.whitelist]
# Development tools directories
development_paths = [
    "~/.cargo/bin",        # Rust tools
    "~/.local/bin",        # Python, pip, pipx
    "~/.bun/bin",         # Bun runtime
    "~/.deno/bin",        # Deno runtime
    "/opt/homebrew/bin",  # macOS ARM Homebrew
]

# Can these paths be recursively accessed?
allow_recursive = [
    "~/.local/lib",       # Python packages
    "~/.cargo/registry",  # Cargo cache
]

# Paths that cannot be removed from allowed list
protected_paths = [
    ".",  # Workspace root
]

# Tools that bypass normal path validation
trusted_tools = [
    "list_files",
    "read_file",
    "write_file",
]

# Audit configuration
[audit]
enabled = true
log_directory = "~/.vtcode/audit"
max_log_files = 30  # Keep 30 days of logs
permission_events = true
path_access_events = true
sandbox_events = true
hook_execution = true
```

### Example: Allow Custom Tools

```toml
[commands]
# Existing config...

# Allow custom scripts in project
allow_glob = [
    # Existing...
    "./.vtcode/scripts/*",
    "./scripts/*",
    "custom-tool",
]

# But block dangerous patterns even in custom scripts
deny_glob = [
    # Existing...
    "./.env",      # Don't accidentally cat .env
    "*/secrets*",  # Block secret files
]
```

---

## Testing Strategy

### Unit Tests

```rust
#[cfg(test)]
mod permission_tests {
    use super::*;
    
    #[test]
    fn test_command_resolution_with_cache() {
        // Verify cache works
        // Verify resolved paths are correct
    }
    
    #[test]
    fn test_audit_log_record() {
        // Verify events are written
        // Verify JSON serialization
    }
    
    #[test]
    fn test_path_registry_categorization() {
        // Verify paths are categorized
        // Verify access control works
    }
}
```

### Integration Tests

```rust
#[cfg(test)]
mod integration_tests {
    #[tokio::test]
    async fn test_full_permission_flow() {
        // User runs command
        // Permission evaluated
        // Logged to audit
        // Cached for next time
        // Verify audit log
    }
}
```

---

## Configuration Changes Needed

### vtcode.toml additions:

```toml
# Permission and audit configuration
[permissions]
# Enable caching of policy decisions (seconds)
cache_ttl = 300

# Enable audit logging
audit_enabled = true
audit_directory = "~/.vtcode/audit"

# What to log
log_allowed_commands = true
log_denied_commands = true
log_prompts = true
log_path_access = true

# Permission summary on session end
show_permission_summary = true

# Integration with audit
[audit.rotation]
keep_days = 30
compress_after_days = 7  # Use gzip for older logs
```

---

## Documentation Updates Needed

Files to create/update:

1. **docs/PERMISSION_SYSTEM.md** - Complete permission system overview
2. **docs/AUDIT_LOGGING.md** - How to read and interpret audit logs
3. **docs/PATH_RESOLUTION.md** - How commands are resolved and validated
4. **docs/SECURITY.md** - Update with new features
5. **AGENTS.md** - Add permission system to agent guidelines

---

## Rollout Plan

### Phase 1: Foundation (Week 1)
- [ ] Implement command resolver
- [ ] Implement audit logger
- [ ] Write tests
- [ ] Document in AGENTS.md

### Phase 2: Integration (Week 2)
- [ ] Integrate resolver into policy evaluator
- [ ] Integrate logger into runloop
- [ ] Add cache layer
- [ ] Test end-to-end

### Phase 3: Features (Week 3)
- [ ] Environment profile scanner
- [ ] Path registry
- [ ] `/audit` slash command
- [ ] Permission summary report

### Phase 4: Polish (Week 4)
- [ ] Comprehensive testing
- [ ] Documentation
- [ ] Configuration examples
- [ ] Performance optimization

---

## Risk Assessment

| Risk | Mitigation | Severity |
|------|-----------|----------|
| Log disk space | Rotate + compress logs | Low |
| Cache invalidation | TTL + manual clear | Low |
| Performance regression | Benchmark during P&R | Medium |
| Breaking changes | New config is optional | Low |
| Security regression | Comprehensive test coverage | Medium |

---

## Success Metrics

1. ✓ All commands are resolved and logged
2. ✓ Audit logs show complete permission flow
3. ✓ No duplicate permission prompts in same session
4. ✓ Agent can describe available tools
5. ✓ Zero security regressions
6. ✓ <1% performance impact
7. ✓ 100% test coverage for new code

---

## Current Code Status

### Existing Components:
- ✓ `CommandPolicyEvaluator` - Config-based allow/deny
- ✓ `SandboxEnvironment` - Filesystem/network restrictions
- ✓ `WorkspaceGuardPolicy` - Path traversal protection
- ✓ `ToolRegistry` - Tool policy management
- ✓ Integration tests for PATH environment

### Missing Components:
- ✗ Command resolution to actual paths
- ✗ Permission caching layer
- ✗ Audit logging system
- ✗ Centralized path whitelist registry
- ✗ Permission audit command

### Partial Components:
- ≈ Lifecycle hooks (exists but not fully utilized)
- ≈ Sandbox logging (basic implementation)

---

## Conclusion

VTCode has a solid foundation for permission management. The improvements suggested above will:

1. **Increase Transparency**: Clear audit trail of all permission decisions
2. **Improve Performance**: Cache repeated policy evaluations
3. **Enhance Security**: Validate commands exist before allowing
4. **Better UX**: Show agent what tools are available
5. **Aid Debugging**: Complete logs for troubleshooting permission issues

The implementation is straightforward, low-risk, and can be done incrementally.
