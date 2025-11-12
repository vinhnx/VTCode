use anyhow::{Context, Result};
use std::collections::HashMap;

/// Detects potential model loops from repetitive tool calls
#[derive(Clone)]
pub struct LoopDetector {
    /// Track identical tool call signatures (tool_name + args)
    repeated_calls: HashMap<String, usize>,
    /// Threshold for triggering detection
    threshold: usize,
    /// Whether detection is enabled
    enabled: bool,
    /// Whether to show interactive prompt
    interactive: bool,
}

impl LoopDetector {
    pub fn new(threshold: usize, enabled: bool, interactive: bool) -> Self {
        Self {
            repeated_calls: HashMap::new(),
            threshold,
            enabled,
            interactive,
        }
    }

    /// Record a tool call signature and check if loop is detected
    /// Returns (is_detected, count) where is_detected = true if count > threshold
    pub fn record_tool_call(&mut self, signature: &str) -> (bool, usize) {
        if !self.enabled {
            return (false, 0);
        }

        let count = self
            .repeated_calls
            .entry(signature.to_string())
            .or_insert(0);
        *count += 1;

        (*count > self.threshold, *count)
    }

    /// Get the count for a signature without recording a new call
    pub fn peek_count(&self, signature: &str) -> usize {
        self.repeated_calls.get(signature).copied().unwrap_or(0)
    }

    /// Check if next call would trigger detection
    pub fn would_trigger(&self, signature: &str) -> bool {
        if !self.enabled {
            return false;
        }
        let current_count = self.repeated_calls.get(signature).copied().unwrap_or(0);
        current_count + 1 > self.threshold
    }

    /// Clear the tracking state
    pub fn reset(&mut self) {
        self.repeated_calls.clear();
    }

    /// Reset tracking for a specific signature only
    pub fn reset_signature(&mut self, signature: &str) {
        self.repeated_calls.remove(signature);
    }

    /// Get the count of repetitions for a signature
    pub fn get_count(&self, signature: &str) -> usize {
        self.repeated_calls.get(signature).copied().unwrap_or(0)
    }

    /// Check if interactive prompts should be shown
    pub fn is_interactive(&self) -> bool {
        self.interactive
    }

    /// Check if detection is enabled
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Disable detection
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Enable detection
    pub fn enable(&mut self) {
        self.enabled = true;
    }
}

/// Options for user response to loop detection prompt
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopDetectionResponse {
    /// Keep detection enabled for future checks
    KeepEnabled,
    /// Disable detection for remainder of this session
    DisableForSession,
}

/// Handle loop detection prompt and return user's choice
/// Falls back to KeepEnabled if non-interactive
pub fn prompt_for_loop_detection(
    interactive: bool,
    signature: &str,
    repeat_count: usize,
) -> Result<LoopDetectionResponse> {
    if !interactive {
        // Non-interactive mode: default to keeping enabled
        return Ok(LoopDetectionResponse::KeepEnabled);
    }

    show_loop_detection_prompt_tui(signature, repeat_count)
}

