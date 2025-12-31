# Phase 3: Extension Integration Plan

**Timeline**: 2-3 weeks  
**Status**: Planning  
**Dependencies**: Phase 2C (Complete ✅)

## Objectives

Integrate `vtcode-file-search` with Zed and VS Code native file pickers, replacing built-in file search with optimized version. This enables users to experience faster file discovery across both IDEs.

## Architecture Overview

```
┌─────────────────────────────────────────────────────────────┐
│                    File Search Integration                   │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  ┌──────────────────┐            ┌──────────────────┐      │
│  │  Zed Extension   │            │ VS Code Extension │      │
│  │                  │            │                  │       │
│  │ ┌────────────┐   │            │ ┌────────────┐   │      │
│  │ │ File       │   │            │ │ File Search│   │      │
│  │ │ Picker UI  │   │            │ │ Service    │   │      │
│  │ └─────┬──────┘   │            │ └─────┬──────┘   │      │
│  └───────┼──────────┘            └───────┼──────────┘      │
│          │                               │                  │
│  ┌───────┴────────────────────────────────┴────────────┐   │
│  │                                                      │   │
│  │      VT Code File Search Bridge (Rust)             │   │
│  │      • FileSearchConfig                            │   │
│  │      • search_files() API                          │   │
│  │      • Cancellation support                        │   │
│  │      • Parallel traversal                          │   │
│  │      • .gitignore respect                          │   │
│  │                                                      │   │
│  └───────┬────────────────────────────────────┬───────┘   │
│          │                                    │             │
│  ┌───────┴──────────────────┐    ┌───────────┴─────────┐  │
│  │  vtcode-file-search      │    │  vtcode CLI (RPC)   │  │
│  │  • Fuzzy matcher         │    │  • subprocess IPC   │  │
│  │  • Atomic results        │    │  • JSON serialization
│  │  • Exclusion patterns    │    │                     │  │
│  │  • Lock-free design      │    │                     │  │
│  └──────────────────────────┘    └─────────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

## Phase 3a: Zed Integration (Week 1-1.5)

### 1.1 Architecture Design

**Approach**: Direct Rust FFI integration (no subprocess overhead)

**Benefits**:
- Zero-copy file results
- Direct cancellation control
- Shared memory between extension and core
- Type-safe API matching Zed's patterns

**Integration Points**:
- `zed_extension_api` for UI hooks
- Direct vtcode-file-search crate import
- Async executor for non-blocking searches

### 1.2 Implementation Tasks

#### Task 1.2.1: Create File Search Service Module
**File**: `zed-extension/src/file_search_service.rs`

```rust
pub struct FileSearchService {
    workspace_root: PathBuf,
    config: FileSearchConfig,
}

