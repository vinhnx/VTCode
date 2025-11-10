# VTCode Permission System: Quick Implementation Guide

**Objective**: Enhance vtcode's permission handling with command resolution, audit logging, and caching  
**Effort**: 4-6 hours across 3 focused modules  
**Impact**: Better visibility, security, and performance  

---

## Overview of Changes

```
Current Flow:                      Improved Flow:
User Input                         User Input
    ↓                                  ↓
Policy Check (no resolution)   →   Command Resolver (resolve to real path)
    ↓                                  ↓
Allow/Deny                         Check Cache (avoid redundant checks)
    ↓                                  ↓
Execute                            Policy Check
    ↓                                  ↓
(No logging)                       Allow/Deny + Log to Audit
                                       ↓
                                   Execute
```

---

## Module 1: Command Resolver (1-2 hours)

### What It Does
Resolves command names to actual filesystem paths using system PATH, with caching.

### File to Create
`vtcode-core/src/tools/command_resolver.rs`

```rust
//! Command resolution system
//! Maps command names to their actual filesystem paths
//! Used by policy evaluator to validate and log command locations

use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, warn};

/// Result of attempting to resolve a command to a filesystem path
#[derive(Debug, Clone)]
pub struct CommandResolution {
    /// The original command name (e.g., "cargo")
    pub command: String,
    
    /// Full path if found in system PATH (e.g., "/Users/user/.cargo/bin/cargo")
    pub resolved_path: Option<PathBuf>,
    
    /// Whether command was found in system PATH
    pub found: bool,
    
    /// Environment used for resolution
    pub search_paths: Vec<PathBuf>,
}

/// Resolver with built-in caching to avoid repeated PATH searches
pub struct CommandResolver {
    /// Cache of already-resolved commands
    cache: HashMap<String, CommandResolution>,
    
    /// Cache hit count for metrics
    cache_hits: usize,
    
    /// Cache miss count for metrics  
    cache_misses: usize,
}

impl CommandResolver {
    /// Create a new resolver with empty cache
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            cache_hits: 0,
            cache_misses: 0,
        }
    }
    
    /// Resolve a command to its filesystem path
    /// 
    /// # Example
    /// ```no_run
    /// let mut resolver = CommandResolver::new();
    /// let cargo = resolver.resolve("cargo");
    /// assert_eq!(cargo.command, "cargo");
    /// assert!(cargo.found);
    /// assert_eq!(cargo.resolved_path, Some("/Users/user/.cargo/bin/cargo".into()));
    /// ```
    pub fn resolve(&mut self, cmd: &str) -> CommandResolution {
        // Extract base command (first word only)
        let base_cmd = cmd.split_whitespace().next().unwrap_or(cmd);
        
        // Check cache first
        if let Some(cached) = self.cache.get(base_cmd) {
            self.cache_hits += 1;
            debug!(
                command = base_cmd,
                cache_hits = self.cache_hits,
                "Command resolution cache hit"
            );
            return cached.clone();
        }
        
        self.cache_misses += 1;
        
        // Try to find command in system PATH
        let resolution = if let Ok(path) = which::which(base_cmd) {
            CommandResolution {
                command: base_cmd.to_string(),
                resolved_path: Some(path.clone()),
                found: true,
                search_paths: Self::get_search_paths(),
            }
        } else {
            warn!(
                command = base_cmd,
                "Command not found in PATH"
            );
            CommandResolution {
                command: base_cmd.to_string(),
                resolved_path: None,
                found: false,
                search_paths: Self::get_search_paths(),
            }
        };
        
        // Cache the result
        self.cache.insert(base_cmd.to_string(), resolution.clone());
        resolution
    }
    
    /// Get current PATH directories being searched
    fn get_search_paths() -> Vec<PathBuf> {
        std::env::var_os("PATH")
            .map(|paths| {
                std::env::split_paths(&paths)
                    .collect()
            })
            .unwrap_or_default()
    }
    
    /// Clear the resolution cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
        debug!("Command resolver cache cleared");
    }
    
    /// Get cache statistics
    pub fn cache_stats(&self) -> (usize, usize) {
        (self.cache_hits, self.cache_misses)
    }
}

