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
    let mut output = String::with_capacity(text.len());
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
                        i += 1; // Include the final letter
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
                        i += 2;
                    }
                }
            } else {
                i += 1;
            }
        } else if bytes[i] == b'\n' || bytes[i] == b'\r' || bytes[i] == b'\t' {
            // Preserve line breaks and tabs
            output.push(bytes[i] as char);
            i += 1;
        } else if bytes[i] < 32 && bytes[i] != b'\t' {
            // Skip other control characters (except tab)
            i += 1;
        } else {
            // Regular character
            output.push(bytes[i] as char);
            i += 1;
        }
    }

    output
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
            Some(2) // For simple 2-byte escape sequences
        }
    }
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
        assert_eq!(parse_ansi_sequence("\x1b[0m"), Some(4));   // ESC[0m = 4 bytes
        assert_eq!(parse_ansi_sequence("\x1b[31m"), Some(5));  // ESC[31m = 5 bytes (\x1b[3 + '1' + 'm')
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
}
