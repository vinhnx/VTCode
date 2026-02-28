//! ANSI escape sequence parser and utilities

const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;
const DEL: u8 = 0x7f;
const C1_ST: u8 = 0x9c;
const C1_DCS: u8 = 0x90;
const C1_SOS: u8 = 0x98;
const C1_CSI: u8 = 0x9b;
const C1_OSC: u8 = 0x9d;
const C1_PM: u8 = 0x9e;
const C1_APC: u8 = 0x9f;
const CAN: u8 = 0x18;
const SUB: u8 = 0x1a;
const MAX_STRING_SEQUENCE_BYTES: usize = 4096;
const MAX_CSI_SEQUENCE_BYTES: usize = 64;

#[inline]
fn parse_c1_at(bytes: &[u8], start: usize) -> Option<(u8, usize)> {
    let first = *bytes.get(start)?;
    if (0x80..=0x9f).contains(&first) {
        return Some((first, 1));
    }
    None
}

#[inline]
fn parse_csi(bytes: &[u8], start: usize) -> Option<usize> {
    // ECMA-48 / ISO 6429 CSI grammar:
    // - parameter bytes: 0x30..0x3F
    // - intermediate bytes: 0x20..0x2F
    // - final byte: 0x40..0x7E
    // (See ANSI escape code article on Wikipedia, CSI section.)
    let mut index = start;
    let mut phase = 0u8; // 0=parameter, 1=intermediate
    let mut consumed = 0usize;

    while index < bytes.len() {
        let byte = bytes[index];
        if byte == ESC {
            // VT100: ESC aborts current control sequence and starts a new one.
            return Some(index);
        }
        if byte == CAN || byte == SUB {
            // VT100: CAN/SUB abort current control sequence.
            return Some(index + 1);
        }

        consumed += 1;
        if consumed > MAX_CSI_SEQUENCE_BYTES {
            // Bound malformed or hostile input.
            return Some(index + 1);
        }

        if phase == 0 && (0x30..=0x3f).contains(&byte) {
            index += 1;
            continue;
        }
        if (0x20..=0x2f).contains(&byte) {
            phase = 1;
            index += 1;
            continue;
        }
        if (0x40..=0x7e).contains(&byte) {
            return Some(index + 1);
        }

        // Invalid CSI byte: abort sequence without consuming this byte.
        return Some(index);
    }

    None
}

#[inline]
fn parse_osc(bytes: &[u8], start: usize) -> Option<usize> {
    let mut consumed = 0usize;
    for index in start..bytes.len() {
        if bytes[index] == ESC && !(index + 1 < bytes.len() && bytes[index + 1] == b'\\') {
            // VT100: ESC aborts current sequence and begins a new one.
            return Some(index);
        }
        if bytes[index] == CAN || bytes[index] == SUB {
            return Some(index + 1);
        }

        if let Some((c1, len)) = parse_c1_at(bytes, index)
            && c1 == C1_ST
        {
            return Some(index + len);
        }

        match bytes[index] {
            BEL | C1_ST => return Some(index + 1),
            ESC if index + 1 < bytes.len() && bytes[index + 1] == b'\\' => return Some(index + 2),
            _ => {}
        }

        consumed += 1;
        if consumed > MAX_STRING_SEQUENCE_BYTES {
            // Cap unbounded strings when terminator is missing.
            return Some(index + 1);
        }
    }
    None
}

#[inline]
fn parse_st_terminated(bytes: &[u8], start: usize) -> Option<usize> {
    let mut consumed = 0usize;
    for index in start..bytes.len() {
        if bytes[index] == ESC && !(index + 1 < bytes.len() && bytes[index + 1] == b'\\') {
            return Some(index);
        }
        if bytes[index] == CAN || bytes[index] == SUB {
            return Some(index + 1);
        }

        if let Some((c1, len)) = parse_c1_at(bytes, index)
            && c1 == C1_ST
        {
            return Some(index + len);
        }

        match bytes[index] {
            C1_ST => return Some(index + 1),
            ESC if index + 1 < bytes.len() && bytes[index + 1] == b'\\' => return Some(index + 2),
            _ => {}
        }

        consumed += 1;
        if consumed > MAX_STRING_SEQUENCE_BYTES {
            return Some(index + 1);
        }
    }
    None
}