impl Default for CommandResolver {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_resolve_common_command() {
        let mut resolver = CommandResolver::new();
        let ls = resolver.resolve("ls");
        assert_eq!(ls.command, "ls");
        // ls should be found on any Unix system
        assert!(ls.found);
    }
    
    #[test]
    fn test_cache_hits() {
        let mut resolver = CommandResolver::new();
        resolver.resolve("ls");
        resolver.resolve("ls");
        let (hits, misses) = resolver.cache_stats();
        assert_eq!(hits, 1);
        assert_eq!(misses, 1);
    }
    
    #[test]
    fn test_nonexistent_command() {
        let mut resolver = CommandResolver::new();
        let fake = resolver.resolve("this_command_definitely_does_not_exist_xyz");
        assert_eq!(fake.command, "this_command_definitely_does_not_exist_xyz");
        assert!(!fake.found);
    }
    
    #[test]
    fn test_extract_base_command() {
        let mut resolver = CommandResolver::new();
        // Should extract "cargo" from "cargo fmt"
        let resolution = resolver.resolve("cargo fmt --check");
        assert_eq!(resolution.command, "cargo");
    }
}
```

### Integration Point 1: Add to lib.rs exports

In `vtcode-core/src/lib.rs`, add to the tools module section:

```rust
pub mod command_resolver;
pub use command_resolver::CommandResolver;
```

### Integration Point 2: Use in CommandPolicyEvaluator

In `vtcode-core/src/tools/command_policy.rs`, add resolver parameter:

```rust
#[derive(Clone)]
pub struct CommandPolicyEvaluator {
    // ... existing fields ...
    
    // NEW: Add resolver
    resolver: Arc<Mutex<CommandResolver>>,
}

impl CommandPolicyEvaluator {
    pub fn with_resolver(mut self, resolver: CommandResolver) -> Self {
        self.resolver = Arc::new(Mutex::new(resolver));
        self
    }
    
    pub async fn allows_text_with_resolution(&self, command_text: &str) -> (bool, Option<PathBuf>) {
        let allowed = self.allows_text(command_text);
        
        // Resolve the command to get actual path
        let resolver = self.resolver.lock().await;
        let base_cmd = command_text.split_whitespace().next().unwrap_or(command_text);
        let resolution = resolver.resolve(base_cmd).clone();
        
        (allowed, resolution.resolved_path)
    }
}
```

---

## Module 2: Audit Logger (1-2 hours)

### What It Does
Records all permission decisions to structured JSON logs for audit trail.

### File to Create
`vtcode-core/src/audit/permission_log.rs`

```rust
//! Permission audit logging system
//! Tracks all permission decisions (allow/deny/prompt) with context
//! Writes to ~/.vtcode/audit/permissions-{date}.log in JSON format

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::path::PathBuf;
use chrono::{DateTime, Local};
use tracing::info;

/// Record of a single permission decision
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionEvent {
    /// When the decision was made
    pub timestamp: DateTime<Local>,
    
    /// What was being requested (command, tool, path, etc.)
    pub subject: String,
    
    /// Type of permission check
    pub event_type: PermissionEventType,
    
    /// The decision reached
    pub decision: PermissionDecision,
    
    /// Why the decision was made
    pub reason: String,
    
    /// Optional resolved path (if applicable)
    pub resolved_path: Option<PathBuf>,
    
    /// Tool or component that made the request
    pub requested_by: String,
}

/// Type of permission event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionEventType {
    CommandExecution,
    ToolUsage,
    FileAccess { read: bool, write: bool },
    NetworkAccess { domain: String },
    SandboxOperation,
    HookExecution,
}

/// The decision reached
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum PermissionDecision {
    Allowed,
    Denied,
    Prompted,
    Cached,
}

/// Audit log for permission decisions
pub struct PermissionAuditLog {
    /// Path to the audit log file
    log_path: PathBuf,
    
    /// Writer for the log file
    writer: BufWriter<std::fs::File>,
    
    /// Count of events logged this session
    event_count: usize,
}

