//! ANSI escape sequence parser and utilities
//!
//! Provides utilities for parsing and processing ANSI escape codes from terminal output.

/// Strip ANSI escape codes from text, keeping only plain text
///
/// Removes all ANSI escape sequences (colors, styles, cursor movements, etc.)
/// while preserving printable characters and control characters like newlines.
///
/// # Example
///
/// ```
/// # use vtcode_core::utils::ansi_parser::strip_ansi;
/// assert_eq!(strip_ansi("\x1b[31mRed text\x1b[0m"), "Red text");
/// assert_eq!(strip_ansi("\x1b[1;32mbold green\x1b[0m"), "bold green");
/// ```
pub fn strip_ansi(text: &str) -> String {
    let mut output = Vec::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // Start of escape sequence
            if i + 1 < bytes.len() {
                match bytes[i + 1] {
                    b'[' => {
                        // CSI sequence - ends with a letter in range 0x40-0x7E
                        i += 2;
                        while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                            i += 1;
                        }
                        if i < bytes.len() {
                            i += 1; // Include the final letter
                        }
                    }
                    b']' => {
                        // OSC sequence - ends with BEL (0x07) or ST (ESC \)
                        i += 2;
                        while i < bytes.len() {
                            if bytes[i] == 0x07 {
                                // BEL terminator
                                i += 1;
                                break;
                            }
                            if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                                // ST terminator (ESC \)
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    b'P' | b'^' | b'_' => {
                        // DCS/PM/APC sequence - ends with ST (ESC \)
                        i += 2;
                        while i < bytes.len() {
                            if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ => {
                        // Other 2-character escape sequences
                        // Only skip if the second byte is ASCII to avoid breaking UTF-8
                        if bytes[i + 1] < 128 {
                            i += 2;
                        } else {
                            // ESC followed by non-ASCII (likely start of UTF-8 sequence).
                            // Just skip the ESC and let the next byte be processed.
                            i += 1;
                        }
                    }
                }
            } else {
                i += 1;
            }
        } else if bytes[i] == b'\n' || bytes[i] == b'\r' || bytes[i] == b'\t' {
            // Preserve line breaks and tabs
            output.push(bytes[i]);
            i += 1;
        } else if bytes[i] < 32 {
            // Skip other control characters (except tab)
            i += 1;
        } else {
            // Regular character
            output.push(bytes[i]);
            i += 1;
        }
    }

    // SAFETY: We started with a valid UTF-8 string and only removed ASCII bytes
    // (ANSI codes and control characters < 32). We never removed bytes from the
    // middle of a multi-byte UTF-8 sequence (which are always >= 128).
    unsafe { String::from_utf8_unchecked(output) }
}

/// Parse and determine the length of the ANSI escape sequence at the start of text
///
/// This function is used to identify how many bytes an escape sequence occupies
/// at the beginning of a text string.
///
/// # Example
///
/// ```
/// # use vtcode_core::utils::ansi_parser::parse_ansi_sequence;
/// assert_eq!(parse_ansi_sequence("\x1b[31m"), Some(4)); // \x1b[31m is 4 bytes
/// assert_eq!(parse_ansi_sequence("\x1b[0m"), Some(4));  // \x1b[0m is 4 bytes
/// assert_eq!(parse_ansi_sequence("plain"), None);       // No escape sequence
/// ```
pub fn parse_ansi_sequence(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 || bytes[0] != 0x1b {
        return None;
    }

    let kind = bytes[1];
    match kind {
        b'[' => {
            // CSI (Control Sequence Introducer) sequence - starts with ESC[
            // Ends with a final byte in the range 0x40-0x7E
            for (index, byte) in bytes.iter().enumerate().skip(2) {
                if (0x40..=0x7e).contains(byte) {
                    return Some(index + 1);
                }
            }
            None
        }
        b']' => {
            // OSC (Operating System Command) sequence - starts with ESC]
            // Ends with either BEL (0x07) or ST (ESC \)
            for index in 2..bytes.len() {
                match bytes[index] {
                    0x07 => return Some(index + 1), // BEL terminator
                    0x1b if index + 1 < bytes.len() && bytes[index + 1] == b'\\' => {
                        // ST terminator (ESC \)
                        return Some(index + 2);
                    }
                    _ => {}
                }
            }
            None
        }
        b'P' | b'^' | b'_' => {
            // DCS/PM/APC sequences - starts with ESC P, ESC ^, or ESC _
            // Ends with ST (ESC \)
            for index in 2..bytes.len() {
                if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\' {
                    return Some(index + 2);
                }
            }
            None
        }
        _ => {
            // Other escape sequences - might be 2-3 bytes
            // Only treat as 2-byte sequence if the second byte is ASCII
            if bytes[1] < 128 {
                Some(2)
            } else {
                // ESC followed by non-ASCII (likely start of UTF-8 sequence).
                // Treat as just ESC (1 byte) or not a sequence?
                // If we return None, the caller might process ESC as char.
                // If we return Some(1), the caller skips ESC.
                // Given this is used for skipping ANSI, skipping just ESC seems safer
                // than skipping ESC + UTF-8 byte.
                Some(1)
            }
        }
    }
}

