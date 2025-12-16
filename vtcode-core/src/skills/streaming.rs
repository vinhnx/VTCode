//! Skill Execution Streaming
//!
//! Provides streaming support for long-running skill executions,
//! enabling real-time progress updates and partial results.

use crate::skills::cli_bridge::{CliToolBridge, CliToolResult};
use anyhow::{Result, anyhow};
use async_stream::stream;
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::pin::Pin;
use std::process::Stdio;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, BufReader};
use tokio::process::Command as TokioCommand;
use tokio::time::{interval, timeout};

/// Streaming execution configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingConfig {
    /// Enable streaming output
    pub enable_streaming: bool,

    /// Buffer size for reading output
    pub buffer_size: usize,

    /// Update interval for progress reports
    pub update_interval_ms: u64,

    /// Maximum execution time
    pub max_execution_time_secs: u64,

    /// Enable partial JSON parsing
    pub enable_partial_json: bool,

    /// Enable progress reporting
    pub enable_progress_reporting: bool,

    /// Include stderr in stream
    pub include_stderr: bool,

    /// Split output by lines
    pub line_based_streaming: bool,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            enable_streaming: true,
            buffer_size: 8192,
            update_interval_ms: 100,
            max_execution_time_secs: 300, // 5 minutes
            enable_partial_json: true,
            enable_progress_reporting: true,
            include_stderr: true,
            line_based_streaming: true,
        }
    }
}

/// Streaming execution event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    /// Progress update
    Progress {
        /// Progress percentage (0-100)
        percentage: f32,
        /// Status message
        message: String,
        /// Elapsed time
        elapsed_ms: u64,
        /// Estimated remaining time
        estimated_remaining_ms: Option<u64>,
    },
    /// Output chunk
    Output {
        /// Output data
        data: String,
        /// Output type (stdout/stderr)
        output_type: OutputType,
        /// Whether this is partial output
        is_partial: bool,
    },
    /// JSON object detected
    JsonObject {
        /// Parsed JSON value
        value: Value,
        /// Raw JSON string
        raw: String,
    },
    /// Execution completed
    Completed {
        /// Final exit code
        exit_code: i32,
        /// Total execution time
        total_time_ms: u64,
        /// Final result
        result: Option<CliToolResult>,
    },
    /// Error occurred
    Error {
        /// Error message
        message: String,
        /// Whether execution should continue
        fatal: bool,
    },
    /// Execution started
    Started {
        /// Command being executed
        command: String,
        /// Arguments
        args: Vec<String>,
        /// Start time
        start_time: chrono::DateTime<chrono::Utc>,
    },
}

/// Output type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OutputType {
    Stdout,
    Stderr,
}

/// Streaming skill executor
pub struct StreamingSkillExecutor {
    config: StreamingConfig,
}

impl StreamingSkillExecutor {
    /// Create new streaming executor with default configuration
    pub fn new() -> Self {
        Self::with_config(StreamingConfig::default())
    }

    /// Create new streaming executor with custom configuration
    pub fn with_config(config: StreamingConfig) -> Self {
        Self { config }
    }