impl PermissionAuditLog {
    /// Create or open the audit log for today
    pub fn new(audit_dir: PathBuf) -> Result<Self> {
        // Create audit directory if needed
        std::fs::create_dir_all(&audit_dir)
            .context("Failed to create audit directory")?;
        
        // Use today's date in filename
        let date = Local::now().format("%Y-%m-%d");
        let log_path = audit_dir.join(format!("permissions-{}.log", date));
        
        // Open file in append mode
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)
            .context(format!("Failed to open audit log at {:?}", log_path))?;
        
        let writer = BufWriter::new(file);
        
        info!(?log_path, "Audit log initialized");
        
        Ok(Self {
            log_path,
            writer,
            event_count: 0,
        })
    }
    
    /// Record a permission event
    pub fn record(&mut self, event: PermissionEvent) -> Result<()> {
        use std::io::Write;
        
        let json = serde_json::to_string(&event)
            .context("Failed to serialize permission event")?;
        
        writeln!(self.writer, "{}", json)
            .context("Failed to write to audit log")?;
        
        self.writer.flush()
            .context("Failed to flush audit log")?;
        
        self.event_count += 1;
        
        info!(
            subject = &event.subject,
            decision = ?event.decision,
            "Permission event logged"
        );
        
        Ok(())
    }
    
    /// Get the number of events logged
    pub fn event_count(&self) -> usize {
        self.event_count
    }
    
    /// Get path to the log file
    pub fn log_path(&self) -> &PathBuf {
        &self.log_path
    }
    
    /// Helper to create and log a permission event
    pub fn log_command_decision(
        &mut self,
        command: &str,
        decision: PermissionDecision,
        reason: &str,
        resolved_path: Option<PathBuf>,
    ) -> Result<()> {
        let event = PermissionEvent {
            timestamp: Local::now(),
            subject: command.to_string(),
            event_type: PermissionEventType::CommandExecution,
            decision,
            reason: reason.to_string(),
            resolved_path,
            requested_by: "CommandPolicyEvaluator".to_string(),
        };
        
        self.record(event)
    }
}

/// Generate a human-readable summary of permission decisions
pub struct PermissionSummary {
    pub total_events: usize,
    pub allowed: usize,
    pub denied: usize,
    pub prompted: usize,
    pub cached: usize,
}

impl PermissionSummary {
    pub fn format(&self) -> String {
        format!(
            "Permission Summary: {} total | {} allowed | {} denied | {} prompted | {} cached",
            self.total_events, self.allowed, self.denied, self.prompted, self.cached
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_audit_log_creation() -> Result<()> {
        let dir = TempDir::new()?;
        let _log = PermissionAuditLog::new(dir.path().to_path_buf())?;
        assert!(dir.path().exists());
        Ok(())
    }
    
    #[test]
    fn test_log_permission_event() -> Result<()> {
        let dir = TempDir::new()?;
        let mut log = PermissionAuditLog::new(dir.path().to_path_buf())?;
        
        log.log_command_decision(
            "cargo fmt",
            PermissionDecision::Allowed,
            "Allow list match",
            Some(PathBuf::from("/usr/local/cargo")),
        )?;
        
        assert_eq!(log.event_count(), 1);
        Ok(())
    }
}
```

### Create Module File
`vtcode-core/src/audit/mod.rs`

```rust
pub mod permission_log;
pub use permission_log::{PermissionAuditLog, PermissionEvent, PermissionDecision};
```

### Integration into lib.rs

```rust
pub mod audit;
pub use audit::{PermissionAuditLog, PermissionEvent, PermissionDecision};
```

### Integration into Agent Runloop

In the agent initialization, create the audit logger:

```rust
// In src/agent/runloop/session.rs or similar
let audit_log = PermissionAuditLog::new(
    workspace_root.join(".vtcode/audit")
)?;

// Pass to tool registry or store in session context
```

---

## Module 3: Permission Cache (0.5-1 hour)

### What It Does
Caches permission decisions for 5 minutes to avoid redundant evaluations.

### File to Create
`vtcode-core/src/tools/command_cache.rs`

```rust
//! Command permission cache
//! Caches policy evaluation results with TTL to improve performance

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::debug;

/// A cached permission decision
#[derive(Debug, Clone)]
struct CacheEntry {
    allowed: bool,
    timestamp: Instant,
    reason: String,
}

/// Cache for command permission decisions
pub struct PermissionCache {
    entries: HashMap<String, CacheEntry>,
    ttl: Duration,
}

impl PermissionCache {
    /// Create cache with 5-minute default TTL
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            ttl: Duration::from_secs(300),
        }
    }
    