impl FileSearchService {
    pub async fn search_files(
        &self,
        pattern: String,
        max_results: usize,
        cancel_flag: Arc<AtomicBool>,
    ) -> Result<Vec<FileMatch>> {
        // Delegate to bridge
        file_search_bridge::search_files(config, Some(cancel_flag))
    }
}
```

**Checklist**:
- [ ] Module structure mirrors Zed extension conventions
- [ ] Implements cancellation token handling
- [ ] Error handling maps Rust errors to Zed API errors
- [ ] Unit tests for service initialization

#### Task 1.2.2: Zed File Picker Integration
**File**: `zed-extension/src/commands.rs` (extend `find_files`)

**Current**: Subprocess call to `vtcode list-files`  
**New**: Direct FFI to file_search_bridge

```rust
pub fn find_files(pattern: &str) -> CommandResponse {
    let service = FileSearchService::new(workspace_root)?;
    service.search_files(pattern, MAX_RESULTS, cancel_flag)
}
```

**Checklist**:
- [ ] Replace subprocess with direct service call
- [ ] Maintain backward compatibility for command signatures
- [ ] Add cancellation via Zed's UI cancellation API
- [ ] Performance metrics before/after

#### Task 1.2.3: Quick Open Enhancement
**File**: `zed-extension/src/editor.rs` (new `QuickFileOpen` mode)

**Integration with Zed's Quick Open**:
- Hook into editor keymap (Cmd+P / Ctrl+P)
- Debounce user input (150ms)
- Real-time result streaming

**Checklist**:
- [ ] Intercept Zed's quick-open activation
- [ ] Stream results to UI as they become available
- [ ] Handle special patterns (e.g., `:` for line numbers)
- [ ] Fuzzy highlighting with indices from nucleo

#### Task 1.2.4: Testing & Optimization
**Tests**:
- [ ] Unit tests for FileSearchService
- [ ] Integration tests with Zed's extension host
- [ ] Cancellation behavior under load
- [ ] Memory usage with large result sets (10k+ files)

**Optimization**:
- [ ] Profile with 100k file workspace
- [ ] Tune thread count based on results
- [ ] Implement LRU caching for recent patterns
- [ ] Benchmark against Zed native quick-open

### 1.3 Milestones

- **Day 1-2**: Design & module structure
- **Day 3-4**: Service implementation & testing
- **Day 5-6**: Zed integration & quick-open hookup
- **Day 7+**: Performance optimization & user testing

---

## Phase 3b: VS Code Integration (Week 1.5-2.5)

### 2.1 Architecture Design

**Approach**: RPC-based service (VT Code CLI subprocess)

**Rationale**:
- VS Code extensions run in Node.js, not Rust
- VT Code CLI already provides RPC interface
- Language interoperability via JSON-RPC
- Subprocess isolation for robustness

**Integration Flow**:

```
VS Code Extension (TypeScript)
    ↓
    RPC Call: { "method": "search_files", "params": {...} }
    ↓
VT Code Subprocess (Rust CLI)
    ↓
file_search_bridge (in vtcode-core)
    ↓
RPC Response: { "result": { "matches": [...] } }
    ↓
Quick Open / Command Palette UI
```

### 2.2 Implementation Tasks

#### Task 2.2.1: VT Code CLI RPC Endpoint
**File**: `vtcode-core/src/rpc/file_search_endpoint.rs` (new)

**API**:
```rust
pub struct SearchFilesRequest {
    pub pattern: String,
    pub workspace_root: PathBuf,
    pub max_results: usize,
    pub exclude_patterns: Vec<String>,
}

pub async fn handle_search_files(req: SearchFilesRequest) -> RpcResult<FileSearchResults>
```

**Checklist**:
- [ ] Add RPC method registration in CLI entrypoint
- [ ] Implement streaming results (for large result sets)
- [ ] Handle cancellation via client disconnect
- [ ] Validate workspace_root (security boundary)
- [ ] Add timeout (default 30s)

#### Task 2.2.2: VS Code File Search Service
**File**: `vscode-extension/src/services/fileSearchService.ts` (new)

```typescript
export class FileSearchService {
    async searchFiles(
        pattern: string,
        maxResults: number,
        token: CancellationToken
    ): Promise<FileMatch[]> {
        // RPC call to VT Code subprocess
    }
    
    async listFiles(excludePatterns: string[]): Promise<string[]> {
        // List all files with optional exclusions
    }
}
```

**Checklist**:
- [ ] Implements VS Code's FileSystemProvider patterns
- [ ] Handles subprocess lifecycle (start/stop)
- [ ] Cancellation token integration
- [ ] Error recovery & reconnection logic
- [ ] Result caching for duplicate queries

#### Task 2.2.3: Command Palette Integration
**File**: `vscode-extension/src/commands/fileSearch.ts` (new)

**Hooks into**:
- `vscode.openFile` command (quick open)
- `workbench.action.quickOpen` (fuzzy file finder)
- Custom `/file-search` slash command in chat

```typescript
export async function registerFileSearchCommands() {
    // Override VS Code's built-in file picker
    vscode.commands.registerCommand('vtcode.quickOpen', async (query) => {
        const results = await fileSearchService.searchFiles(query, 50);
        // Present results in QuickPick UI
    });
}
```

**Checklist**:
- [ ] Register command in `package.json`
- [ ] Implement QuickPick UI with previews
- [ ] Handle file selection & opening
- [ ] Add fuzzy highlighting (match indices)

#### Task 2.2.4: Chat Integration
**File**: `vscode-extension/src/chat/fileSearchTools.ts` (new)

**Enables agents to search files directly in chat**:

```
User: @vtcode Find all test files in the src directory

