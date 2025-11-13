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
}

impl LoopDetector {
    pub fn new(threshold: usize, enabled: bool, _interactive: bool) -> Self {
        Self {
            repeated_calls: HashMap::new(),
            threshold,
            enabled,
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

    /// Clear the tracking state
    pub fn reset(&mut self) {
        self.repeated_calls.clear();
    }

    /// Reset tracking for a specific signature only
    pub fn reset_signature(&mut self, signature: &str) {
        self.repeated_calls.remove(signature);
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

        let (detected, count) = detector.record_tool_call(sig);
        assert!(!detected && count == 1);
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

        // sig2 should still be at 1 - verify by recording again
        let (_, sig2_count) = detector.record_tool_call(sig2);
        assert_eq!(sig2_count, 2);
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
    fn test_non_interactive_mode() {
        // Non-interactive mode should still detect loops properly
        let mut detector = LoopDetector::new(3, true, false);
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

        // Selectively reset only sig1
        detector.reset_signature("sig1");

        // After reset, sig1 should be back at 1 (not detected)
        assert!(!detector.record_tool_call("sig1").0);

        // sig2 should continue from where it was - at 3 > threshold of 2
        assert!(detector.record_tool_call("sig2").0);
    }
}