    /// Create cache with custom TTL
    pub fn with_ttl(ttl: Duration) -> Self {
        Self {
            entries: HashMap::new(),
            ttl,
        }
    }
    
    /// Check if a command is cached and not expired
    pub fn get(&self, command: &str) -> Option<bool> {
        if let Some(entry) = self.entries.get(command) {
            if entry.timestamp.elapsed() < self.ttl {
                debug!(
                    command = command,
                    reason = &entry.reason,
                    "Permission cache hit ({}s old)",
                    entry.timestamp.elapsed().as_secs()
                );
                return Some(entry.allowed);
            }
        }
        None
    }
    
    /// Store a permission decision in cache
    pub fn put(&mut self, command: &str, allowed: bool, reason: &str) {
        self.entries.insert(
            command.to_string(),
            CacheEntry {
                allowed,
                timestamp: Instant::now(),
                reason: reason.to_string(),
            },
        );
        debug!(
            command = command,
            allowed = allowed,
            reason = reason,
            "Cached permission decision"
        );
    }
    
    /// Clear expired entries
    pub fn cleanup_expired(&mut self) {
        let cutoff = Instant::now() - self.ttl;
        self.entries.retain(|_, entry| entry.timestamp > cutoff);
    }
    
    /// Get cache statistics
    pub fn stats(&self) -> (usize, usize) {
        let total = self.entries.len();
        let expired = self.entries
            .iter()
            .filter(|(_, entry)| entry.timestamp.elapsed() >= self.ttl)
            .count();
        (total, expired)
    }
    
    /// Clear all entries
    pub fn clear(&mut self) {
        self.entries.clear();
        debug!("Permission cache cleared");
    }
}

impl Default for PermissionCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    
    #[test]
    fn test_cache_stores_decision() {
        let mut cache = PermissionCache::new();
        cache.put("cargo fmt", true, "allow_glob match");
        assert_eq!(cache.get("cargo fmt"), Some(true));
    }
    
    #[test]
    fn test_cache_expires() {
        let mut cache = PermissionCache::with_ttl(Duration::from_millis(100));
        cache.put("cargo fmt", true, "test");
        
        // Immediately available
        assert_eq!(cache.get("cargo fmt"), Some(true));
        
        // Wait for expiration
        thread::sleep(Duration::from_millis(150));
        assert_eq!(cache.get("cargo fmt"), None);
    }
    
    #[test]
    fn test_cache_cleanup() {
        let mut cache = PermissionCache::with_ttl(Duration::from_millis(100));
        cache.put("cmd1", true, "test");
        cache.put("cmd2", false, "test");
        
        thread::sleep(Duration::from_millis(150));
        let (total, _) = cache.stats();
        assert_eq!(total, 2);
        
        cache.cleanup_expired();
        let (total, _) = cache.stats();
        assert_eq!(total, 0);
    }
}
```

### Integration into CommandPolicyEvaluator

In `vtcode-core/src/tools/command_policy.rs`:

```rust
use crate::tools::command_cache::PermissionCache;

#[derive(Clone)]
pub struct CommandPolicyEvaluator {
    // ... existing fields ...
    
    // NEW: Add cache
    cache: PermissionCache,
}

impl CommandPolicyEvaluator {
    pub fn new(config: &CommandsConfig) -> Self {
        Self {
            // ... existing initialization ...
            cache: PermissionCache::new(),
        }
    }
    
    pub fn allows_text(&self, command_text: &str) -> bool {
        let cmd = command_text.trim();
        
        // Check cache first
        if let Some(allowed) = self.cache.get(cmd) {
            return allowed;
        }
        
        // ... existing evaluation logic ...
        let allowed = /* evaluation result */;
        
        // Cache the result
        self.cache.put(cmd, allowed, "evaluated");
        
        allowed
    }
}
```

---

## Configuration Changes

Add to `vtcode.toml`:

```toml
# Permission system enhancements
[permissions]
# Enable command resolution to actual paths
resolve_commands = true

