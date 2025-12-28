# MCP Implementation Roadmap

## Current Status

VT Code has a **functional but foundational** MCP implementation. Based on review of Claude's official MCP documentation, this roadmap outlines improvements needed to reach production-grade quality.

## Phase 1: Foundation (Current - Complete)

### ✅ Completed
- **MCP Integration Guide** - Comprehensive documentation of current implementation
- **Architecture Documentation** - Component breakdown and responsibilities
- **Configuration Guide** - How to configure MCP servers and policies
- **Security Framework** - Validation, allowlists, and access control

### Current Limitations
- No connection pooling (providers initialized sequentially)
- No tool discovery caching
- No performance optimization
- Limited enterprise features

---

## Phase 2: Performance & Optimization (Next Priority)

### Goal
Improve startup time (60%+) and tool discovery latency (99%+) through connection pooling and caching.

### Required Work

#### 2.1 Connection Pooling Implementation
**Status**: Disabled (needs API compatibility fixes)

**What needs fixing**:
```rust
// Current issue: McpProvider::initialize() signature mismatch
// Current: async fn initialize(params: InitializeRequestParams, ...)
// Needed: Flexible to work with pool's concurrent initialization

// Fix: Either
// 1. Create wrapper that handles concurrent initialization properly
// 2. Refactor McpProvider to support connection pooling pattern
// 3. Create separate PooledMcpProvider wrapper
```

**Expected benefits**:
- 3 providers: 3.0s → 1.2s (60% improvement)
- Semaphore-based concurrency control
- Graceful timeout handling

**Effort**: Medium (2-3 days)

#### 2.2 Tool Discovery Caching
**Status**: Disabled (struct field mismatch with ToolDiscoveryResult)

**What needs fixing**:
```rust
// Current ToolDiscoveryResult structure:
pub struct ToolDiscoveryResult {
    pub name: String,
    pub provider: String,
    pub description: String,
    pub relevance_score: f32,
    pub input_schema: Option<Value>,
}

// Cache module expected: tool.name, tool.provider, etc.
// Fix: Adapt cache to match actual structure OR extend ToolDiscoveryResult
```

**Expected benefits**:
- Bloom filter for O(1) negative lookups
- LRU cache with TTL for positive results
- 99%+ cache hit reduction on repeated queries
- Sub-millisecond lookups

**Effort**: Easy (1-2 days)

#### 2.3 Performance Monitoring
**Status**: Not started

**What's needed**:
- Metrics collection (pool stats, cache stats, tool execution times)
- Prometheus integration
- Performance dashboard
- Logging of initialization times

**Effort**: Medium (2-3 days)

---

## Phase 3: Enterprise Features (Medium Priority)

### Goal
Support organizational governance and compliance requirements.

### Required Work

#### 3.1 Managed MCP Configuration
**Status**: Documented but not tested

**Implementation**:
```toml
# /etc/claude-code/managed-mcp.json (system-wide)
# /Library/Application Support/ClaudeCode/managed-mcp.json (macOS)
{
  "mcpServers": {
    "approved-server": { }
  }
}
```

**Testing needed**:
- Verify system-wide config is read on all platforms
- Test precedence (managed > user > project)
- Validate user cannot override managed config

**Effort**: Low (1 day)

#### 3.2 Allowlist/Denylist Enforcement
**Status**: Code exists but needs testing

**Testing needed**:
- Name-based matching
- Command-based pattern matching
- URL-based pattern matching
- Denylist precedence over allowlist
- Cross-provider enforcement

**Effort**: Medium (2 days for full coverage)

#### 3.3 Audit Logging
**Status**: Not started

**What's needed**:
- Log all tool executions (tool name, args, result)
- Track provider initialization/failures
- Record configuration changes
- Support structured logging (JSON)
- Integration with SIEM systems

**Effort**: Medium (2-3 days)

---

## Phase 4: Advanced Features (Lower Priority)

### 4.1 HTTP Transport Optimization
- Connection pooling for HTTP clients
- Retry logic with exponential backoff
- Circuit breaker pattern
- Load balancing across replicas

