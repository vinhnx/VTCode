//! Bidirectional conversion between byte-offset cursor positions (used by
//! `InputManager` / the `Editor` trait) and `(row, char_col)` positions used by
//! `ratatui_textarea::TextArea`.

/// Convert an absolute byte offset into a `(row, char_col)` pair suitable for
/// `TextArea::set_cursor`.
///
/// `lines` must be the current `TextArea::lines()` output.
pub(super) fn byte_offset_to_row_col(lines: &[String], byte_offset: usize) -> (usize, usize) {
    let mut consumed = 0usize;
    for (row, line) in lines.iter().enumerate() {
        let line_byte_len = line.len();
        if consumed + line_byte_len >= byte_offset {
            let col = line[..byte_offset - consumed].chars().count();
            return (row, col);
        }
        // +1 for the '\n' that TextArea strips from each line boundary
        consumed += line_byte_len + 1;
    }
    // Past the end: clamp to last line, last column
    let last_row = lines.len().saturating_sub(1);
    let last_col = lines.last().map_or(0, |l| l.chars().count());
    (last_row, last_col)
}

/// Convert a `(row, char_col)` pair from `TextArea::cursor()` back to an
/// absolute byte offset.
pub(super) fn row_col_to_byte_offset(lines: &[String], row: usize, char_col: usize) -> usize {
    let mut offset = 0usize;
    for (i, line) in lines.iter().enumerate() {
        if i == row {
            let byte_col = line
                .char_indices()
                .nth(char_col)
                .map_or(line.len(), |(idx, _)| idx);
            return offset + byte_col;
        }
        offset += line.len() + 1; // +1 for '\n'
    }
    // Past the end
    offset
}

/// Convert a character column index within a single line to a byte offset.
pub(super) fn char_col_to_byte_offset(line: &str, char_col: usize) -> usize {
    line.char_indices()
        .nth(char_col)
        .map_or(line.len(), |(idx, _)| idx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_start() {
        let lines = vec!["hello".to_string()];
        assert_eq!(byte_offset_to_row_col(&lines, 0), (0, 0));
        assert_eq!(row_col_to_byte_offset(&lines, 0, 0), 0);
    }

    #[test]
    fn single_line_middle() {
        let lines = vec!["hello".to_string()];
        assert_eq!(byte_offset_to_row_col(&lines, 3), (0, 3));
        assert_eq!(row_col_to_byte_offset(&lines, 0, 3), 3);
    }

    #[test]
    fn single_line_end() {
        let lines = vec!["hello".to_string()];
        assert_eq!(byte_offset_to_row_col(&lines, 5), (0, 5));
        assert_eq!(row_col_to_byte_offset(&lines, 0, 5), 5);
    }

    #[test]
    fn multi_line_first_line() {
        let lines = vec!["hello".to_string(), "world".to_string()];
        assert_eq!(byte_offset_to_row_col(&lines, 3), (0, 3));
        assert_eq!(row_col_to_byte_offset(&lines, 0, 3), 3);
    }

    #[test]
    fn multi_line_second_line() {
        let lines = vec!["hello".to_string(), "world".to_string()];
        // byte offset 6 = 'h','e','l','l','o','\n' = start of "world"
        assert_eq!(byte_offset_to_row_col(&lines, 6), (1, 0));
        assert_eq!(row_col_to_byte_offset(&lines, 1, 0), 6);
    }

    #[test]
    fn multi_line_second_line_offset() {
        let lines = vec!["hello".to_string(), "world".to_string()];
        // byte offset 9 = 'h','e','l','l','o','\n','w','o','r' = 'l' in "world"
        assert_eq!(byte_offset_to_row_col(&lines, 9), (1, 3));
        assert_eq!(row_col_to_byte_offset(&lines, 1, 3), 9);
    }

    #[test]
    fn utf8_multibyte() {
        let lines = vec!["你好".to_string()];
        // '你' = 3 bytes, '好' = 3 bytes
        assert_eq!(byte_offset_to_row_col(&lines, 0), (0, 0));
        assert_eq!(byte_offset_to_row_col(&lines, 3), (0, 1));
        assert_eq!(byte_offset_to_row_col(&lines, 6), (0, 2));
        assert_eq!(row_col_to_byte_offset(&lines, 0, 0), 0);
        assert_eq!(row_col_to_byte_offset(&lines, 0, 1), 3);
        assert_eq!(row_col_to_byte_offset(&lines, 0, 2), 6);
    }

    #[test]
    fn roundtrip_multiline() {
        let lines = vec!["abc".to_string(), "def".to_string(), "ghi".to_string()];
        // Full content is "abc\ndef\nghi" = 11 bytes
        for byte_offset in 0..=11 {
            let (row, col) = byte_offset_to_row_col(&lines, byte_offset);
            let back = row_col_to_byte_offset(&lines, row, col);
            assert_eq!(back, byte_offset, "roundtrip failed at byte {byte_offset}");
        }
    }

    #[test]
    fn past_end_clamps() {
        let lines = vec!["hello".to_string()];
        let (row, col) = byte_offset_to_row_col(&lines, 100);
        assert_eq!(row, 0);
        assert_eq!(col, 5);
    }

    #[test]
    fn empty_content() {
        let lines = vec!["".to_string()];
        assert_eq!(byte_offset_to_row_col(&lines, 0), (0, 0));
        assert_eq!(row_col_to_byte_offset(&lines, 0, 0), 0);
    }
}
