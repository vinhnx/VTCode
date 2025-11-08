# Step 7: Observability & Metrics

Implementing comprehensive observability for the MCP code execution system to measure effectiveness, identify bottlenecks, and guide agent behavior optimization.

## Overview

**Goal**: Instrument all 5 execution steps to track:
- Agent efficiency (did agent find what it needed?)
- Code execution performance (speed, resource usage)
- Token/context savings (how much did we save?)
- Skill adoption (which skills are reused?)
- Security effectiveness (PII detection rate)
- Error rates and recovery patterns

## Metrics Architecture

```
Agent Execution
    ↓
[Tool Discovery] → discovery_metrics
    - queries_count
    - hit_rate (found what agent needed?)
    - response_time_ms
    - detail_level_requested
    - cache_hits

[Code Execution] → execution_metrics
    - language (python3/javascript)
    - duration_ms
    - success_rate
    - memory_usage_mb
    - timeout_occurrences

[SDK Generation] → sdk_metrics
    - generation_time_ms
    - tools_included
    - ipc_handler_size_bytes
    - cache_utilization

[Data Filtering] → filtering_metrics
    - input_size
    - output_size
    - reduction_ratio (token savings)
    - operation_type (filter/map/reduce)

[Skill Usage] → skill_metrics
    - skill_name
    - execution_count
    - success_rate
    - avg_duration_ms
    - reuse_ratio

[PII Protection] → security_metrics
    - pii_detected_count
    - patterns_matched (email, ssn, card, etc.)
    - tokens_created
    - audit_trail_events
```

## Module Structure

```rust
vtcode-core/src/metrics/
├── mod.rs                 # Metrics registry and aggregation
├── discovery_metrics.rs   # Tool discovery tracking
├── execution_metrics.rs   # Code execution tracking
├── sdk_metrics.rs         # SDK generation tracking
├── filtering_metrics.rs   # Data filtering tracking
├── skill_metrics.rs       # Skill usage tracking
├── security_metrics.rs    # PII and security tracking
└── tests.rs              # Metric tests
```

## Implementation: MetricsCollector

### Core Struct

```rust
pub struct MetricsCollector {
    discovery: Arc<Mutex<DiscoveryMetrics>>,
    execution: Arc<Mutex<ExecutionMetrics>>,
    sdk: Arc<Mutex<SdkMetrics>>,
    filtering: Arc<Mutex<FilteringMetrics>>,
    skills: Arc<Mutex<SkillMetrics>>,
    security: Arc<Mutex<SecurityMetrics>>,
    start_time: Instant,
}

impl MetricsCollector {
    pub fn new() -> Self { ... }
    
    // Query metrics
    pub fn get_discovery_metrics(&self) -> DiscoveryMetrics { ... }
    pub fn get_execution_metrics(&self) -> ExecutionMetrics { ... }
    pub fn get_summary(&self) -> MetricsSummary { ... }
    
    // Record events
    pub fn record_discovery_query(&self, keyword: String, detail_level: DetailLevel) { ... }
    pub fn record_execution_start(&self, language: String) { ... }
    pub fn record_execution_complete(&self, duration_ms: u64, success: bool) { ... }
    pub fn record_pii_detection(&self, pattern_type: String) { ... }
    pub fn record_skill_execution(&self, skill_name: String, duration_ms: u64) { ... }
    
    // Export
    pub fn export_json(&self) -> serde_json::Value { ... }
    pub fn export_prometheus(&self) -> String { ... }
}
```

## Data Structures

### Discovery Metrics

```rust
pub struct DiscoveryMetrics {
    total_queries: u64,
    successful_queries: u64,
    failed_queries: u64,
    total_time_ms: u64,
    avg_response_time_ms: u64,
    detail_level_distribution: HashMap<DetailLevel, u64>,
    hit_rate: f64, // Did agent find what it searched for?
    cache_hits: u64,
    recent_queries: VecDeque<QueryRecord>,
}

pub struct QueryRecord {
    keyword: String,
    detail_level: DetailLevel,
    result_count: u64,
    response_time_ms: u64,
    timestamp: DateTime<Utc>,
    agent_continued_search: bool, // Did agent search again after this?
}
```