### 4.2 Resource Caching
- Persistent cache for static resources
- Invalidation signals from providers
- Disk-based cache with TTL
- Memory-mapped file support

### 4.3 Tool Execution Optimization
- Tool result caching (read-only tools only)
- Partial result streaming
- Timeout management per tool
- Resource limits (memory, CPU)

### 4.4 Distributed MCP
- Shared cache across multiple instances
- Distributed tool execution
- Load balancing and failover
- Provider health checks

---

## Detailed Implementation Guide

### Fixing Connection Pooling

**Current Issue**: `McpProvider` initialization is tightly coupled to sequential operation

**Solution Option 1: Wrapper Pattern** (Recommended)
```rust
pub struct PooledMcpProvider {
    inner: McpProvider,
    semaphore: Arc<Semaphore>,
}

impl PooledMcpProvider {
    pub async fn initialize_pooled(...) -> Result<()> {
        let _permit = self.semaphore.acquire().await?;
        self.inner.initialize(...).await
    }
}
```

**Solution Option 2: Refactor McpProvider**
```rust
// Add optional pooling support to McpProvider
pub struct McpProvider {
    pool: Option<Arc<Semaphore>>,
}

impl McpProvider {
    pub async fn initialize_with_pool(
        config: McpProviderConfig,
        pool: Option<Arc<Semaphore>>,
    ) -> Result<()> {
        if let Some(p) = pool {
            let _permit = p.acquire().await?;
        }
        // ... existing init code
    }
}
```

**Effort**: 2-3 days (includes testing and integration)

### Fixing Tool Discovery Caching

**Step 1: Adapt cache to actual structure**
```rust
pub struct CachedToolDiscovery {
    cache: Arc<RwLock<HashMap<String, Vec<McpToolInfo>>>>,
}

impl CachedToolDiscovery {
    pub fn search_tools(
        &self,
        keyword: &str,
        detail_level: DetailLevel,
    ) -> Vec<ToolDiscoveryResult> {
        // Search cached tools
        // Return matching ToolDiscoveryResult items
    }
}
```

**Step 2: Integrate with ToolDiscovery service**
```rust
pub struct ToolDiscovery {
    mcp_client: Arc<dyn McpToolExecutor>,
    cache: Arc<CachedToolDiscovery>,  // Add caching layer
}
```

**Step 3: Update search_tools to use cache**
```rust
pub async fn search_tools(
    &self,
    keyword: &str,
    detail_level: DetailLevel,
) -> Result<Vec<ToolDiscoveryResult>> {
    // Check cache first
    if let Some(cached) = self.cache.search_tools(keyword, detail_level) {
        return Ok(cached);
    }

    // Fetch from MCP client
    let all_tools = self.mcp_client.list_mcp_tools().await?;
    let results = self.perform_search(&all_tools, keyword, detail_level);

    // Cache results
    self.cache.cache_results(keyword, results.clone(), detail_level);

    Ok(results)
}
```

**Effort**: 1-2 days (simpler, no API changes needed)

---

## Testing Strategy

### Phase 2 Testing
```bash
# Connection pooling
cargo test mcp::connection_pool --lib
cargo bench mcp::initialization

# Tool discovery caching
cargo test mcp::tool_discovery --lib
cargo bench mcp::tool_search
```

### Integration Testing
```bash
# Test with real MCP servers
cargo test --test integration_mcp -- --nocapture

# Performance regression testing
cargo bench mcp
```

### Enterprise Testing
```bash
# Test managed config loading
cargo test mcp::enterprise --lib

# Test allowlist enforcement
cargo test mcp::security --lib
```

---

## Success Metrics

### Phase 2 Goals
| Metric | Current | Target | Status |
|--------|---------|--------|--------|
| 3-provider startup | 3.0s | 1.2s | 🔲 Todo |
| Tool search (repeated) | 500ms | <1ms | 🔲 Todo |
| Cache hit rate | N/A | >99% | 🔲 Todo |
| P99 latency | - | <100ms | 🔲 Todo |