#[inline]
fn parse_ansi_sequence_bytes(bytes: &[u8]) -> Option<usize> {
    if bytes.is_empty() {
        return None;
    }

    if let Some((c1, c1_len)) = parse_c1_at(bytes, 0) {
        return match c1 {
            C1_CSI => parse_csi(bytes, c1_len),
            C1_OSC => parse_osc(bytes, c1_len),
            C1_DCS | C1_SOS | C1_PM | C1_APC => parse_st_terminated(bytes, c1_len),
            _ => Some(c1_len),
        };
    }

    match bytes[0] {
        ESC => {
            if bytes.len() < 2 {
                return None;
            }

            match bytes[1] {
                b'[' => parse_csi(bytes, 2),
                b']' => parse_osc(bytes, 2),
                b'P' | b'^' | b'_' | b'X' => parse_st_terminated(bytes, 2),
                next if next < 128 => Some(2),
                _ => Some(1),
            }
        }
        _ => None,
    }
}

/// Strip ANSI escape codes from text, keeping only plain text
pub fn strip_ansi(text: &str) -> String {
    let mut output = Vec::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == ESC
            && let Some(len) = parse_ansi_sequence_bytes(&bytes[i..])
        {
            i += len;
            continue;
        }
        if bytes[i] == ESC {
            // Incomplete/unterminated control sequence at end of available text.
            break;
        }

        if bytes[i] == b'\n' || bytes[i] == b'\r' || bytes[i] == b'\t' {
            output.push(bytes[i]);
            i += 1;
        } else if bytes[i] < 32 || bytes[i] == DEL {
            i += 1;
        } else {
            output.push(bytes[i]);
            i += 1;
        }
    }

    unsafe { String::from_utf8_unchecked(output) }
}

/// Strip ANSI escape codes from arbitrary bytes, preserving non-control bytes.
///
/// This is the preferred API when input may contain raw C1 (8-bit) controls.
pub fn strip_ansi_bytes(input: &[u8]) -> Vec<u8> {
    let mut output = Vec::with_capacity(input.len());
    let bytes = input;
    let mut i = 0;

    while i < bytes.len() {
        if (bytes[i] == ESC || parse_c1_at(bytes, i).is_some())
            && let Some(len) = parse_ansi_sequence_bytes(&bytes[i..])
        {
            i += len;
            continue;
        }
        if bytes[i] == ESC || parse_c1_at(bytes, i).is_some() {
            // Incomplete/unterminated control sequence at end of available text.
            break;
        }

        if bytes[i] == b'\n' || bytes[i] == b'\r' || bytes[i] == b'\t' {
            output.push(bytes[i]);
            i += 1;
        } else if bytes[i] < 32 || bytes[i] == DEL {
            i += 1;
        } else {
            output.push(bytes[i]);
            i += 1;
        }
    }
    output
}

/// Parse and determine the length of the ANSI escape sequence at the start of text
pub fn parse_ansi_sequence(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    parse_ansi_sequence_bytes(bytes)
}

