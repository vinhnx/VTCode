# Tool Improvements System - Quick Start

## Installation

The improvements system is built into vtcode-core. Import the modules:

```rust
use vtcode::tools::{
    ImprovementsConfig,
    jaro_winkler_similarity,
    PatternDetector,
    MiddlewareChain,
    LoggingMiddleware,
    CachingMiddleware,
    ObservabilityContext,
};
```

## Basic Examples

### 1. String Similarity

Find tools related to a given tool name:

```rust
use vtcode::tools::jaro_winkler_similarity;

let similarity = jaro_winkler_similarity("grep_file", "grep_directory");
// Returns: 0.873 (high similarity, prefix bonus)

if similarity > 0.8 {
    println!("These tools are related!");
}
```

### 2. Pattern Detection

Detect if a user is stuck in a loop:

```rust
use vtcode::tools::PatternDetector;

let detector = PatternDetector::new(10);

let history = vec![
    ("grep_file".to_string(), "pattern:error".to_string(), 0.3),
    ("grep_file".to_string(), "pattern:error".to_string(), 0.3),
    ("grep_file".to_string(), "pattern:error".to_string(), 0.3),
];

match detector.detect(&history) {
    PatternState::Loop => println!("User is repeating the same command!"),
    PatternState::RefinementChain => println!("User is improving their approach"),
    PatternState::Degradation => println!("Results are getting worse"),
    _ => println!("No significant pattern"),
}
```

### 3. Configuration Management

Load and validate configuration:

```rust
use vtcode::tools::ImprovementsConfig;

// Load from TOML file
let config = ImprovementsConfig::from_file("vtcode.toml")?;

// Validate all settings
config.validate()?;

// Access settings
println!("Min similarity: {}", config.similarity.min_similarity_threshold);
println!("Cache size: {}", config.cache.max_entries);
```

### 4. Middleware Chain

Build a composable execution pipeline:

```rust
use std::sync::Arc;
use vtcode::tools::{
    MiddlewareChain,
    LoggingMiddleware,
    CachingMiddleware,
    RetryMiddleware,
    ToolRequest,
    RequestMetadata,
};
use tracing::Level;

// Build chain
let chain = MiddlewareChain::new()
    .add(Arc::new(LoggingMiddleware::new(Level::DEBUG)))
    .add(Arc::new(CachingMiddleware::new()))
    .add(Arc::new(RetryMiddleware::new(3, 100, 5000)));

// Execute tool with middleware
let request = ToolRequest {
    tool_name: "grep_file".to_string(),
    arguments: "pattern:error".to_string(),
    context: "src/".to_string(),
    metadata: RequestMetadata::default(),
};

let result = chain.execute_sync(request, |req| {
    // Your tool execution logic here
    MiddlewareResult {
        success: true,
        result: Some("found 10 errors".to_string()),
        error: None,
        metadata: Default::default(),
    }
});

println!("Execution took: {}ms", result.metadata.duration_ms);
println!("From cache: {}", result.metadata.from_cache);
```

### 5. Observability

Track events and errors:

```rust
use std::sync::Arc;
use vtcode::tools::{
    ObservabilityContext,
    EventType,
    ImprovementError,
    ErrorKind,
};

// Create observability context with logging
let obs = Arc::new(ObservabilityContext::logging());

// Record an event
obs.event(
    EventType::ToolSelected,
    "selector",
    "selected grep_file based on pattern match",
    Some(0.95),
);

// Record a metric
obs.metric("similarity", "jaro_winkler", 0.87);

// Record an error
let err = ImprovementError::new(
    ErrorKind::SelectionFailed,
    "no viable candidates",
    "select_tool",
).with_severity(ErrorSeverity::Warning);

obs.error(&err);
```

## Configuration File Example

Create `vtcode.toml`:

```toml
[similarity]
min_similarity_threshold = 0.6
high_similarity_threshold = 0.8
argument_weight = 0.4
return_type_weight = 0.3
description_weight = 0.2
success_history_weight = 0.1

[time_decay]
decay_constant = 0.1
half_life_hours = 24.0
minimum_score = 0.1
recent_window_hours = 1.0

[patterns]
min_sequence_length = 3
pattern_window_seconds = 300
confidence_threshold = 0.75
max_patterns = 100
enable_advanced_detection = true

[cache]
max_entries = 10000
ttl = 3600
enable_result_cache = true
enable_metadata_cache = true
enable_pattern_cache = true

[context]
max_context_tokens = 100000
truncation_threshold_percent = 85.0
enable_compaction = true
max_history_entries = 100

[fallback]
max_attempts = 3
backoff_multiplier = 2.0
initial_backoff_ms = 100
max_backoff_ms = 5000
enable_exponential_backoff = true
```

## Real-World Scenarios

### Scenario 1: Improving Tool Selection

```rust
// User is trying to find files
let tools = vec!["grep_file", "find_file", "read_file"];

// Compute similarities to "grep_pattern"
let target = "grep_pattern";

let mut scores: Vec<_> = tools
    .iter()
    .map(|&tool| {
        let sim = jaro_winkler_similarity(tool, target);
        (tool, sim)
    })
    .collect();

// Sort by similarity
scores.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());

println!("Best match: {} (similarity: {})", scores[0].0, scores[0].1);
```

### Scenario 2: Detecting User Frustration

```rust
let detector = PatternDetector::new(10);

// Simulating user action history
let history = vec![
    ("grep_file".to_string(), "pattern1".to_string(), 0.5),
    ("grep_file".to_string(), "pattern2".to_string(), 0.4),
    ("grep_file".to_string(), "pattern3".to_string(), 0.3),
];

match detector.detect(&history) {
    PatternState::Degradation => {
        println!("🚨 User is frustrated - results are getting worse");
        println!("Suggestion: Try a different approach");
    }
    _ => {}
}
```

### Scenario 3: Caching with Observability

```rust
let obs = Arc::new(ObservabilityContext::logging());
let cache = CachingMiddleware::new();

let request = ToolRequest {
    tool_name: "expensive_tool".to_string(),
    arguments: "complex_search".to_string(),
    context: "codebase".to_string(),
    metadata: RequestMetadata::default(),
};

// First call - cache miss
let result1 = cache.execute(
    request.clone(),
    Box::new(|_| MiddlewareResult {
        success: true,
        result: Some("result".to_string()),
        error: None,
        metadata: Default::default(),
    }),
);

if !result1.metadata.from_cache {
    obs.event(
        EventType::CacheMiss,
        "cache",
        "no cached result for expensive_tool",
        None,
    );
}

// Second call - cache hit (fast!)
let result2 = cache.execute(
    request,
    Box::new(|_| MiddlewareResult {
        success: true,
        result: Some("new_result".to_string()),
        error: None,
        metadata: Default::default(),
    }),
);

if result2.metadata.from_cache {
    obs.event(
        EventType::CacheHit,
        "cache",
        "returned cached result (0ms)",
        Some(1.0),
    );
}
```

## Performance Tips

1. **Use Configuration**: Tune thresholds for your workload
2. **Enable Caching**: Cache expensive tool results
3. **Monitor Events**: Use observability to identify bottlenecks
4. **Pattern Detection**: Helps identify stuck loops early
5. **Middleware Ordering**: Logging → Caching → Retry

## Troubleshooting

### "Configuration validation failed"

```rust
match config.validate() {
    Ok(_) => println!("Config is valid"),
    Err(e) => println!("Error: {}", e),
}
```

### "Similarity score too low"

Adjust `min_similarity_threshold`:

```rust
let mut config = ImprovementsConfig::default();
config.similarity.min_similarity_threshold = 0.5;  // More lenient
```

### "Cache not working"

Ensure `enable_result_cache = true` in config and check cache keys match exactly.

### "Too many middleware layers"

Each middleware adds overhead. For performance-critical paths, use only essential middleware.

## Next Steps

- Read [PRODUCTION_GRADE_IMPROVEMENTS.md](PRODUCTION_GRADE_IMPROVEMENTS.md) for deep dive
- Check integration tests: `cargo test --lib tools::improvements_integration_tests`
- Configure for your use case based on config examples
- Monitor with observability sinks