# Enable audit logging of all permission decisions
audit_enabled = true
audit_directory = "~/.vtcode/audit"

# Audit what to record
log_allowed_commands = true
log_denied_commands = true
log_permission_prompts = true
log_sandbox_events = true

# Cache configuration
cache_ttl_seconds = 300
cache_enabled = true
```

---

## Testing Plan

### Unit Tests
```bash
cargo test -p vtcode-core command_resolver
cargo test -p vtcode-core permission_log  
cargo test -p vtcode-core command_cache
```

### Integration Test
Add to `tests/integration_permissions.rs`:

```rust
#[tokio::test]
async fn test_full_permission_flow() {
    // 1. Create resolver
    // 2. Create audit log
    // 3. Create cache
    // 4. Simulate command execution
    // 5. Verify:
    //    - Command resolved
    //    - Decision cached
    //    - Event logged
}
```

---

## Rollout Steps

### Step 1: Add CommandResolver (30 min)
```bash
# Create file
cargo test -p vtcode-core command_resolver

# Verify compilation
cargo check -p vtcode-core
```

### Step 2: Add AuditLog (30 min)
```bash
# Create module
cargo test -p vtcode-core permission_log

# Verify it works
cargo build -p vtcode-core
```

### Step 3: Add Cache (20 min)
```bash
# Create cache
cargo test -p vtcode-core command_cache

# Verify full build
cargo build
```

### Step 4: Integrate All (30 min)
- Modify CommandPolicyEvaluator to use all three
- Wire up to agent session
- Add config options

### Step 5: Test & Verify (30 min)
```bash
# Run all tests
cargo test

# Run linter
cargo clippy

# Format code
cargo fmt

# Test on real commands
cargo run -- ask "cargo fmt"
```

---

## Verification Checklist

- [ ] All three modules compile without warnings
- [ ] All tests pass: `cargo test`
- [ ] All clippy checks pass: `cargo clippy`
- [ ] Formatted with: `cargo fmt`
- [ ] Audit log created in ~/.vtcode/audit
- [ ] Command paths resolved correctly
- [ ] Cache reduces redundant evaluations
- [ ] Existing tests still pass
- [ ] No breaking changes to public APIs
- [ ] Documentation updated in AGENTS.md

---

## Expected Results

After implementation:

**Before**:
```
$ cargo fmt  
(runs without showing what was checked or why)
```

**After**:
```
$ cargo fmt
[INFO] Resolving command: cargo → /Users/user/.cargo/bin/cargo
[INFO] Checking cache: cache_hit=true
[INFO] Policy decision: ALLOWED (match: cargo *)
[INFO] Event logged: ~/.vtcode/audit/permissions-2025-11-09.log
(runs)
```

With audit log showing:
```json
{
  "timestamp": "2025-11-09T14:22:33.123456",
  "subject": "cargo fmt",
  "event_type": "CommandExecution",
  "decision": "Allowed",
  "reason": "allow_glob match: cargo *",
  "resolved_path": "/Users/user/.cargo/bin/cargo",
  "requested_by": "CommandPolicyEvaluator"
}
```

---

## Quick Reference

| Component | Time | Files | Key Classes |
|-----------|------|-------|-------------|
| Command Resolver | 1-2h | 1 new | `CommandResolver`, `CommandResolution` |
| Audit Logger | 1-2h | 2 new | `PermissionAuditLog`, `PermissionEvent` |
| Permission Cache | 0.5-1h | 1 new | `PermissionCache` |
| Integration | 0.5-1h | Modified | `CommandPolicyEvaluator`, agent session |
| **Total** | **4-6h** | **4-5 files** | - |

---

## Next Steps After Implementation

1. Add `/audit` slash command to view audit logs
2. Create `PermissionSummary` report
3. Add environment profile scanner
4. Create centralized path whitelist registry
5. Enhanced sandbox logging integration