/// Fast ASCII-only ANSI stripping for performance-critical paths
///
/// This function assumes the input contains only ASCII characters (no unicode)
/// and provides a faster path for ANSI removal in such cases.
pub fn strip_ansi_ascii_only(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut last_valid = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // Copy any valid characters before this escape sequence
            if last_valid < i {
                output.push_str(&text[last_valid..i]);
            }

            // Start of escape sequence
            if i + 1 < bytes.len() {
                match bytes[i + 1] {
                    b'[' => {
                        // CSI sequence - ends with a letter in range 0x40-0x7E
                        i += 2;
                        while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                            i += 1;
                        }
                        if i < bytes.len() {
                            i += 1; // Include the final letter
                        }
                    }
                    b']' => {
                        // OSC sequence - ends with BEL (0x07) or ST (ESC \)
                        i += 2;
                        while i < bytes.len() {
                            if bytes[i] == 0x07 {
                                i += 1;
                                break;
                            }
                            if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    b'P' | b'^' | b'_' => {
                        // DCS/PM/APC sequence - ends with ST (ESC \)
                        i += 2;
                        while i < bytes.len() {
                            if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'\\' {
                                i += 2;
                                break;
                            }
                            i += 1;
                        }
                    }
                    _ => {
                        // Other 2-character escape sequences
                        // Only skip if the second byte is ASCII to avoid breaking UTF-8
                        if bytes[i + 1] < 128 {
                            i += 2;
                        } else {
                            // ESC followed by non-ASCII (likely start of UTF-8 sequence).
                            // Just skip the ESC and let the next byte be processed.
                            i += 1;
                        }
                    }
                }
            } else {
                i += 1;
            }
            last_valid = i;
        } else {
            // Regular ASCII character - continue scanning
            i += 1;
        }
    }

    // Copy any remaining valid characters
    if last_valid < text.len() {
        output.push_str(&text[last_valid..]);
    }

    output
}

