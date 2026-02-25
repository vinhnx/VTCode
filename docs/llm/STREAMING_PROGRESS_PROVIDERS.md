# Streaming Timeout Progress - Provider Integration Guide

## Overview

The `StreamingProgressTracker` in `vtcode-core/src/llm/providers/streaming_progress.rs` provides a unified interface for tracking streaming timeout progress across all LLM providers.

**Supported Providers:**
-   OpenAI (GPT-4, o1, o1-mini)
-   Anthropic (Claude)
-   Google Gemini
-   Ollama
-   OpenRouter
-   Minimax (via Anthropic wrapper)
-   LM Studio (via OpenAI wrapper)
-   DeepSeek
-   Z.AI
-   xAI (Grok)
-   Moonshot

## Quick Start

### Basic Usage

```rust
use vtcode_core::llm::providers::StreamingProgressBuilder;
use std::time::Duration;

// Create a tracker with 10-minute timeout
let tracker = StreamingProgressBuilder::new(600)
    .warning_threshold(0.80)
    .callback(Box::new(|progress: f32| {
        println!("Progress: {:.0}%", progress * 100.0);
    }))
    .build();

// During streaming:
tracker.report_first_chunk();           // 0.1 (10%)
tracker.report_chunk_received();        // Updates based on elapsed time
// ... more chunks ...
tracker.report_error();                 // 1.0 (100%)
```

## Integration Patterns

### Pattern 1: OpenAI Streaming

```rust
use vtcode_core::llm::providers::StreamingProgressBuilder;
use std::time::Duration;

async fn stream_with_progress(
    client: &HttpClient,
    timeout_secs: u64,
) -> Result<String> {
    let tracker = StreamingProgressBuilder::new(timeout_secs)
        .callback(Box::new(|progress: f32| {
            if progress >= 0.8 {
                eprintln!("  Streaming approaching timeout: {:.0}%", progress * 100.0);
            }
        }))
        .build();

    let mut response = String::new();
    
    // Start streaming
    let stream = client.stream_response().await?;
    tracker.report_first_chunk();

    for chunk in stream {
        tracker.report_chunk_received();
        response.push_str(&chunk?);
        
        if tracker.is_approaching_timeout() {
            eprintln!("Timeout imminent!");
            break;
        }
    }

    Ok(response)
}
```

### Pattern 2: Anthropic Streaming

```rust
use vtcode_core::llm::providers::StreamingProgressTracker;
use std::time::Duration;

async fn anthropic_stream_with_progress() {
    let tracker = StreamingProgressTracker::new(Duration::from_secs(600))
        .with_warning_threshold(0.75);

    // Setup streaming
    tracker.report_first_chunk();

    // For each streaming message event:
    loop {
        match event_stream.next().await {
            Some(event) => {
                tracker.report_chunk_received();
                process_event(event);
            }
            None => {
                tracker.report_progress_with_elapsed(tracker.elapsed());
                break;
            }
        }
    }
}
```

### Pattern 3: Gemini Streaming

```rust
use vtcode_core::llm::providers::StreamingProgressBuilder;

let tracker = StreamingProgressBuilder::with_duration(
    Duration::from_secs(600)
)
.callback(Box::new(move |progress: f32| {
    progress_atomic.store((progress * 100.0) as u8, Ordering::Relaxed);
}))
.build();

// In the streaming response handler:
tracker.report_first_chunk();
while let Some(chunk) = stream.next().await {
    tracker.report_chunk_received();
    process_chunk(chunk);
}
```

### Pattern 4: Ollama Streaming

```rust
let tracker = StreamingProgressBuilder::new(600)
    .warning_threshold(0.85)  // Warn at 85%
    .callback(Box::new(|p: f32| {
        ui.update_progress(p);
    }))
    .build();

// Ollama streaming loop
for line in response.lines() {
    tracker.report_chunk_received();
    let delta = parse_line(line)?;
    output.push_str(&delta);
}
```

## Configuration

### From Config File

```toml
[streaming]
# Total timeout for streaming operations (seconds)
timeout = 600

# Warning threshold (0.0-1.0, default 0.8)
warning_threshold = 0.8

[timeouts]
# Alternative config path
streaming_ceiling_seconds = 600
warning_threshold_percent = 80
```

### Programmatic

```rust
let tracker = StreamingProgressBuilder::new(
    config.streaming.timeout_secs
)
.warning_threshold(
    config.streaming.warning_threshold as f32
)
.callback(progress_callback)
.build();
```

## Monitoring and Observability

### Progress Logging

