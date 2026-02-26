//! Streaming processor for handling real-time responses from the Gemini API
//!
//! This module provides functionality to process streaming responses from the Gemini API,
//! parse them in real-time, and provide callbacks for handling each chunk of data.

use crate::gemini::models::{Content, Part};
use crate::gemini::streaming::{
    StreamingCandidate, StreamingError, StreamingMetrics, StreamingResponse,
};
use futures::stream::StreamExt;
use reqwest::Response;
use serde_json::Value;
use std::time::Instant;
use tokio::time::{Duration, timeout};
use tracing;

/// Configuration for the streaming processor
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// Timeout for reading each chunk
    pub chunk_timeout: Duration,
    /// Maximum time to wait for the first chunk
    pub first_chunk_timeout: Duration,
    /// Maximum total time for entire streaming request (0 means no limit)
    pub total_timeout: Duration,
    /// Buffer size for chunk processing
    pub buffer_size: usize,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            chunk_timeout: Duration::from_secs(30),
            first_chunk_timeout: Duration::from_secs(60),
            total_timeout: Duration::from_secs(600),
            buffer_size: 1024,
        }
    }
}

impl StreamingConfig {
    /// Create config with custom total timeout (in seconds)
    pub fn with_total_timeout(total_timeout_secs: u64) -> Self {
        Self {
            total_timeout: Duration::from_secs(total_timeout_secs),
            ..Default::default()
        }
    }
}

/// Callback for streaming timeout progress updates
pub type ProgressCallback = Box<dyn Fn(f32) + Send + Sync>;

/// Streaming processor for handling real-time responses from the Gemini API
pub struct StreamingProcessor {
    config: StreamingConfig,
    metrics: StreamingMetrics,
    current_event_data: String,
    progress_callback: Option<ProgressCallback>,
    warning_threshold: f32,
}

impl StreamingProcessor {
    /// Create a new streaming processor with default configuration
    pub fn new() -> Self {
        Self {
            config: StreamingConfig::default(),
            metrics: StreamingMetrics::default(),
            // Optimize: Pre-allocate for typical JSON event size
            current_event_data: String::with_capacity(512),
            progress_callback: None,
            warning_threshold: 0.8,
        }
    }

    /// Create a new streaming processor with custom configuration
    pub fn with_config(config: StreamingConfig) -> Self {
        Self {
            config,
            metrics: StreamingMetrics::default(),
            // Optimize: Pre-allocate for typical JSON event size
            current_event_data: String::with_capacity(512),
            progress_callback: None,
            warning_threshold: 0.8,
        }
    }

    /// Set a progress callback that reports timeout progress as a ratio (0.0-1.0)
    pub fn with_progress_callback(mut self, callback: ProgressCallback) -> Self {
        self.progress_callback = Some(callback);
        self
    }

    /// Set the warning threshold for timeout progress (0.0-1.0, default 0.8)
    pub fn with_warning_threshold(mut self, threshold: f32) -> Self {
        self.warning_threshold = threshold.clamp(0.0, 1.0);
        self
    }