/// Detect if text contains unicode characters that need special handling
///
/// Returns true if the text contains non-ASCII characters (bytes >= 0x80)
pub fn contains_unicode(text: &str) -> bool {
    text.bytes().any(|b| b >= 0x80)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text() {
        assert_eq!(strip_ansi("hello world"), "hello world");
    }

    #[test]
    fn test_strip_ansi_basic() {
        assert_eq!(strip_ansi("\x1b[31mred\x1b[0m"), "red");
    }

    #[test]
    fn test_strip_ansi_bold() {
        assert_eq!(strip_ansi("\x1b[1;32mbold green\x1b[0m"), "bold green");
    }

    #[test]
    fn test_strip_ansi_multiple() {
        let input = "Checking \x1b[0m\x1b[1m\x1b[32mvtcode\x1b[0m";
        assert_eq!(strip_ansi(input), "Checking vtcode");
    }

    #[test]
    fn test_preserve_newlines() {
        let input = "line1\nline2";
        assert_eq!(strip_ansi(input), "line1\nline2");
    }

    #[test]
    fn test_preserve_tabs() {
        let input = "col1\tcol2";
        assert_eq!(strip_ansi(input), "col1\tcol2");
    }

    #[test]
    fn test_ansi_with_newlines() {
        let input = "\x1b[31mRed\x1b[0m\nNormal";
        assert_eq!(strip_ansi(input), "Red\nNormal");
    }

    #[test]
    fn test_empty_string() {
        assert_eq!(strip_ansi(""), "");
    }

    #[test]
    fn test_only_ansi_codes() {
        let input = "\x1b[31m\x1b[0m";
        assert_eq!(strip_ansi(input), "");
    }

    #[test]
    fn test_osc_sequence_with_bel() {
        // OSC sequence terminated with BEL (0x07)
        let input = "\x1b]0;Title\x07Normal";
        assert_eq!(strip_ansi(input), "Normal");
    }

    #[test]
    fn test_osc_sequence_with_st() {
        // OSC sequence terminated with ST (ESC \)
        let input = "\x1b]0;Title\x1b\\Normal";
        assert_eq!(strip_ansi(input), "Normal");
    }

    #[test]
    fn test_mixed_sequences() {
        let input = "Start\x1b[31m\x1b[1mBold Red\x1b[0m\x1b[32mGreen\x1b[0mEnd";
        assert_eq!(strip_ansi(input), "StartBold RedGreenEnd");
    }

    #[test]
    fn test_incomplete_escape() {
        // Incomplete escape sequence at end
        let input = "Text\x1b";
        assert_eq!(strip_ansi(input), "Text");
    }

    #[test]
    fn test_escape_at_end() {
        // Escape followed by nothing
        let input = "Text\x1b[";
        assert_eq!(strip_ansi(input), "Text");
    }

    #[test]
    fn test_cargo_check_example() {
        let input = "Checking \x1b[0m\x1b[1m\x1b[32mvtcode\x1b[0m";
        assert_eq!(strip_ansi(input), "Checking vtcode");
    }

    #[test]
    fn test_parse_ansi_sequence_basic() {
        assert_eq!(parse_ansi_sequence("\x1b[0m"), Some(4)); // ESC[0m = 4 bytes
        assert_eq!(parse_ansi_sequence("\x1b[31m"), Some(5)); // ESC[31m = 5 bytes (\x1b[3 + '1' + 'm')
        assert_eq!(parse_ansi_sequence("\x1b[1;32m"), Some(7)); // ESC[1;32m = 7 bytes (\x1b[ + '1' + ';' + '3' + '2' + 'm')
    }

    #[test]
    fn test_parse_ansi_sequence_no_sequence() {
        assert_eq!(parse_ansi_sequence("plain"), None);
        assert_eq!(parse_ansi_sequence(""), None);
        assert_eq!(parse_ansi_sequence("m"), None);
    }

    #[test]
    fn test_parse_ansi_sequence_osc() {
        // OSC sequences terminated with BEL
        assert_eq!(parse_ansi_sequence("\x1b]0;Title\x07"), Some(10));
        // OSC sequences terminated with ST (ESC \)
        assert_eq!(parse_ansi_sequence("\x1b]0;Title\x1b\\"), Some(11));
    }

    #[test]
    fn test_parse_ansi_sequence_dcs() {
        // DCS sequence terminated with ST (ESC \)
        assert_eq!(parse_ansi_sequence("\x1bP0;1;2p\x1b\\"), Some(10));
    }

    #[test]
    fn test_unicode_preservation() {
        let input = "VT\u{2014}Code"; // VT—Code
        assert_eq!(strip_ansi(input), "VT\u{2014}Code");

        let input_ansi = "\x1b[31mVT\u{2014}Code\x1b[0m";
        assert_eq!(strip_ansi(input_ansi), "VT\u{2014}Code");
    }
}
