//! ANSI escape sequence parser and utilities

/// Strip ANSI escape codes from text, keeping only plain text
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
                    b'P' | b'^' | b'_' | b'X' => {
                        // DCS/PM/APC/SOS sequence - ends with ST (ESC \)
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
                        if bytes[i + 1] < 128 {
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                }
            } else {
                i += 1;
            }
        } else if bytes[i] == b'\n' || bytes[i] == b'\r' || bytes[i] == b'\t' {
            output.push(bytes[i]);
            i += 1;
        } else if bytes[i] < 32 {
            i += 1;
        } else {
            output.push(bytes[i]);
            i += 1;
        }
    }

    unsafe { String::from_utf8_unchecked(output) }
}

/// Parse and determine the length of the ANSI escape sequence at the start of text
pub fn parse_ansi_sequence(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    if bytes.len() < 2 || bytes[0] != 0x1b {
        return None;
    }

    let kind = bytes[1];
    match kind {
        b'[' => {
            for (index, byte) in bytes.iter().enumerate().skip(2) {
                if (0x40..=0x7e).contains(byte) {
                    return Some(index + 1);
                }
            }
            None
        }
        b']' => {
            for index in 2..bytes.len() {
                match bytes[index] {
                    0x07 => return Some(index + 1),
                    0x1b if index + 1 < bytes.len() && bytes[index + 1] == b'\\' => {
                        return Some(index + 2);
                    }
                    _ => {}
                }
            }
            None
        }
        b'P' | b'^' | b'_' | b'X' => {
            for index in 2..bytes.len() {
                if bytes[index] == 0x1b && index + 1 < bytes.len() && bytes[index + 1] == b'\\' {
                    return Some(index + 2);
                }
            }
            None
        }
        _ => {
            if bytes[1] < 128 {
                Some(2)
            } else {
                Some(1)
            }
        }
    }
}

/// Fast ASCII-only ANSI stripping for performance-critical paths
pub fn strip_ansi_ascii_only(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut last_valid = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if last_valid < i {
                output.push_str(&text[last_valid..i]);
            }

            if i + 1 < bytes.len() {
                match bytes[i + 1] {
                    b'[' => {
                        i += 2;
                        while i < bytes.len() && !(0x40..=0x7e).contains(&bytes[i]) {
                            i += 1;
                        }
                        if i < bytes.len() {
                            i += 1;
                        }
                    }
                    b']' => {
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
                    b'P' | b'^' | b'_' | b'X' => {
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
                        if bytes[i + 1] < 128 {
                            i += 2;
                        } else {
                            i += 1;
                        }
                    }
                }
            } else {
                i += 1;
            }
            last_valid = i;
        } else {
            i += 1;
        }
    }

    if last_valid < text.len() {
        output.push_str(&text[last_valid..]);
    }

    output
}

/// Detect if text contains unicode characters that need special handling
pub fn contains_unicode(text: &str) -> bool {
    text.bytes().any(|b| b >= 0x80)
}