Chat Tool Registry:
├── file_search (search by pattern)
├── list_files (enumerate directory)
└── find_references (cross-reference search)
```

**Checklist**:
- [ ] Register as MCP-compatible tool
- [ ] Implement tool schema validation
- [ ] Handle agent cancellation mid-search
- [ ] Stream results to chat UI

#### Task 2.2.5: Configuration & Settings
**File**: `vscode-extension/package.json` (extend)

```json
{
  "contributes": {
    "configuration": {
      "properties": {
        "vtcode.fileSearch.maxResults": {
          "type": "number",
          "default": 100
        },
        "vtcode.fileSearch.respectGitignore": {
          "type": "boolean",
          "default": true
        },
        "vtcode.fileSearch.excludePatterns": {
          "type": "array",
          "default": ["**/node_modules/**", "**/.git/**"]
        }
      }
    }
  }
}
```

**Checklist**:
- [ ] User-configurable limits
- [ ] Pattern exclusion rules
- [ ] Enable/disable by default
- [ ] Settings schema validation

### 2.3 Milestones

- **Day 1-2**: CLI RPC endpoint & testing
- **Day 3-4**: FileSearchService implementation
- **Day 5-6**: Command palette integration
- **Day 7**: Chat integration & configuration

---

## Phase 3c: Cross-Platform & Performance (Week 2.5-3)

### 3.1 Performance Benchmarking

**Test Scenarios**:

| Scenario | Files | Pattern | Expected | Target |
|----------|-------|---------|----------|--------|
| Small project | 1k | "main" | <50ms | <30ms |
| Medium project | 10k | "test" | <200ms | <150ms |
| Large project | 100k | "lib" | <1s | <750ms |
| Monorepo | 500k | "index" | <5s | <3s |

**Benchmarking Script**: `scripts/bench_file_search.sh`

```bash
#!/bin/bash
# Compare vtcode-file-search vs ripgrep vs Zed native

declare -a patterns=("main" "test" "index" "config")
declare -a workspaces=("/path/to/small" "/path/to/medium" "/path/to/large")

for workspace in "${workspaces[@]}"; do
    for pattern in "${patterns[@]}"; do
        echo "Testing: $workspace with pattern: $pattern"
        
        # VT Code file search
        time vtcode-file-search "$pattern" "$workspace"
        
        # Ripgrep (baseline)
        time rg --files "$workspace" | grep -F "$pattern"
        
        # Zed quick open (if applicable)
        # Manual timing via editor UI
    done