```rust
let tracker = StreamingProgressBuilder::new(600)
    .callback(Box::new(|progress: f32| {
        // Log at 10% intervals
        let percent = (progress * 100.0) as i32;
        if percent % 10 == 0 && percent > 0 {
            tracing::info!("Streaming progress: {}%", percent);
        }
    }))
    .build();
```

### UI Updates

```rust
let progress_state = Arc::new(AtomicU8::new(0));
let progress_clone = progress_state.clone();

let tracker = StreamingProgressBuilder::new(timeout)
    .callback(Box::new(move |p: f32| {
        let percent = (p * 100.0) as u8;
        progress_clone.store(percent, Ordering::Relaxed);
    }))
    .build();

// UI thread reads progress_state regularly
```

### Metrics Collection

```rust
#[derive(Default)]
struct StreamingMetrics {
    progress_checkpoints: Vec<u8>,
    warnings_triggered: bool,
    timeout_occurred: bool,
}

let metrics = Arc::new(Mutex::new(StreamingMetrics::default()));
let metrics_clone = metrics.clone();

let tracker = StreamingProgressBuilder::new(600)
    .callback(Box::new(move |p: f32| {
        let mut m = metrics_clone.lock().unwrap();
        let percent = (p * 100.0) as u8;
        
        if percent >= 80 {
            m.warnings_triggered = true;
        }
        
        m.progress_checkpoints.push(percent);
    }))
    .build();
```

## Error Handling

### Timeout Detection

```rust
if tracker.is_approaching_timeout() {
    // Graceful degradation
    log_warning("Streaming approaching timeout, returning partial results");
    return Ok(accumulated_response);
}
```

### Automatic Warnings

The tracker automatically logs warnings when approaching threshold:

```
WARN Streaming operation at 80% of timeout limit (480s/600s elapsed). Approaching timeout.
```

### Custom Error Handling

```rust
let tracker = StreamingProgressBuilder::new(600).build();

match streaming_operation().await {
    Ok(result) => {
        tracker.report_progress_with_elapsed(tracker.elapsed());
        Ok(result)
    }
    Err(e) => {
        tracker.report_error();  // Reports 100%
        tracing::error!(
            "Streaming failed at {}% progress: {}",
            tracker.progress_percent(),
            e
        );
        Err(e)
    }
}
```

## Best Practices

### 1. Create Tracker at Stream Start

```rust
//   Good
let tracker = StreamingProgressBuilder::new(timeout).build();
stream_start();

//   Bad - creates tracker after streaming begins
stream_start();
let tracker = StreamingProgressBuilder::new(timeout).build();
```

### 2. Report at Key Points

```rust
//   Good progression reporting
tracker.report_first_chunk();           // First signal
tracker.report_chunk_received();        // Each chunk
tracker.report_error();                 // On failure
```

### 3. Use Appropriate Thresholds

```rust
// Short operations: warn later
let quick = StreamingProgressBuilder::new(30)
    .warning_threshold(0.9)     // 27 seconds
    .build();

// Long operations: warn earlier
let long = StreamingProgressBuilder::new(3600)
    .warning_threshold(0.7)     // 2520 seconds (42 min)
    .build();
```

### 4. Handle Zero Timeouts

```rust
// For unlimited/unconfigured timeouts
let tracker = if timeout > 0 {
    StreamingProgressBuilder::new(timeout).build()
} else {
    StreamingProgressBuilder::new(u64::MAX).build()
};
```

## Testing

```rust
#[tokio::test]
async fn test_streaming_progress() {
    let progress_log = Arc::new(Mutex::new(Vec::new()));
    let progress_clone = progress_log.clone();

    let tracker = StreamingProgressBuilder::new(100)
        .callback(Box::new(move |p: f32| {
            progress_clone.lock().unwrap().push(p);
        }))
        .build();

    // Simulate chunks
    for i in 1..=10 {
        tracker.report_progress_with_elapsed(
            Duration::from_secs(i * 10)
        );
    }

    let log = progress_log.lock().unwrap();
    assert!(!log.is_empty());
    assert!(log.iter().all(|&p| p >= 0.0 && p <= 1.0));
}
```

## Troubleshooting

### Progress Not Updating

**Check:**
1. Is `report_chunk_received()` being called?
2. Is timeout > 0?
3. Is callback registered?

### Warnings Not Appearing

**Check:**
1. Is warning threshold set correctly?
2. Is logging enabled at WARN level?
3. Is tracker being used in streaming context?

### High Memory Usage

**Check:**
1. Are trackers being created in tight loops?
2. Are callbacks holding onto resources?
3. Consider cleanup strategy for completed operations

## Related

- `vtcode-core/src/llm/providers/streaming_progress.rs` - Implementation
- `docs/config/STREAMING_TIMEOUT_PROGRESS.md` - Gemini-specific details