/// Display loop detection prompt with interactive TUI selection
/// Only called when interactive mode is enabled
fn show_loop_detection_prompt_tui(
    signature: &str,
    repeat_count: usize,
) -> Result<LoopDetectionResponse> {
    use dialoguer::Select;

    let options = vec![
        "Keep loop detection enabled (esc)",
        "Disable loop detection for this session",
    ];

    // Create a preview of the signature (truncate if too long)
    let sig_preview = if signature.len() > 100 {
        format!("{}...", &signature[..100])
    } else {
        signature.to_string()
    };

    let prompt = format!(
        "A potential loop was detected.\n\nLooping tool call: '{}'\nRepeat count: {}\n\nWhat would you like to do?",
        sig_preview, repeat_count
    );

    let selection = Select::new()
        .with_prompt(prompt)
        .default(0)
        .items(&options)
        .interact()
        .context("Failed to read user input for loop detection prompt")?;

    match selection {
        0 => Ok(LoopDetectionResponse::KeepEnabled),
        1 => Ok(LoopDetectionResponse::DisableForSession),
        _ => Ok(LoopDetectionResponse::KeepEnabled),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_detector_threshold() {
        let mut detector = LoopDetector::new(3, true, true);

        let sig = "read_file::/path/to/file";

        let (detected, count) = detector.record_tool_call(sig);
        assert!(!detected && count == 1);

        let (detected, count) = detector.record_tool_call(sig);
        assert!(!detected && count == 2);

        let (detected, count) = detector.record_tool_call(sig);
        assert!(!detected && count == 3);

        let (detected, count) = detector.record_tool_call(sig);
        assert!(detected && count == 4); // count = 4 > threshold of 3
    }

    #[test]
    fn test_loop_detector_disabled() {
        let mut detector = LoopDetector::new(3, false, true);

        let sig = "read_file::/path/to/file";

        assert!(!detector.record_tool_call(sig).0);
        assert!(!detector.record_tool_call(sig).0);
        assert!(!detector.record_tool_call(sig).0);
        assert!(!detector.record_tool_call(sig).0); // Still false since detection is disabled
    }

    #[test]
    fn test_loop_detector_reset() {
        let mut detector = LoopDetector::new(2, true, true);

        let sig = "read_file::/path/to/file";

        assert!(!detector.record_tool_call(sig).0); // count = 1
        assert!(!detector.record_tool_call(sig).0); // count = 2
        assert!(detector.record_tool_call(sig).0); // count = 3 > threshold of 2

        detector.reset();

        assert_eq!(detector.get_count(sig), 0);
        assert!(!detector.record_tool_call(sig).0); // count = 1
    }

    #[test]
    fn test_loop_detector_different_signatures() {
        let mut detector = LoopDetector::new(2, true, true);

        let sig1 = "read_file::/path/to/file1";
        let sig2 = "read_file::/path/to/file2";

        assert!(!detector.record_tool_call(sig1).0); // sig1 count = 1
        assert!(!detector.record_tool_call(sig2).0); // sig2 count = 1
        assert!(!detector.record_tool_call(sig1).0); // sig1 count = 2
        assert!(detector.record_tool_call(sig1).0); // sig1 count = 3 > threshold

        // sig2 should still be at 1
        assert_eq!(detector.get_count(sig2), 1);
    }

    #[test]
    fn test_loop_detector_interactive_flag() {
        let interactive_detector = LoopDetector::new(3, true, true);
        let non_interactive_detector = LoopDetector::new(3, true, false);

        assert!(interactive_detector.is_interactive());
        assert!(!non_interactive_detector.is_interactive());
    }

    #[test]
    fn test_loop_detector_enable_disable() {
        let mut detector = LoopDetector::new(2, true, true);

        assert!(detector.is_enabled());

        detector.disable();
        assert!(!detector.is_enabled());
        assert!(!detector.record_tool_call("test").0);

        detector.enable();
        assert!(detector.is_enabled());
    }

    #[test]
    fn test_loop_detection_response_enum() {
        assert_eq!(
            LoopDetectionResponse::KeepEnabled,
            LoopDetectionResponse::KeepEnabled
        );
        assert_eq!(
            LoopDetectionResponse::DisableForSession,
            LoopDetectionResponse::DisableForSession
        );
        assert_ne!(
            LoopDetectionResponse::KeepEnabled,
            LoopDetectionResponse::DisableForSession
        );
    }

    #[test]
    fn test_peek_count() {
        let mut detector = LoopDetector::new(2, true, true);

        assert_eq!(detector.peek_count("test"), 0);

        detector.record_tool_call("test");
        assert_eq!(detector.peek_count("test"), 1);

        detector.record_tool_call("test");
        assert_eq!(detector.peek_count("test"), 2);
    }

    #[test]
    fn test_would_trigger() {
        let mut detector = LoopDetector::new(2, true, true);

        assert!(!detector.would_trigger("test")); // would be 1
        detector.record_tool_call("test");

        assert!(!detector.would_trigger("test")); // would be 2
        detector.record_tool_call("test");

        assert!(detector.would_trigger("test")); // would be 3 > threshold of 2
    }

    #[test]
    fn test_non_interactive_mode() {
        let detector = LoopDetector::new(3, true, false);
        assert!(!detector.is_interactive());

        // Non-interactive mode should still detect loops
        let mut detector = detector;
        assert!(!detector.record_tool_call("test").0); // 1
        assert!(!detector.record_tool_call("test").0); // 2
        assert!(!detector.record_tool_call("test").0); // 3
        assert!(detector.record_tool_call("test").0); // 4 > threshold
    }

    #[test]
    fn test_selective_reset() {
        let mut detector = LoopDetector::new(2, true, true);

        // Record two different signatures
        detector.record_tool_call("sig1");
        detector.record_tool_call("sig1");
        detector.record_tool_call("sig2");
        detector.record_tool_call("sig2");

        assert_eq!(detector.get_count("sig1"), 2);
        assert_eq!(detector.get_count("sig2"), 2);

        // Selectively reset only sig1
        detector.reset_signature("sig1");

        assert_eq!(detector.get_count("sig1"), 0); // Reset
        assert_eq!(detector.get_count("sig2"), 2); // Untouched
    }
}