done
```

**Checklist**:
- [ ] Establish baseline metrics
- [ ] Profile memory usage under load
- [ ] Test with .gitignore files (50k+ patterns)
- [ ] Measure index loading time
- [ ] Compare cold vs warm cache performance

### 3.2 Cross-Platform Validation

**Test Matrix**:

| Platform | OS Version | Zed | VS Code | Status |
|----------|-----------|-----|---------|--------|
| macOS | 14.x (M-series) | 0.160+ | 1.95+ | TBD |
| macOS | 13.x (Intel) | 0.160+ | 1.95+ | TBD |
| Linux | Ubuntu 22.04 | 0.160+ | 1.95+ | TBD |
| Linux | Fedora 39 | 0.160+ | 1.95+ | TBD |
| Windows | 11 | 0.160+ | 1.95+ | TBD |
| Windows | 10 | 0.160+ | 1.95+ | TBD |

**Test Cases**:
- [ ] File paths with spaces
- [ ] Symlink handling
- [ ] Windows path separators (backslash)
- [ ] Unicode filenames
- [ ] Deep nesting (100+ levels)
- [ ] Permission denied edge cases

### 3.3 User Acceptance Testing

**UAT Checklist**:

1. **Functionality**
   - [ ] Search returns all matching files
   - [ ] Fuzzy matching works intuitively
   - [ ] Case sensitivity option works
   - [ ] Exclusion patterns respected

2. **Performance**
   - [ ] First result appears within 100ms
   - [ ] Typing feels responsive (debounce working)
   - [ ] No UI freeze on large result sets

3. **Integration**
   - [ ] Hotkey works (Cmd+P / Ctrl+P)
   - [ ] Results preview file content
   - [ ] Selection opens file correctly
   - [ ] Cancel button works

4. **Edge Cases**
   - [ ] Very long filenames (>255 chars)
   - [ ] Patterns with special regex chars
   - [ ] Binary files excluded properly
   - [ ] .gitignore updates reflected immediately

---

## Technical Decisions

### Decision 1: Zed Integration Method
**Chosen**: Direct Rust FFI  
**Rationale**: Minimal overhead, type-safety, shared memory

**Alternative Rejected**: Subprocess via CLI
- Would add 200-500ms latency per search
- Duplicate codebase (Rust + TypeScript)

### Decision 2: VS Code Integration Method
**Chosen**: RPC-based service  
**Rationale**: Node.js ↔ Rust boundary, language interoperability

**Alternative Rejected**: Native module (node-gyp)
- Complex build process
- Platform-specific compilation
- Security review overhead

### Decision 3: Caching Strategy
**Chosen**: Per-session LRU cache (100 entries)  
**Rationale**: Most users search same patterns repeatedly

**Cache Key**: `(pattern, max_results, exclude_patterns)` hash  
**TTL**: Session lifetime (user closes IDE)

---

## Risk Mitigation

| Risk | Impact | Mitigation | Owner |
|------|--------|-----------|-------|
| Zed API instability | High | Pin extension API version, use stable APIs only | @dev-zed |
| VS Code subprocess failures | High | Implement health checks, auto-restart logic | @dev-vscode |
| Performance regression | High | Continuous benchmarking, regression tests | @perf-team |
| Large workspace hangs | Medium | Timeout on search + progress indicator | @ux-team |
| Unicode handling issues | Medium | Comprehensive test suite, platform testing | @qa-team |

---

## Success Criteria

1. ✅ File search latency < 1s for 100k files
2. ✅ Extension loads without startup delay
3. ✅ All tests pass on macOS, Linux, Windows
4. ✅ User feedback: "Noticeably faster than before"
5. ✅ Zero regressions in existing functionality

---

## Dependencies & Blockers

### External Dependencies
- ✅ vtcode-file-search crate (complete)
- ✅ file_search_bridge module (complete)
- ⏳ Zed 0.160+ release (Q1 2026)
- ⏳ VS Code 1.95+ (available now)

### Internal Dependencies
- ✅ Phase 2C: Tool Integration (complete)
- ⏳ Performance baseline established
- ⏳ Cross-platform test infrastructure

### Known Blockers
- None identified

---

## Timeline Summary

| Week | Phase | Tasks | Deliverables |
|------|-------|-------|--------------|
| 1 | 3a | Zed service + file picker | Working quick-open in Zed |
| 1.5 | 3b-1 | CLI RPC endpoint | File search JSON-RPC API |
| 2 | 3b-2 | VS Code integration | Command palette integration |
| 2.5 | 3c | Performance & testing | Benchmarks, cross-platform validation |

**Total Estimated Effort**: 2-3 weeks (80-120 hours)

---

## Next Steps

1. **Approve Architecture** - Review design with team
2. **Set Up Infrastructure** - CI/CD for extension testing
3. **Begin Implementation** - Start with Phase 3a (Zed)
4. **Establish Baselines** - Benchmark current vs. new
5. **User Testing** - Early access program with 10-20 users

---

## Related Documents

- [PHASE_2C_INTEGRATION_COMPLETE.md](PHASE_2C_INTEGRATION_COMPLETE.md) - Tool integration summary
- [FILE_SEARCH_IMPLEMENTATION.md](FILE_SEARCH_IMPLEMENTATION.md) - Core library details
- [zed-extension/README.md](../zed-extension/README.md) - Zed extension guide
- [vscode-extension/README.md](../vscode-extension/README.md) - VS Code extension guide