### Execution Metrics

```rust
pub struct ExecutionMetrics {
    total_executions: u64,
    successful_executions: u64,
    failed_executions: u64,
    timeouts: u64,
    total_duration_ms: u64,
    avg_duration_ms: u64,
    language_distribution: HashMap<String, u64>,
    memory_peak_mb: u64,
    memory_avg_mb: u64,
    sdk_generation_time_ms: u64,
    recent_executions: VecDeque<ExecutionRecord>,
}

pub struct ExecutionRecord {
    language: String,
    duration_ms: u64,
    success: bool,
    exit_code: i32,
    memory_used_mb: u64,
    result_size_bytes: u64,
    timestamp: DateTime<Utc>,
}
```

### Filtering Metrics

```rust
pub struct FilteringMetrics {
    total_operations: u64,
    operation_distribution: HashMap<String, u64>, // filter, map, reduce, etc.
    total_input_bytes: u64,
    total_output_bytes: u64,
    avg_reduction_ratio: f64,
    context_tokens_saved: u64,
    recent_operations: VecDeque<FilteringRecord>,
}

pub struct FilteringRecord {
    operation: String,
    input_size_bytes: u64,
    output_size_bytes: u64,
    duration_ms: u64,
    tokens_saved_estimate: u64,
    timestamp: DateTime<Utc>,
}
```

### Skill Metrics

```rust
pub struct SkillMetrics {
    total_skills: u64,
    active_skills: u64,
    total_executions: u64,
    skill_stats: HashMap<String, SkillStats>,
    reuse_ratio: f64, // executions / saved_skills
    recent_skill_usage: VecDeque<SkillUsageRecord>,
}

pub struct SkillStats {
    name: String,
    language: String,
    execution_count: u64,
    success_count: u64,
    total_duration_ms: u64,
    avg_duration_ms: u64,
    created_at: DateTime<Utc>,
    last_used: DateTime<Utc>,
}

pub struct SkillUsageRecord {
    skill_name: String,
    success: bool,
    duration_ms: u64,
    timestamp: DateTime<Utc>,
}
```

### Security Metrics

```rust
pub struct SecurityMetrics {
    pii_detections: u64,
    pattern_distribution: HashMap<String, u64>, // email, ssn, card, etc.
    tokens_created: u64,
    audit_events: VecDeque<AuditEvent>,
}

pub struct AuditEvent {
    event_type: String, // "pii_detected", "tokenized", "detokenized"
    pattern_type: String,
    timestamp: DateTime<Utc>,
    severity: String,
}
```

## Integration Points

### 1. Tool Discovery

```rust
// In tool_discovery.rs
impl ToolDiscovery {
    pub fn search(&self, keyword: &str, metrics: Arc<MetricsCollector>) 
        -> Result<SearchResults> {
        let start = Instant::now();
        
        let results = self.perform_search(keyword)?;
        let duration_ms = start.elapsed().as_millis() as u64;
        
        // Record metrics
        metrics.record_discovery_query(
            keyword.to_string(),
            DetailLevel::NameAndDescription,
        );
        metrics.record_discovery_response_time(duration_ms, results.len());
        
        Ok(results)
    }
}
```

### 2. Code Execution

```rust
// In code_executor.rs
impl CodeExecutor {
    pub fn execute(&self, params: ExecutionParams, metrics: Arc<MetricsCollector>) 
        -> Result<ExecutionOutput> {
        metrics.record_execution_start(params.language.clone());
        let start = Instant::now();
        
        let output = self.execute_internal(params)?;
        let duration_ms = start.elapsed().as_millis() as u64;
        
        metrics.record_execution_complete(
            duration_ms,
            output.exit_code == 0,
        );
        metrics.record_execution_memory(output.memory_used_mb);
        metrics.record_result_size(output.result.len());
        
        Ok(output)
    }
}
```