### Phase 3 Goals
| Metric | Current | Target | Status |
|--------|---------|--------|--------|
| Managed config support | ❌ | ✅ | 🔲 Todo |
| Allowlist enforcement | ⚠️ | ✅ | 🔲 Todo |
| Audit logging | ❌ | ✅ | 🔲 Todo |

---

## Architecture Decisions Needed

### 1. Caching Strategy
**Options**:
- A) **In-memory only** (current cache module approach)
  - Pros: Fast, simple
  - Cons: Lost on restart, memory overhead

- B) **Hybrid (memory + disk)**
  - Pros: Persistent across restarts
  - Cons: More complex, I/O overhead

- C) **Distributed (multi-instance)**
  - Pros: Shared across instances
  - Cons: Network latency, external dependency

**Recommendation**: Start with A, upgrade to B later

### 2. Connection Pool Configuration
**Options**:
- A) **Dynamic**: Auto-tune based on provider response times
- B) **Fixed**: User-configured in vtcode.toml
- C) **Adaptive**: Start fixed, adjust over time

**Recommendation**: Start with B, migrate to C

### 3. Error Handling
**Options**:
- A) **Fail-fast**: One provider failure blocks others
- B) **Best-effort**: Continue with available providers
- C) **Circuit-breaker**: Temporarily skip failing providers

**Recommendation**: Implement B (graceful degradation)

---

## Resource Requirements

### Team Size
- **Minimum**: 1 engineer (part-time)
- **Recommended**: 1-2 engineers (full-time for 2-3 months)

### Skills Needed
- Rust (async/await, trait-driven design)
- Performance optimization
- Distributed systems concepts
- Testing and benchmarking

### Infrastructure
- CI/CD for performance testing
- Benchmark tracking (optional)
- Load testing environment (for HTTP transport)

---

## Risk Assessment

### Risk: API Breaking Changes
**Severity**: Medium
**Mitigation**: Keep backward compatibility, add new APIs gradually

### Risk: Performance Regression
**Severity**: High
**Mitigation**: Comprehensive benchmarking, regression detection in CI

### Risk: Cache Invalidation Issues
**Severity**: Medium
**Mitigation**: Conservative TTL, manual refresh endpoints

### Risk: Deadlocks in Connection Pool
**Severity**: Medium
**Mitigation**: Extensive testing, timeout protection

---

## Timeline Estimate

| Phase | Duration | Notes |
|-------|----------|-------|
| Phase 1: Foundation | ✅ Done | Documentation complete |
| Phase 2: Performance | 3-4 weeks | ~2 engineers part-time |
| Phase 3: Enterprise | 2-3 weeks | ~1 engineer |
| Phase 4: Advanced | 6-8 weeks | Ongoing, lower priority |

---

## Next Steps

### Immediate (This Week)
1. ✅ Document current state (DONE)
2. Choose caching strategy
3. Choose pool configuration approach
4. Create GitHub issues for each task

### Short Term (Next 2 Weeks)
1. Fix tool discovery caching compatibility
2. Add comprehensive tests
3. Performance benchmarking setup
4. Documentation updates

### Medium Term (Next 4-8 Weeks)
1. Implement connection pooling
2. Enterprise feature testing
3. Performance optimization
4. Load testing

---

## Related Documentation

- **Current Implementation**: [MCP_INTEGRATION_GUIDE.md](./MCP_INTEGRATION_GUIDE.md)
- **Architecture**: [ARCHITECTURE.md](./ARCHITECTURE.md)
- **Security**: [SECURITY_MODEL.md](./SECURITY_MODEL.md)
- **Configuration**: [config/CONFIGURATION_PRECEDENCE.md](./config/CONFIGURATION_PRECEDENCE.md)

---

## Questions for Discussion

1. **Priority**: Which features are highest priority for your use case?
2. **Caching**: Should we persist cache across restarts?
3. **Enterprise**: Which enterprise features are critical?
4. **Performance**: Are the 60% startup improvement goals sufficient?
5. **Team**: How many engineers can be allocated?

---

**Last Updated**: Dec 28, 2025
**Status**: In Progress
**Owner**: VT Code Team