    /// Execute CLI tool with streaming output
    pub fn execute_cli_tool_streaming(
        &self,
        bridge: &CliToolBridge,
        args: Value,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>> {
        let config = self.config.clone();
        let bridge = bridge.clone();
        let args = args.clone();

        Box::pin(stream! {
            let start_time = Instant::now();
            let start_datetime = chrono::Utc::now();

            // Build command
            let mut cmd = TokioCommand::new(&bridge.config.executable_path);

            // Set working directory
            if let Some(working_dir) = &bridge.config.working_dir {
                cmd.current_dir(working_dir);
            }

            // Set environment variables
            if let Some(env) = &bridge.config.environment {
                for (key, value) in env {
                    cmd.env(key, value);
                }
            }

            // Configure I/O for streaming
            cmd.stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .kill_on_drop(true);

            // Add arguments
            if let Err(e) = Self::configure_arguments(&mut cmd, &args) {
                yield Err(anyhow!("Failed to configure arguments: {}", e));
                return;
            }

            // Get command string for logging
            let _command_str = format!("{:?}", cmd);
            let args: Vec<String> = cmd.as_std().get_args()
                .map(|arg| arg.to_string_lossy().to_string())
                .collect();

            // Emit started event
            yield Ok(StreamEvent::Started {
                command: bridge.config.executable_path.display().to_string(),
                args,
                start_time: start_datetime,
            });

            // Start the process
            let mut child = match cmd.spawn() {
                Ok(child) => child,
                Err(e) => {
                    yield Err(anyhow!("Failed to spawn process: {}", e));
                    return;
                }
            };

            let stdout = match child.stdout.take() {
                Some(stdout) => stdout,
                None => {
                    yield Err(anyhow!("Failed to capture stdout"));
                    return;
                }
            };

            let stderr = if config.include_stderr {
                child.stderr.take()
            } else {
                None
            };

            // Create progress tracking
            let progress_tracker = ProgressTracker::new(config.update_interval_ms);
            let mut progress_interval = interval(Duration::from_millis(config.update_interval_ms));

            // Stream stdout
            let stdout_stream = Self::stream_output(
                stdout,
                OutputType::Stdout,
                config.clone(),
                progress_tracker.clone(),
            );

            // Stream stderr if enabled
            let stderr_stream = if let Some(stderr) = stderr {
                Some(Self::stream_output(
                    stderr,
                    OutputType::Stderr,
                    config.clone(),
                    progress_tracker.clone(),
                ))
            } else {
                None
            };

            // Combine streams
            let mut stdout_stream = Box::pin(stdout_stream);
            let mut stderr_stream = stderr_stream.map(|s| Box::pin(s));

            // Stream events
            let mut output_buffer = String::new();
            let mut json_buffer = String::new();
            let _is_parsing_json = false;

            loop {
                tokio::select! {
                    // Progress updates
                    _ = progress_interval.tick() => {
                        let progress = progress_tracker.get_progress();
                        yield Ok(StreamEvent::Progress {
                            percentage: progress.percentage,
                            message: progress.message.clone(),
                            elapsed_ms: start_time.elapsed().as_millis() as u64,
                            estimated_remaining_ms: progress.estimated_remaining_ms,
                        });
                    }

                    // Stdout events
                    Some(event) = stdout_stream.next() => {
                        match event {
                            Ok(StreamEvent::Output { data, output_type, is_partial }) => {
                                output_buffer.push_str(&data);

                                // Try to detect JSON objects
                                if config.enable_partial_json {
                                    if let Some(json_events) = Self::extract_json_objects(&mut json_buffer, &data) {
                                        for json_event in json_events {
                                            yield Ok(json_event);
                                        }
                                    }
                                }

                                yield Ok(StreamEvent::Output {
                                    data,
                                    output_type,
                                    is_partial,
                                });
                            }
                            Ok(event) => yield Ok(event),
                            Err(e) => {
                                yield Err(anyhow!("Stdout stream error: {}", e));
                                break;
                            }
                        }
                    }

                    // Stderr events
                    Some(event_result) = async {
                        match &mut stderr_stream {
                            Some(stream) => stream.next().await,
                            None => None,
                        }
                    } => {
                        match event_result {
                            Ok(StreamEvent::Output { data, output_type, is_partial }) => {
                                yield Ok(StreamEvent::Output {
                                    data,
                                    output_type,
                                    is_partial,
                                });
                            }
                            Ok(event) => yield Ok(event),
                            Err(e) => {
                                yield Err(anyhow!("Stderr stream error: {}", e));
                            }
                        }
                    }

                    // Process completion
                    else => {
                        break;
                    }
                }

                // Check timeout
                if start_time.elapsed().as_secs() > config.max_execution_time_secs {
                    let _ = child.kill().await;
                    yield Err(anyhow!("Execution timed out after {} seconds", config.max_execution_time_secs));
                    return;
                }
            }

            // Wait for process completion
            let exit_status = match timeout(
                Duration::from_secs(config.max_execution_time_secs),
                child.wait()
            ).await {
                Ok(Ok(status)) => status,
                Ok(Err(e)) => {
                    yield Err(anyhow!("Failed to wait for process: {}", e));
                    return;
                }
                Err(_) => {
                    yield Err(anyhow!("Process wait timed out"));
                    return;
                }
            };

            let exit_code = exit_status.code().unwrap_or(-1);
            let total_time_ms = start_time.elapsed().as_millis() as u64;

            // Create final result
            let result = CliToolResult {
                exit_code,
                stdout: output_buffer.clone(),
                stderr: String::new(), // Would need to capture stderr separately
                json_output: None,
                execution_time_ms: total_time_ms,
            };

            yield Ok(StreamEvent::Completed {
                exit_code,
                total_time_ms,
                result: Some(result),
            });
        })
    }

    /// Stream output from a reader
    fn stream_output<R>(
        reader: R,
        output_type: OutputType,
        config: StreamingConfig,
        mut progress_tracker: ProgressTracker,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>
    where
        R: AsyncReadExt + Send + Unpin + 'static,
    {
        Box::pin(stream! {
            let mut reader = BufReader::new(reader);
            let mut buffer = vec![0u8; config.buffer_size];
            let mut line_buffer = String::new();

            loop {
                match reader.read(&mut buffer).await {
                    Ok(0) => {
                        // EOF reached
                        if !line_buffer.is_empty() && config.line_based_streaming {
                            progress_tracker.update_with_output(&line_buffer);
                            yield Ok(StreamEvent::Output {
                                data: line_buffer.clone(),
                                output_type: output_type.clone(),
                                is_partial: false,
                            });
                        }
                        break;
                    }
                    Ok(n) => {
                        let data = String::from_utf8_lossy(&buffer[..n]);

                        if config.line_based_streaming {
                            line_buffer.push_str(&data);

                            // Process complete lines
                            while let Some(line_end) = line_buffer.find('\n') {
                                let line = line_buffer[..line_end + 1].to_string();
                                line_buffer.drain(..line_end + 1);

                                progress_tracker.update_with_output(&line);
                                yield Ok(StreamEvent::Output {
                                    data: line,
                                    output_type: output_type.clone(),
                                    is_partial: false,
                                });
                            }
                        } else {
                            // Stream raw data
                            progress_tracker.update_with_output(&data);
                            yield Ok(StreamEvent::Output {
                                data: data.to_string(),
                                output_type: output_type.clone(),
                                is_partial: true,
                            });
                        }
                    }
                    Err(e) => {
                        yield Err(anyhow!("Read error: {}", e));
                        break;
                    }
                }
            }
        })
    }

    /// Extract JSON objects from output
    fn extract_json_objects(json_buffer: &mut String, new_data: &str) -> Option<Vec<StreamEvent>> {
        json_buffer.push_str(new_data);

        let mut events = vec![];

        // Try to find complete JSON objects
        while let Some(brace_start) = json_buffer.find('{') {
            let mut brace_count = 0;
            let mut end_pos = None;

            for (i, ch) in json_buffer[brace_start..].chars().enumerate() {
                match ch {
                    '{' => brace_count += 1,
                    '}' => {
                        brace_count -= 1;
                        if brace_count == 0 {
                            end_pos = Some(brace_start + i + 1);
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if let Some(end) = end_pos {
                let json_str = &json_buffer[brace_start..end];

                if let Ok(value) = serde_json::from_str::<Value>(json_str) {
                    events.push(StreamEvent::JsonObject {
                        value,
                        raw: json_str.to_string(),
                    });

                    // Remove processed JSON from buffer
                    json_buffer.drain(..end);
                } else {
                    // Invalid JSON, skip this object
                    json_buffer.drain(..brace_start + 1);
                }
            } else {
                // Incomplete JSON, keep in buffer
                break;
            }
        }

        if events.is_empty() {
            None
        } else {
            Some(events)
        }
    }

    /// Configure command arguments from JSON
    fn configure_arguments(cmd: &mut TokioCommand, args: &Value) -> Result<()> {
        if args.is_null() {
            return Ok(());
        }

        match args {
            Value::String(s) => {
                cmd.arg(s);
            }
            Value::Array(arr) => {
                for arg in arr {
                    if let Some(s) = arg.as_str() {
                        cmd.arg(s);
                    }
                }
            }
            Value::Object(map) => {
                for (key, value) in map {
                    if let Some(s) = value.as_str() {
                        cmd.arg(format!("--{}", key));
                        cmd.arg(s);
                    } else if value.is_boolean() && value.as_bool().unwrap() {
                        cmd.arg(format!("--{}", key));
                    }
                }
            }
            _ => {
                let json_str = serde_json::to_string(args)?;
                cmd.arg(json_str);
            }
        }

        Ok(())
    }
}

/// Progress tracking for streaming execution
#[derive(Debug, Clone)]
pub struct ProgressTracker {
    start_time: Instant,
    #[allow(dead_code)]
    update_interval_ms: u64,
    total_output_bytes: usize,
    last_output_time: Instant,
    estimated_total_bytes: Option<usize>,
}

impl ProgressTracker {
    /// Create new progress tracker
    pub fn new(update_interval_ms: u64) -> Self {
        Self {
            start_time: Instant::now(),
            update_interval_ms,
            total_output_bytes: 0,
            last_output_time: Instant::now(),
            estimated_total_bytes: None,
        }
    }

    /// Update with new output
    pub fn update_with_output(&mut self, output: &str) {
        self.total_output_bytes += output.len();
        self.last_output_time = Instant::now();

        // Simple heuristic: estimate total based on output rate
        if self.estimated_total_bytes.is_none() && self.start_time.elapsed().as_secs() > 5 {
            let elapsed_secs = self.start_time.elapsed().as_secs().max(1);
            let bytes_per_second = self.total_output_bytes / elapsed_secs as usize;

            // Estimate 2-5 minutes of output at current rate
            self.estimated_total_bytes = Some(bytes_per_second * 180); // 3 minutes
        }
    }

    /// Get current progress
    pub fn get_progress(&self) -> ProgressInfo {
        let elapsed_ms = self.start_time.elapsed().as_millis() as u64;
        let percentage = if let Some(estimated) = self.estimated_total_bytes {
            if estimated > 0 {
                ((self.total_output_bytes as f32 / estimated as f32) * 100.0).min(95.0)
            } else {
                0.0
            }
        } else {
            // Time-based progress if no size estimate
            let estimated_total_ms = 300_000; // 5 minutes
            ((elapsed_ms as f32 / estimated_total_ms as f32) * 100.0).min(95.0)
        };

        let estimated_remaining_ms = if let Some(estimated) = self.estimated_total_bytes {
            if self.total_output_bytes > 0 {
                let bytes_remaining = estimated.saturating_sub(self.total_output_bytes);
                let bytes_per_ms = self.total_output_bytes as f32 / elapsed_ms as f32;
                Some((bytes_remaining as f32 / bytes_per_ms) as u64)
            } else {
                None
            }
        } else {
            None
        };

        let message = if percentage < 10.0 {
            "Starting execution...".to_string()
        } else if percentage < 50.0 {
            "Processing...".to_string()
        } else if percentage < 90.0 {
            "Almost complete...".to_string()
        } else {
            "Finalizing...".to_string()
        };

        ProgressInfo {
            percentage,
            message,
            elapsed_ms,
            estimated_remaining_ms,
        }
    }
}

/// Progress information
#[derive(Debug, Clone)]
pub struct ProgressInfo {
    /// Progress percentage (0-100)
    pub percentage: f32,

    /// Status message
    pub message: String,

    /// Elapsed time in milliseconds
    pub elapsed_ms: u64,

    /// Estimated remaining time
    pub estimated_remaining_ms: Option<u64>,
}

/// Extension trait for streaming execution
pub trait StreamingExecution {
    /// Execute with streaming output
    fn execute_streaming(
        &self,
        args: Value,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>>;
}

impl StreamingExecution for CliToolBridge {
    fn execute_streaming(
        &self,
        args: Value,
    ) -> Pin<Box<dyn Stream<Item = Result<StreamEvent>> + Send>> {
        let executor = StreamingSkillExecutor::new();
        executor.execute_cli_tool_streaming(self, args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_config_default() {
        let config = StreamingConfig::default();
        assert!(config.enable_streaming);
        assert_eq!(config.buffer_size, 8192);
        assert_eq!(config.max_execution_time_secs, 300);
    }

    #[test]
    fn test_progress_tracker() {
        let mut tracker = ProgressTracker::new(100);

        // Initial progress
        let progress = tracker.get_progress();
        assert!(progress.percentage >= 0.0 && progress.percentage <= 100.0);

        // Update with output
        tracker.update_with_output("test output");
        let progress = tracker.get_progress();
        assert!(progress.elapsed_ms > 0);
    }

    #[tokio::test]
    async fn test_json_extraction() {
        let mut buffer = String::new();
        let data = r#"{"key": "value"} some text {"another": "object"}"#;

        let events = StreamingSkillExecutor::extract_json_objects(&mut buffer, data);
        assert!(events.is_some());

        let events = events.unwrap();
        assert_eq!(events.len(), 2);

        match &events[0] {
            StreamEvent::JsonObject { value, .. } => {
                assert_eq!(value["key"], "value");
            }
            _ => panic!("Expected JsonObject event"),
        }
    }
}
