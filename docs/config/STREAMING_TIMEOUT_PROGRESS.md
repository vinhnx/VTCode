# Streaming Timeout Progress Handling

## Overview

Enhanced streaming timeout handling with real-time progress reporting. This allows clients to monitor how close a streaming operation is to its timeout limit and warn users before failure occurs.

## Features

### 1. Progress Callback Interface

The `StreamingProcessor` now supports a progress callback that receives real-time updates (0.0-1.0) representing how much of the timeout has elapsed:

```rust
pub type ProgressCallback = Box<dyn Fn(f32) + Send + Sync>;
```

**Progress Scale:**
- `0.0` = Operation just started
- `0.1` = First chunk received
- `0.5` = Halfway through timeout
- `0.8` = Approaching warning threshold (default)
- `0.99` = Critical - near timeout (clamped)
- `1.0` = Timeout or error occurred

### 2. Configurable Warning Threshold

Set when streaming operations should trigger warnings before actual timeout:

```rust
let processor = StreamingProcessor::new()
    .with_warning_threshold(0.8); // Warn at 80% of timeout
```

**Default:** 80% of total timeout

### 3. Automatic Warning Logging

When progress approaches the warning threshold, automatic warnings are logged:

```
WARN Streaming operation at 80% of timeout limit (480/600s elapsed). Approaching timeout.
```

This gives users and operators visibility into potential timeouts before they occur.

## Usage

### Basic Setup

```rust
use vtcode_core::gemini::streaming::{StreamingProcessor, StreamingConfig};
use std::time::Duration;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;

let timeout_progress = Arc::new(AtomicU8::new(0));
let progress_clone = timeout_progress.clone();

let processor = StreamingProcessor::new()
    .with_progress_callback(Box::new(move |progress: f32| {
        let percent = (progress * 100.0) as u8;
        progress_clone.store(percent, Ordering::Relaxed);
    }))
    .with_warning_threshold(0.8);

// Use processor as normal
```

### UI Integration

Show timeout progress in the UI:

```rust
let progress_percent = timeout_progress.load(Ordering::Relaxed);
if progress_percent >= 80 {
    println!("  Operation approaching timeout: {}% elapsed", progress_percent);
}
```

### Monitoring

Log progress updates during streaming:

```rust
let processor = StreamingProcessor::new()
    .with_progress_callback(Box::new(|progress: f32| {
        if progress > 0.0 && progress % 0.1 == 0.0 {
            eprintln!("Streaming progress: {:.0}%", progress * 100.0);
        }
    }))
    .with_warning_threshold(0.75);
```

## Implementation Details

### Progress Reporting Points

1. **After first chunk** (0.1): Initial progress indication
2. **During chunk processing** (continuous): Reported at each chunk
3. **At warning threshold**: Automatic warning logged
4. **At timeout/error** (1.0): Final report

### Clamping Logic

Progress is clamped at 99% during the stream to prevent the callback from receiving 1.0 until the operation actually completes or times out. This prevents UI premature completion indicators.

### No-Timeout Handling

When `total_timeout` is 0 (unlimited), progress reporting is skipped to avoid division by zero.

## Configuration

### In `vtcode.toml`

```toml
[streaming]
total_timeout_seconds = 600
warning_threshold_percent = 80

[timeouts]
streaming_ceiling_seconds = 600
warning_threshold_percent = 80
```

### Programmatically

```rust
let config = StreamingConfig {
    chunk_timeout: Duration::from_secs(30),
    first_chunk_timeout: Duration::from_secs(60),
    total_timeout: Duration::from_secs(600),
    buffer_size: 1024,
};

let processor = StreamingProcessor::with_config(config)
    .with_warning_threshold(0.80);
```

## Error Reporting

When a timeout occurs, the progress callback receives `1.0` before the error is returned:

```rust
return Err(StreamingError::TimeoutError {
    operation: "streaming".to_string(),
    duration: elapsed,
});
```

This ensures the UI can show timeout state appropriately.

## Performance Considerations

- **Minimal overhead**: Progress calculation is O(1) per chunk
- **Lock-free**: Uses atomic operations for thread-safe reporting
- **Non-blocking**: Callback execution doesn't block streaming
- **Efficient**: Only logs warnings at threshold, not per chunk

## Testing

```rust
#[tokio::test]
async fn test_streaming_progress_callback() {
    let progress = Arc::new(Mutex::new(0.0));
    let progress_clone = progress.clone();

    let processor = StreamingProcessor::new()
        .with_progress_callback(Box::new(move |p: f32| {
            let mut prog = progress_clone.blocking_lock();
            *prog = p;
        }));

    // Test with real streaming...
}
```

## Related

- `docs/config/STREAMING_TIMEOUT.md` - Timeout configuration
- `vtcode-core/src/gemini/streaming/processor.rs` - Implementation