    /// Process a streaming response from the Gemini API
    ///
    /// This method takes a response and processes it in real-time, calling the provided
    /// callback for each chunk of content received.
    ///
    /// # Arguments
    ///
    /// * `response` - The HTTP response from the Gemini API
    /// * `on_chunk` - A callback function that will be called for each text chunk received
    ///
    /// # Returns
    ///
    /// A result containing the final accumulated response or a streaming error
    pub async fn process_stream<F>(
        &mut self,
        response: Response,
        mut on_chunk: F,
    ) -> Result<StreamingResponse, StreamingError>
    where
        F: FnMut(&str) -> Result<(), StreamingError>,
    {
        self.metrics.request_start_time = Some(Instant::now());
        self.metrics.total_requests += 1;
        self.current_event_data.clear();

        // Get the response stream
        let mut stream = response.bytes_stream();

        let mut accumulated_response = StreamingResponse {
            candidates: Vec::new(),
            usage_metadata: None,
        };

        let mut _has_valid_content = false;
        // Optimize: Pre-allocate buffer with typical SSE line size to reduce reallocations
        let mut buffer = String::with_capacity(self.config.buffer_size);
        let request_start = Instant::now();

        // Wait for the first chunk with a longer timeout
        let first_chunk_start = Instant::now();
        let first_chunk_result = timeout(self.config.first_chunk_timeout, stream.next()).await;

        match first_chunk_result {
            Ok(Some(Ok(bytes))) => {
                self.metrics.first_chunk_time = Some(Instant::now());
                self.metrics.total_bytes += bytes.len();
                self.report_progress(first_chunk_start, request_start);

                // Process the first chunk
                buffer.push_str(&String::from_utf8_lossy(&bytes));
                match self.process_buffer(&mut buffer, &mut accumulated_response, &mut on_chunk) {
                    Ok(valid) => _has_valid_content = valid,
                    Err(e) => return Err(e),
                }
            }
            Ok(Some(Err(e))) => {
                self.metrics.error_count += 1;
                return Err(StreamingError::NetworkError {
                    message: format!("Failed to read first chunk: {}", e),
                    is_retryable: true,
                });
            }
            Ok(None) => {
                return Err(StreamingError::StreamingError {
                    message: "Empty streaming response".to_owned(),
                    partial_content: None,
                });
            }
            Err(_) => {
                self.metrics.error_count += 1;
                self.report_progress(first_chunk_start, request_start);
                return Err(StreamingError::TimeoutError {
                    operation: "first_chunk".to_owned(),
                    duration: self.config.first_chunk_timeout,
                });
            }
        }

        // Process subsequent chunks
        while let Some(result) = stream.next().await {
            let elapsed = request_start.elapsed();

            // Check if total timeout has been exceeded
            if self.config.total_timeout.as_secs() > 0 {
                if elapsed > self.config.total_timeout {
                    self.metrics.error_count += 1;
                    self.report_progress_at_timeout(elapsed);
                    return Err(StreamingError::TimeoutError {
                        operation: "streaming".to_owned(),
                        duration: elapsed,
                    });
                }
                // Report progress towards timeout
                self.report_progress_with_timeout(elapsed);
            }

            match result {
                Ok(bytes) => {
                    self.metrics.total_bytes += bytes.len();

                    // Add to buffer
                    buffer.push_str(&String::from_utf8_lossy(&bytes));

                    // Process buffer
                    match self.process_buffer(&mut buffer, &mut accumulated_response, &mut on_chunk)
                    {
                        Ok(valid) => {
                            if valid {
                                _has_valid_content = true;
                            }
                        }
                        Err(e) => return Err(e),
                    }
                }
                Err(e) => {
                    self.metrics.error_count += 1;
                    self.report_progress_at_timeout(elapsed);
                    return Err(StreamingError::NetworkError {
                        message: format!("Failed to read chunk: {}", e),
                        is_retryable: true,
                    });
                }
            }

            self.metrics.total_chunks += 1;
        }

        // Process any remaining data in the buffer
        if !buffer.is_empty() {
            match self.process_remaining_buffer(
                &mut buffer,
                &mut accumulated_response,
                &mut on_chunk,
            ) {
                Ok(valid) => {
                    if valid {
                        _has_valid_content = true;
                    }
                }
                Err(e) => return Err(e),
            }
        }

        if !_has_valid_content {
            return Err(StreamingError::ContentError {
                message: "No valid content received from streaming API".to_owned(),
            });
        }

        Ok(accumulated_response)
    }

    fn process_buffer<F>(
        &mut self,
        buffer: &mut String,
        accumulated_response: &mut StreamingResponse,
        on_chunk: &mut F,
    ) -> Result<bool, StreamingError>
    where
        F: FnMut(&str) -> Result<(), StreamingError>,
    {
        let mut _has_valid_content = false;
        let mut processed_chars = 0;

        while let Some(newline_pos) = buffer[processed_chars..].find('\n') {
            let line_end = processed_chars + newline_pos;
            let line = &buffer[processed_chars..line_end];
            processed_chars = line_end + 1;

            match self.handle_line(line, accumulated_response, on_chunk) {
                Ok(valid) => {
                    if valid {
                        _has_valid_content = true;
                    }
                }
                Err(e) => return Err(e),
            }
        }

        // Optimize: Use drain to avoid allocation when removing processed content
        if processed_chars > 0 {
            buffer.drain(..processed_chars);
        }

        Ok(_has_valid_content)
    }

