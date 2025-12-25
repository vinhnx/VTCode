# Streaming Timeout Configuration

## Overview

VT Code uses configurable timeouts to prevent long-running LLM streaming requests from consuming resources indefinitely. The `streaming_ceiling_seconds` configuration controls the maximum time the system will wait for an LLM provider to stream a response.

## Default Configuration

```toml
[timeouts]
# Maximum duration for streaming API responses in seconds. Set to 0 to disable.
# Increase this if you encounter streaming timeouts with long inputs or slow networks
streaming_ceiling_seconds = 600  # 10 minutes
```

## When You Encounter a Streaming Timeout

If you see an error like:

```
Streaming request timeout after 333s. Try reducing input length or increasing timeout in config.

Streaming timeout troubleshooting:
- Reduce input length or complexity
- Increase timeout in config: [timeouts] streaming_ceiling_seconds
- Check network connectivity
- Check network stability for streaming connections
- Consider using non-streaming mode for very long inputs
```

### Solutions

#### 1. Increase the Timeout (Recommended for slow networks)

Edit `vtcode.toml` and increase the `streaming_ceiling_seconds` value:

```toml
[timeouts]
streaming_ceiling_seconds = 900  # 15 minutes
```

Then restart VT Code for the changes to take effect.

#### 2. Reduce Input Length

-   Break large codebase analyses into smaller chunks
-   Summarize large files before asking questions about them
-   Use more specific search queries to reduce context

#### 3. Check Network Connectivity

-   Verify your internet connection is stable
-   Check if your network has latency issues
-   Try running in a different network environment
-   Look for firewall or proxy issues that might slow streaming

#### 4. Monitor Streaming Progress

VT Code will warn you when streaming operations approach the timeout limit (at 80% by default). If you see warnings frequently, consider increasing the timeout.

## Configuration Options

### `streaming_ceiling_seconds` (integer, default: 600)

Maximum time in seconds to wait for an LLM streaming response.

-   **0**: Disables the timeout entirely (not recommended)
-   **300-600**: Good for typical use cases and standard network conditions
-   **900-1800**: Recommended for large codebases, complex analyses, or slow networks
-   **>1800**: Use only if you consistently hit timeouts and have a reliable connection

### Warning Threshold

When combined with the global `warning_threshold_percent` setting, VT Code will emit a warning once streaming exceeds 80% of the timeout:

```toml
[timeouts]
streaming_ceiling_seconds = 600
warning_threshold_percent = 80  # Warn at 480 seconds (80% of 600)
```

## Troubleshooting Guide

### Timeout occurs consistently with large inputs

**Solution**: Increase `streaming_ceiling_seconds` and reduce input complexity:

```toml
streaming_ceiling_seconds = 1200  # 20 minutes
```

### Timeout occurs on slow/unreliable networks

**Solution**: Increase timeout and ensure stable connection:

```toml
streaming_ceiling_seconds = 1500  # 25 minutes
```

### Timeout occurs sometimes but not always

**Symptom**: The same query works occasionally but times out other times

**Solution**: This typically indicates network instability. Try:

1. Increase timeout slightly: `streaming_ceiling_seconds = 800`
2. Retry during periods of stable network
3. Check for background network activity consuming bandwidth

### Streaming completes successfully but slowly

**Symptom**: Sees warning at 80% but eventually completes

**Solution**: You can safely increase the warning threshold or just monitor:

```toml
warning_threshold_percent = 85  # Warn at 85% instead of 80%
```

## Performance Impact

-   **Higher timeouts**: No performance impact during normal operation, only affects how long errors take to report
-   **Disabled timeout (0)**: Requests could potentially hang indefinitely without feedback
-   **Very low timeouts (<300s)**: May cause false timeouts on slower networks

## Related Configuration

See also:

-   `[timeouts] default_ceiling_seconds` - For standard (non-streaming) tool timeouts
-   `[timeouts] pty_ceiling_seconds` - For terminal command timeouts
-   `[timeouts] warning_threshold_percent` - For timeout warning threshold