/// Fast ASCII-only ANSI stripping for performance-critical paths
pub fn strip_ansi_ascii_only(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut last_valid = 0;

    while i < bytes.len() {
        if (bytes[i] == ESC || parse_c1_at(bytes, i).is_some())
            && let Some(len) = parse_ansi_sequence_bytes(&bytes[i..])
        {
            if last_valid < i {
                output.push_str(&text[last_valid..i]);
            }
            i += len;
            last_valid = i;
            continue;
        }

        i += 1;
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

#[cfg(test)]
mod tests {
    use super::{CAN, SUB, strip_ansi, strip_ansi_ascii_only};

    #[test]
    fn strips_esc_csi_sequences() {
        let input = "a\x1b[31mred\x1b[0mz";
        assert_eq!(strip_ansi(input), "aredz");
        assert_eq!(strip_ansi_ascii_only(input), "aredz");
    }

    #[test]
    fn utf8_encoded_c1_is_not_reprocessed_as_control() {
        // XTerm/ECMA-48: controls are processed once; decoded UTF-8 text is not reprocessed as C1.
        let input = "a\u{009b}31mred";
        assert_eq!(strip_ansi(input), input);
    }

    #[test]
    fn strip_removes_ascii_del_control() {
        let input = format!("a{}b", char::from(0x7f));
        assert_eq!(strip_ansi(&input), "ab");
    }

    #[test]
    fn csi_aborts_on_esc_then_new_sequence_parses() {
        let input = "a\x1b[31\x1b[32mgreen\x1b[0mz";
        assert_eq!(strip_ansi(input), "agreenz");
    }

    #[test]
    fn csi_aborts_on_can_and_sub() {
        let can = format!("a\x1b[31{}b", char::from(CAN));
        let sub = format!("a\x1b[31{}b", char::from(SUB));
        assert_eq!(strip_ansi(&can), "ab");
        assert_eq!(strip_ansi(&sub), "ab");
    }

    #[test]
    fn osc_aborts_on_esc_non_st() {
        let input = "a\x1b]title\x1b[31mred\x1b[0mz";
        assert_eq!(strip_ansi(input), "aredz");
    }

    #[test]
    fn incomplete_sequence_drops_tail() {
        let input = "text\x1b[31";
        assert_eq!(strip_ansi(input), "text");
    }

    #[test]
    fn strips_common_progress_redraw_sequences() {
        // Common pattern for dynamic CLI updates:
        // carriage return + erase line + redraw text.
        let input = "\r\x1b[2KProgress 10%\r\x1b[2KDone\n";
        assert_eq!(strip_ansi(input), "\rProgress 10%\rDone\n");
    }

    #[test]
    fn strips_cursor_navigation_sequences() {
        let input = "left\x1b[1D!\nup\x1b[1Arow";
        assert_eq!(strip_ansi(input), "left!\nuprow");
    }

    #[test]
    fn strip_ansi_bytes_supports_raw_c1_csi() {
        let input = [
            b'a', 0x9b, b'3', b'1', b'm', b'r', b'e', b'd', 0x9b, b'0', b'm', b'z',
        ];
        let out = super::strip_ansi_bytes(&input);
        assert_eq!(out, b"aredz");
    }

    #[test]
    fn strip_ansi_bytes_supports_raw_c1_osc_and_st() {
        let mut input = b"pre".to_vec();
        input.extend_from_slice(&[0x9d]);
        input.extend_from_slice(b"8;;https://example.com");
        input.extend_from_slice(&[0x9c]);
        input.extend_from_slice(b"link");
        input.extend_from_slice(&[0x9d]);
        input.extend_from_slice(b"8;;");
        input.extend_from_slice(&[0x9c]);
        input.extend_from_slice(b"post");
        let out = super::strip_ansi_bytes(&input);
        assert_eq!(out, b"prelinkpost");
    }

    #[test]
    fn csi_respects_parameter_intermediate_final_grammar() {
        // Parameter bytes ("1;2"), intermediate bytes (" "), then final ("m")
        let input = "a\x1b[1;2 mred\x1b[0mz";
        assert_eq!(strip_ansi(input), "aredz");
    }

    #[test]
    fn malformed_csi_does_not_consume_following_text() {
        // 0x10 is not valid CSI parameter/intermediate/final.
        let malformed = format!("a\x1b[12{}visible", char::from(0x10));
        assert_eq!(strip_ansi(&malformed), "avisible");
    }

    #[test]
    fn strips_wikipedia_sgr_8bit_color_pattern() {
        let input = "x\x1b[38;5;196mred\x1b[0my";
        assert_eq!(strip_ansi(input), "xredy");
    }

    #[test]
    fn strips_wikipedia_sgr_truecolor_pattern() {
        let input = "x\x1b[48;2;12;34;56mblock\x1b[0my";
        assert_eq!(strip_ansi(input), "xblocky");
    }

    #[test]
    fn strips_wikipedia_osc8_hyperlink_pattern() {
        let input = "go \x1b]8;;https://example.com\x1b\\here\x1b]8;;\x1b\\ now";
        assert_eq!(strip_ansi(input), "go here now");
    }

    #[test]
    fn strips_dec_private_mode_csi() {
        let input = "a\x1b[?25lb\x1b[?25hc";
        assert_eq!(strip_ansi(input), "abc");
    }
}
