# Optimization: Efficient Input Pausing ⚡

## Overview

In response to the best practices note regarding external process handling, we have optimized the `InputListener` to use **blocking synchronization** instead of polling when paused.

## The Change

Previously, when the TUI was suspended (e.g., while Vim was open), the background thread would sleep in a loop:

```rust
if paused {
    std::thread::sleep(Duration::from_millis(10)); // Busy-ish wait
    continue;
}
```

We have replaced this with a blocking receive:

```rust
if paused {
    // Blocks the thread completely until a message arrives
    // Zero CPU usage
    if let Ok(control) = control_rx.recv() {
        // ... handle resume ...
    }
}
```

## Alignment with Best Practices

1.  **Relinquish Control**: The input thread now completely yields execution, ensuring absolutely no interference with the external process.
2.  **Resource Efficiency**: Zero CPU cycles are wasted checking for resume signals while the external app is running.
3.  **Responsiveness**: Resuming is instantaneous upon receiving the signal, rather than waiting for the next sleep cycle to finish.

This complements our existing "Drain Events" fix (for ANSI artifacts) and "Suspend/Resume" architecture, providing a robust and efficient integration with external terminal applications.