### 3. PII Tokenization

```rust
// In pii_tokenizer.rs
impl PiiTokenizer {
    pub fn tokenize_string(&self, text: &str, metrics: Arc<MetricsCollector>)
        -> Result<(String, TokenMap)> {
        let detected = self.detect_patterns(text)?;
        
        for pattern in &detected {
            metrics.record_pii_detection(pattern.type_name.clone());
            metrics.record_audit_event(
                "pii_detected",
                pattern.type_name.clone(),
                "info",
            );
        }
        
        let (tokenized, map) = self.perform_tokenization(text, detected)?;
        metrics.record_tokens_created(map.len());
        
        Ok((tokenized, map))
    }
}
```

### 4. Skill Manager

```rust
// In skill_manager.rs
impl SkillManager {
    pub fn execute_skill(&self, skill_name: &str, metrics: Arc<MetricsCollector>)
        -> Result<SkillOutput> {
        let start = Instant::now();
        
        let output = self.execute_internal(skill_name)?;
        let duration_ms = start.elapsed().as_millis() as u64;
        
        metrics.record_skill_execution(
            skill_name.to_string(),
            duration_ms,
            output.success,
        );
        
        Ok(output)
    }
}
```

## Metrics Export Formats

### JSON Export

```json
{
  "timestamp": "2024-11-08T10:30:45Z",
  "session_duration_ms": 45000,
  "discovery": {
    "total_queries": 12,
    "successful_queries": 11,
    "hit_rate": 0.92,
    "avg_response_time_ms": 45,
    "cache_hits": 3
  },
  "execution": {
    "total_executions": 8,
    "successful_executions": 7,
    "success_rate": 0.875,
    "avg_duration_ms": 850,
    "languages": {
      "python3": 5,
      "javascript": 3
    },
    "timeouts": 0
  },
  "filtering": {
    "total_operations": 14,
    "avg_reduction_ratio": 0.65,
    "estimated_tokens_saved": 8500
  },
  "skills": {
    "total_skills": 3,
    "executions": 2,
    "reuse_ratio": 0.67
  },
  "security": {
    "pii_detected": 5,
    "patterns": {
      "email": 2,
      "ssn": 1,
      "api_key": 2
    }
  }
}
```

### Prometheus Metrics

```prometheus
# HELP vtcode_discovery_queries_total Total tool discovery queries
# TYPE vtcode_discovery_queries_total counter
vtcode_discovery_queries_total 12

# HELP vtcode_discovery_hit_rate Hit rate of discovery queries
# TYPE vtcode_discovery_hit_rate gauge
vtcode_discovery_hit_rate 0.92

# HELP vtcode_execution_duration_ms Code execution duration
# TYPE vtcode_execution_duration_ms histogram
vtcode_execution_duration_ms_bucket{le="100"} 2
vtcode_execution_duration_ms_bucket{le="500"} 5
vtcode_execution_duration_ms_bucket{le="1000"} 7
vtcode_execution_duration_ms_bucket{le="+Inf"} 8

# HELP vtcode_pii_detections_total Total PII patterns detected
# TYPE vtcode_pii_detections_total counter
vtcode_pii_detections_total{pattern="email"} 2
vtcode_pii_detections_total{pattern="ssn"} 1

# HELP vtcode_skill_reuse_ratio Ratio of skill reuse
# TYPE vtcode_skill_reuse_ratio gauge
vtcode_skill_reuse_ratio 0.67

# HELP vtcode_context_tokens_saved Estimated tokens saved by filtering
# TYPE vtcode_context_tokens_saved counter
vtcode_context_tokens_saved 8500
```

## Dashboards & Visualization

### Key Performance Indicators (KPIs)

