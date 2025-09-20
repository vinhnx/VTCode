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
use std::time::Instant;
use tokio::time::{Duration, timeout};

/// Configuration for the streaming processor
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// Timeout for reading each chunk
    pub chunk_timeout: Duration,
    /// Maximum time to wait for the first chunk
    pub first_chunk_timeout: Duration,
    /// Buffer size for chunk processing
    pub buffer_size: usize,
}

impl Default for StreamingConfig {
    fn default() -> Self {
        Self {
            chunk_timeout: Duration::from_secs(30),
            first_chunk_timeout: Duration::from_secs(60),
            buffer_size: 1024,
        }
    }
}

/// Streaming processor for handling real-time responses from the Gemini API
pub struct StreamingProcessor {
    config: StreamingConfig,
    metrics: StreamingMetrics,
    current_event_data: String,
}

impl StreamingProcessor {
    /// Create a new streaming processor with default configuration
    pub fn new() -> Self {
        Self {
            config: StreamingConfig::default(),
            metrics: StreamingMetrics::default(),
            current_event_data: String::new(),
        }
    }

    /// Create a new streaming processor with custom configuration
    pub fn with_config(config: StreamingConfig) -> Self {
        Self {
            config,
            metrics: StreamingMetrics::default(),
            current_event_data: String::new(),
        }
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
        let mut buffer = String::new();

        // Wait for the first chunk with a longer timeout
        let first_chunk_result = timeout(self.config.first_chunk_timeout, stream.next()).await;

        match first_chunk_result {
            Ok(Some(Ok(bytes))) => {
                self.metrics.first_chunk_time = Some(Instant::now());
                self.metrics.total_bytes += bytes.len();

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
                    message: "Empty streaming response".to_string(),
                    partial_content: None,
                });
            }
            Err(_) => {
                self.metrics.error_count += 1;
                return Err(StreamingError::TimeoutError {
                    operation: "first_chunk".to_string(),
                    duration: self.config.first_chunk_timeout,
                });
            }
        }

        // Process subsequent chunks
        while let Some(result) = stream.next().await {
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
                message: "No valid content received from streaming API".to_string(),
            });
        }

        Ok(accumulated_response)
    }

    /// Process the buffer and extract complete SSE events
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

        if processed_chars > 0 {
            *buffer = buffer[processed_chars..].to_string();
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

        if trimmed.starts_with("data:") {
            let data_segment = trimmed[5..].trim_start();
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
            }
            return Ok(false);
        }

        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            return self.process_event(trimmed, accumulated_response, on_chunk);
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
        self.process_event(&event_data, accumulated_response, on_chunk)
    }

    fn process_event<F>(
        &mut self,
        event_data: &str,
        accumulated_response: &mut StreamingResponse,
        on_chunk: &mut F,
    ) -> Result<bool, StreamingError>
    where
        F: FnMut(&str) -> Result<(), StreamingError>,
    {
        let mut _has_valid_content = false;
        let trimmed = event_data.trim();

        if trimmed.is_empty() {
            return Ok(false);
        }

        match serde_json::from_str::<StreamingResponse>(trimmed) {
            Ok(response) => {
                if let Some(candidate) = response.candidates.first() {
                    match self.process_candidate(candidate, on_chunk) {
                        Ok(valid) => {
                            if valid {
                                _has_valid_content = true;
                            }

                            accumulated_response.candidates.extend(response.candidates);
                            if response.usage_metadata.is_some() {
                                accumulated_response.usage_metadata = response.usage_metadata;
                            }
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            Err(parse_err) => {
                if let Some(text) = self.extract_text_from_line(trimmed) {
                    if !text.trim().is_empty() {
                        on_chunk(&text)?;
                        self.append_text_candidate(accumulated_response, &text);
                        _has_valid_content = true;
                    }
                } else {
                    eprintln!(
                        "Warning: Failed to parse streaming line as JSON: {}",
                        parse_err
                    );
                }
            }
        }

        Ok(_has_valid_content)
    }

    fn append_text_candidate(&mut self, accumulated_response: &mut StreamingResponse, text: &str) {
        if text.is_empty() {
            return;
        }

        if let Some(last_candidate) = accumulated_response.candidates.last_mut() {
            if let Some(Part::Text { text: existing }) = last_candidate.content.parts.last_mut() {
                existing.push_str(text);
                return;
            }

            last_candidate.content.parts.push(Part::Text {
                text: text.to_string(),
            });
            return;
        }

        accumulated_response.candidates.push(StreamingCandidate {
            content: Content {
                role: "model".to_string(),
                parts: vec![Part::Text {
                    text: text.to_string(),
                }],
            },
            finish_reason: None,
            index: Some(accumulated_response.candidates.len()),
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

        // Process each part of the content
        for part in &candidate.content.parts {
            match part {
                Part::Text { text } => {
                    if !text.trim().is_empty() {
                        on_chunk(text)?;
                        _has_valid_content = true;
                    }
                }
                Part::FunctionCall { .. } => {
                    // Function calls are handled separately in the tool execution flow
                    _has_valid_content = true;
                }
                Part::FunctionResponse { .. } => {
                    _has_valid_content = true;
                }
            }
        }

        Ok(_has_valid_content)
    }

    /// Extract text content from a line that might not be valid JSON
    fn extract_text_from_line(&self, line: &str) -> Option<String> {
        // Simple extraction of text content between quotes
        // This is a fallback for cases where the line isn't valid JSON
        let mut extracted = String::new();
        let mut in_quotes = false;
        let mut escape_next = false;
        let mut current_field = String::new();

        for ch in line.chars() {
            if escape_next {
                current_field.push(ch);
                escape_next = false;
                continue;
            }

            match ch {
                '\\' => {
                    escape_next = true;
                    current_field.push(ch);
                }
                '"' => {
                    if in_quotes {
                        // End of quoted string
                        extracted.push_str(&current_field);
                        current_field.clear();
                        in_quotes = false;
                    } else {
                        // Start of quoted string
                        current_field.clear();
                        in_quotes = true;
                    }
                }
                _ => {
                    if in_quotes {
                        current_field.push(ch);
                    }
                }
            }
        }

        if extracted.is_empty() {
            None
        } else {
            Some(extracted)
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
}