    /// Process any remaining data in the buffer after streaming is complete
    fn process_remaining_buffer<F>(
        &mut self,
        buffer: &mut String,
        accumulated_response: &mut StreamingResponse,
        on_chunk: &mut F,
    ) -> Result<bool, StreamingError>
    where
        F: FnMut(&str) -> Result<(), StreamingError>,
    {
        let mut _has_valid_content = false;

        if !buffer.is_empty() {
            let remaining_line = buffer.trim_end_matches('\r');
            if !remaining_line.trim().is_empty() {
                match self.handle_line(remaining_line, accumulated_response, on_chunk) {
                    Ok(valid) => {
                        if valid {
                            _has_valid_content = true;
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
        }

        buffer.clear();

        match self.finalize_current_event(accumulated_response, on_chunk) {
            Ok(valid) => {
                if valid {
                    _has_valid_content = true;
                }
            }
            Err(e) => return Err(e),
        }

        Ok(_has_valid_content)
    }

    /// Handle a single SSE line
    fn handle_line<F>(
        &mut self,
        raw_line: &str,
        accumulated_response: &mut StreamingResponse,
        on_chunk: &mut F,
    ) -> Result<bool, StreamingError>
    where
        F: FnMut(&str) -> Result<(), StreamingError>,
    {
        let mut _has_valid_content = false;
        let line = raw_line.trim_end_matches('\r');

        if line.is_empty() {
            match self.finalize_current_event(accumulated_response, on_chunk) {
                Ok(valid) => {
                    if valid {
                        _has_valid_content = true;
                    }
                }
                Err(e) => return Err(e),
            }
            return Ok(_has_valid_content);
        }

        let trimmed = line.trim();

        if trimmed.is_empty() {
            return Ok(false);
        }

        if trimmed.starts_with(':') {
            return Ok(false);
        }

        if trimmed.starts_with("event:") || trimmed.starts_with("id:") {
            return Ok(false);
        }

        if let Some(data_segment) = trimmed.strip_prefix("data:") {
            let data_segment = data_segment.trim_start();
            if data_segment == "[DONE]" {
                match self.finalize_current_event(accumulated_response, on_chunk) {
                    Ok(valid) => {
                        if valid {
                            _has_valid_content = true;
                        }
                    }
                    Err(e) => return Err(e),
                }
                return Ok(_has_valid_content);
            }

            if !data_segment.is_empty() {
                if !self.current_event_data.is_empty() {
                    self.current_event_data.push('\n');
                }
                self.current_event_data.push_str(data_segment);

                match self.try_flush_current_event(accumulated_response, on_chunk) {
                    Ok(valid) => {
                        if valid {
                            _has_valid_content = true;
                        }
                    }
                    Err(e) => return Err(e),
                }
            }
            return Ok(_has_valid_content);
        }

        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            if !self.current_event_data.is_empty() {
                self.current_event_data.push('\n');
            }
            self.current_event_data.push_str(trimmed);
            return Ok(false);
        }

        if !self.current_event_data.is_empty() {
            self.current_event_data.push('\n');
        }
        self.current_event_data.push_str(trimmed);

        Ok(false)
    }

    fn finalize_current_event<F>(
        &mut self,
        accumulated_response: &mut StreamingResponse,
        on_chunk: &mut F,
    ) -> Result<bool, StreamingError>
    where
        F: FnMut(&str) -> Result<(), StreamingError>,
    {
        if self.current_event_data.trim().is_empty() {
            self.current_event_data.clear();
            return Ok(false);
        }

        let event_data = std::mem::take(&mut self.current_event_data);
        self.process_event(event_data, accumulated_response, on_chunk)
    }

    fn try_flush_current_event<F>(
        &mut self,
        accumulated_response: &mut StreamingResponse,
        on_chunk: &mut F,
    ) -> Result<bool, StreamingError>
    where
        F: FnMut(&str) -> Result<(), StreamingError>,
    {
        let trimmed = self.current_event_data.trim();
        if trimmed.is_empty() {
            return Ok(false);
        }

        match serde_json::from_str::<Value>(trimmed) {
            Ok(parsed) => {
                self.current_event_data.clear();
                self.process_event_value(parsed, accumulated_response, on_chunk)
            }
            Err(parse_err) => {
                if parse_err.is_eof() {
                    return Ok(false);
                }

                Err(StreamingError::ParseError {
                    message: format!("Failed to parse streaming JSON: {}", parse_err),
                    raw_response: trimmed.to_owned(),
                })
            }
        }
    }

    fn process_event<F>(
        &mut self,
        event_data: String,
        accumulated_response: &mut StreamingResponse,
        on_chunk: &mut F,
    ) -> Result<bool, StreamingError>
    where
        F: FnMut(&str) -> Result<(), StreamingError>,
    {
        let trimmed = event_data.trim();

        if trimmed.is_empty() {
            return Ok(false);
        }

        match serde_json::from_str::<Value>(trimmed) {
            Ok(parsed) => self.process_event_value(parsed, accumulated_response, on_chunk),
            Err(parse_err) => {
                if parse_err.is_eof() {
                    self.current_event_data = trimmed.to_owned();
                    return Ok(false);
                }

                Err(StreamingError::ParseError {
                    message: format!("Failed to parse streaming JSON: {}", parse_err),
                    raw_response: trimmed.to_owned(),
                })
            }
        }
    }

    fn append_text_candidate(&mut self, accumulated_response: &mut StreamingResponse, text: &str) {
        if text.is_empty() {
            return;
        }

        if let Some(last_candidate) = accumulated_response.candidates.last_mut() {
            Self::merge_parts(
                &mut last_candidate.content.parts,
                vec![Part::Text {
                    text: text.to_owned(),
                    thought_signature: None,
                }],
            );
            return;
        }

        let index = accumulated_response.candidates.len();

        accumulated_response.candidates.push(StreamingCandidate {
            content: Content {
                role: "model".to_owned(),
                parts: vec![Part::Text {
                    text: text.to_owned(),
                    thought_signature: None,
                }],
            },
            finish_reason: None,
            index: Some(index),
        });
    }

    /// Process a streaming candidate and extract content
    fn process_candidate<F>(
        &self,
        candidate: &StreamingCandidate,
        on_chunk: &mut F,
    ) -> Result<bool, StreamingError>
    where
        F: FnMut(&str) -> Result<(), StreamingError>,
    {
        let mut _has_valid_content = false;

        if candidate.finish_reason.is_some() {
            _has_valid_content = true;
        }

        // Process each part of the content
        for part in &candidate.content.parts {
            match part {
                Part::Text { text, .. } => {
                    if !text.trim().is_empty() {
                        on_chunk(text)?;
                        _has_valid_content = true;
                    }
                }
                Part::InlineData { .. } => {
                    _has_valid_content = true;
                }
                Part::FunctionCall { .. } => {
                    // Function calls are handled separately in the tool execution flow
                    _has_valid_content = true;
                }
                Part::FunctionResponse { .. } => {
                    _has_valid_content = true;
                }
                Part::CacheControl { .. } => {}
            }
        }

        Ok(_has_valid_content)
    }

    fn process_event_value<F>(
        &mut self,
        value: Value,
        accumulated_response: &mut StreamingResponse,
        on_chunk: &mut F,
    ) -> Result<bool, StreamingError>
    where
        F: FnMut(&str) -> Result<(), StreamingError>,
    {
        match value {
            Value::Array(items) => {
                let mut has_valid = false;
                for item in items {
                    if self.process_event_value(item, accumulated_response, on_chunk)? {
                        has_valid = true;
                    }
                }
                Ok(has_valid)
            }
            Value::Object(mut map) => {
                if let Some(error_value) = map.get("error") {
                    let message = error_value
                        .get("message")
                        .and_then(Value::as_str)
                        .unwrap_or("Gemini streaming error")
                        .to_owned();
                    let code = error_value
                        .get("code")
                        .and_then(Value::as_i64)
                        .unwrap_or(500) as u16;
                    return Err(StreamingError::ApiError {
                        status_code: code,
                        message,
                        is_retryable: code == 429,
                    });
                }

                if let Some(usage) = map.remove("usageMetadata") {
                    accumulated_response.usage_metadata = Some(usage);
                }

                let mut has_valid = false;

                if let Some(candidates_value) = map.remove("candidates") {
                    let candidate_values: Vec<Value> = match candidates_value {
                        Value::Array(items) => items,
                        Value::Object(_) => vec![candidates_value],
                        _ => Vec::new(),
                    };

                    for candidate_value in candidate_values {
                        match serde_json::from_value::<StreamingCandidate>(candidate_value.clone())
                        {
                            Ok(candidate) => {
                                if self.process_candidate(&candidate, on_chunk)? {
                                    has_valid = true;
                                }
                                self.merge_candidate(accumulated_response, candidate);
                            }
                            Err(err) => {
                                if let Some(text) = Self::extract_text_from_value(&candidate_value)
                                {
                                    if !text.trim().is_empty() {
                                        on_chunk(&text)?;
                                        self.append_text_candidate(accumulated_response, &text);
                                        has_valid = true;
                                    }
                                } else {
                                    return Err(StreamingError::ParseError {
                                        message: format!("Failed to parse candidate: {}", err),
                                        raw_response: candidate_value.to_string(),
                                    });
                                }
                            }
                        }
                    }
                }

                if let Some(text_value) = map
                    .remove("text")
                    .and_then(|v| v.as_str().map(|s| s.to_owned()))
                    && !text_value.trim().is_empty()
                {
                    on_chunk(&text_value)?;
                    self.append_text_candidate(accumulated_response, &text_value);
                    has_valid = true;
                }

                Ok(has_valid)
            }
            Value::String(text) => {
                if text.trim().is_empty() {
                    Ok(false)
                } else {
                    on_chunk(&text)?;
                    self.append_text_candidate(accumulated_response, &text);
                    Ok(true)
                }
            }
            _ => Ok(false),
        }
    }

    fn merge_candidate(
        &mut self,
        accumulated_response: &mut StreamingResponse,
        mut candidate: StreamingCandidate,
    ) {
        let index = candidate
            .index
            .unwrap_or(accumulated_response.candidates.len());

        if let Some(existing) = accumulated_response
            .candidates
            .iter_mut()
            .find(|existing| existing.index.unwrap_or(index) == index)
        {
            if existing.content.role.is_empty() {
                existing.content.role = candidate.content.role.clone();
            }

            Self::merge_parts(&mut existing.content.parts, candidate.content.parts);

            if candidate.finish_reason.is_some() {
                existing.finish_reason = candidate.finish_reason;
            }
        } else {
            candidate.index = Some(index);
            accumulated_response.candidates.push(candidate);
        }
    }

    fn merge_parts(target: &mut Vec<Part>, source_parts: Vec<Part>) {
        if target.is_empty() {
            *target = source_parts;
            return;
        }

        for part in source_parts {
            match (target.last_mut(), &part) {
                (
                    Some(Part::Text {
                        text: existing,
                        thought_signature: existing_sig,
                    }),
                    Part::Text {
                        text: new_text,
                        thought_signature: new_sig,
                    },
                ) => {
                    existing.push_str(new_text);
                    // Preserve thought signature if it arrives in a later chunk
                    if existing_sig.is_none() && new_sig.is_some() {
                        *existing_sig = new_sig.clone();
                    }
                }
                _ => target.push(part),
            }
        }
    }

    fn extract_text_from_value(value: &Value) -> Option<String> {
        match value {
            Value::String(text) => {
                if text.trim().is_empty() {
                    None
                } else {
                    Some(text.clone())
                }
            }
            Value::Array(items) => {
                let mut collected = String::new();
                for item in items {
                    if let Some(fragment) = Self::extract_text_from_value(item) {
                        collected.push_str(&fragment);
                    }
                }
                if collected.is_empty() {
                    None
                } else {
                    Some(collected)
                }
            }
            Value::Object(map) => {
                if let Some(text) = map.get("text").and_then(Value::as_str)
                    && !text.trim().is_empty()
                {
                    return Some(text.to_owned());
                }

                if let Some(parts) = map.get("parts").and_then(Value::as_array)
                    && let Some(parts_text) =
                        Self::extract_text_from_value(&Value::Array(parts.clone()))
                {
                    return Some(parts_text);
                }

                for nested in map.values() {
                    if let Some(nested_text) = Self::extract_text_from_value(nested)
                        && !nested_text.trim().is_empty()
                    {
                        return Some(nested_text);
                    }
                }

                None
            }
            _ => None,
        }
    }

    /// Report progress towards the timeout threshold
    fn report_progress_with_timeout(&self, elapsed: Duration) {
        if self.config.total_timeout.as_secs() == 0 {
            return;
        }

        let progress = elapsed.as_secs_f32() / self.config.total_timeout.as_secs_f32();
        let progress_clamped = progress.min(0.99); // Cap at 99% to avoid premature completion

        if let Some(ref callback) = self.progress_callback {
            callback(progress_clamped);
        }

        // Warn if approaching warning threshold
        if progress >= self.warning_threshold {
            tracing::warn!(
                "Streaming operation at {:.0}% of timeout limit ({}/{:?} elapsed). Approaching timeout.",
                progress_clamped * 100.0,
                elapsed.as_secs(),
                self.config.total_timeout
            );
        }
    }

    /// Report progress when timeout/error occurs
    fn report_progress_at_timeout(&self, _elapsed: Duration) {
        if let Some(ref callback) = self.progress_callback {
            callback(1.0); // Report as complete/failed
        }
    }

    /// Report immediate progress (e.g., after first chunk)
    fn report_progress(&self, _event_time: Instant, _start_time: Instant) {
        if let Some(ref callback) = self.progress_callback {
            callback(0.1); // Report early progress
        }
    }

    /// Get current streaming metrics
    pub fn metrics(&self) -> &StreamingMetrics {
        &self.metrics
    }

    /// Reset streaming metrics
    pub fn reset_metrics(&mut self) {
        self.metrics = StreamingMetrics::default();
    }
}

impl Default for StreamingProcessor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_streaming_processor_creation() {
        let processor = StreamingProcessor::new();
        assert_eq!(processor.metrics().total_requests, 0);
    }

    #[test]
    fn test_streaming_processor_with_config() {
        use std::time::Duration;

        let config = StreamingConfig {
            chunk_timeout: Duration::from_secs(10),
            first_chunk_timeout: Duration::from_secs(30),
            total_timeout: Duration::from_secs(120),
            buffer_size: 512,
        };

        let processor = StreamingProcessor::with_config(config);
        assert_eq!(processor.metrics().total_requests, 0);
    }

    #[test]
    fn test_streaming_config_default() {
        let config = StreamingConfig::default();
        assert_eq!(config.buffer_size, 1024);
    }

    #[test]
    fn test_handles_back_to_back_data_lines_without_blank_lines() {
        let mut processor = StreamingProcessor::new();
        let mut accumulated = StreamingResponse {
            candidates: Vec::new(),
            usage_metadata: None,
        };
        let mut received_chunks: Vec<String> = Vec::new();
        let mut buffer = String::from(
            "data: {\"candidates\":[{\"index\":0,\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\"Hello\"}]}}]}\n",
        );
        buffer.push_str(
            "data: {\"candidates\":[{\"index\":0,\"content\":{\"role\":\"model\",\"parts\":[{\"text\":\" world\"}]}}]}\n",
        );

        {
            let mut on_chunk = |chunk: &str| {
                received_chunks.push(chunk.to_owned());
                Ok(())
            };
            let has_valid = processor
                .process_buffer(&mut buffer, &mut accumulated, &mut on_chunk)
                .expect("processing should succeed");
            assert!(has_valid);
        }

        assert_eq!(received_chunks, vec!["Hello", " world"]);
        assert_eq!(accumulated.candidates.len(), 1);
        let combined = match &accumulated.candidates[0].content.parts[0] {
            Part::Text { text, .. } => text.clone(),
            _ => String::new(),
        };
        assert_eq!(combined, "Hello world");
    }
}