```
┌────────────────────────────────────────────────────────┐
│              MCP Execution Metrics Dashboard            │
├────────────────────────────────────────────────────────┤
│                                                        │
│ Discovery Hit Rate:        92% ↑                     │
│ Execution Success Rate:    87.5% ✓                   │
│ Avg Code Execution:        850ms                     │
│ Token Savings (est):       8,500 tokens              │
│ Skill Reuse Ratio:         67%                       │
│ PII Detection:             5 patterns found          │
│                                                        │
├────────────────────────────────────────────────────────┤
│                                                        │
│ Top Skills (by usage):                               │
│  1. filter_test_files      [██████████] 5 runs      │
│  2. analyze_rs_files       [██████] 3 runs          │
│  3. export_json            [████] 2 runs            │
│                                                        │
│ Execution Duration Distribution:                     │
│  < 100ms  ██  (2)                                   │
│  100-500ms ███  (3)                                  │
│  500-1000ms ██████  (5)                              │
│  > 1000ms  ████  (2)                                │
│                                                        │
└────────────────────────────────────────────────────────┘
```

## Logging Integration

### Structured Logging

```rust
// Log with metrics context
info!(
    target: "mcp_metrics",
    discovery_queries = metrics.get_discovery_metrics().total_queries,
    execution_success_rate = metrics.get_execution_metrics().success_rate(),
    estimated_token_savings = metrics.get_filtering_metrics().context_tokens_saved,
    "MCP execution session metrics"
);
```

## Storage & Persistence

### Metrics History

Store metrics for trending analysis:

```
.vtcode/metrics/
├── 2024-11-08/
│   ├── session_001.json  (10:00 - 10:30)
│   ├── session_002.json  (14:15 - 14:45)
│   └── daily_summary.json
├── 2024-11-07/
│   ├── session_001.json
│   └── daily_summary.json
└── quarterly_report.json
```

### Time-Series Storage

For systems with persistent storage:
- InfluxDB: Send metrics via InfluxDB line protocol
- Prometheus: Expose `/metrics` endpoint
- CSV: Export time-series data for analysis

## Use Cases

### 1. Agent Performance Analysis

**Question**: Is the agent finding the tools it needs?

```rust
let discovery = metrics.get_discovery_metrics();
if discovery.hit_rate < 0.8 {
    warn!("Low discovery hit rate: {}%", discovery.hit_rate * 100);
    // Recommend: improve tool naming or add tool aliases
}
```

### 2. Bottleneck Identification

**Question**: What's slowing down code execution?

```rust
let execution = metrics.get_execution_metrics();
if execution.avg_duration_ms > 2000 {
    warn!("Slow code execution: {} ms avg", execution.avg_duration_ms);
    // Recommend: optimize SDK generation, add caching
}
```

### 3. Skill Effectiveness

**Question**: Which skills are actually being reused?

```rust
let skills = metrics.get_skill_metrics();
for (name, stats) in &skills.skill_stats {
    let utilization = stats.execution_count as f64 / 
                      skills.total_executions as f64;
    if utilization < 0.1 {
        info!("Underutilized skill: {}", name);
        // Recommend: remove or improve documentation
    }
}
```

### 4. Security Posture

**Question**: How effective is PII protection?

```rust
let security = metrics.get_security_metrics();
let detection_rate = security.pii_detections as f64 / 
                    security.audit_events.len() as f64;
if detection_rate < 0.95 {
    warn!("PII detection rate low: {}%", detection_rate * 100);
    // Recommend: add custom patterns, improve regex
}
```

## Testing

```bash
# Test metrics collection
cargo test -p vtcode-core metrics --lib

# Test integration with executors
cargo test -p vtcode-core metrics::integration --lib

# Generate sample metrics report
cargo run -p vtcode --example generate_metrics_report
```

## Next Steps

Once Step 7 is complete:

**Step 8**: Tool Versioning & Compatibility
- Track tool schema changes
- Validate skill compatibility
- Migrate outdated skills

**Step 9**: Agent Behavior Optimization
- Use metrics to guide agent decisions
- Learn optimal tool discovery patterns
- Predict code execution failures

## References

- Prometheus Metrics: https://prometheus.io/docs/concepts/metric_types/
- OpenTelemetry: https://opentelemetry.io/
- Observability Best Practices: https://honeycomb.io/resources/guide-to-observability/
