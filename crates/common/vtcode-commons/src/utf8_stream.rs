//! Incremental UTF-8 decoding across chunk boundaries (network streams, pipe reads).

use std::borrow::Cow;

/// Incrementally decodes a byte stream into UTF-8 text.
///
/// Chunks can split a multibyte code point (CJK, emoji, accented characters,
/// smart quotes) across boundaries. Decoding each chunk independently with
/// `String::from_utf8_lossy` corrupts such code points into `U+FFFD`
/// replacement characters. This decoder buffers any trailing incomplete
/// sequence until the rest of its bytes arrive, while still replacing genuinely
/// invalid bytes with `U+FFFD` (matching `from_utf8_lossy` semantics). Call
/// [`Utf8StreamDecoder::finish`] at end of stream so a truncated final sequence
/// becomes `U+FFFD` instead of vanishing.
#[derive(Debug, Default)]
pub struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `bytes` and returns the decodable UTF-8 prefix. A trailing
    /// incomplete multibyte sequence is retained for the next call.
    pub fn push(&mut self, bytes: &[u8]) -> String {
        let mut out = Vec::with_capacity(bytes.len());
        self.push_bytes(bytes, &mut out);
        // `push_bytes` only ever writes valid UTF-8, so this never allocates a lossy copy.
        String::from_utf8(out).unwrap_or_else(|error| String::from_utf8_lossy(error.as_bytes()).into_owned())
    }

    /// Like [`Utf8StreamDecoder::push`], but borrows `bytes` without copying when
    /// nothing is pending and the chunk is valid UTF-8 (the common case for
    /// process output), so hot read loops avoid a per-chunk allocation.
    pub fn push_cow<'a>(&mut self, bytes: &'a [u8]) -> Cow<'a, str> {
        if self.pending.is_empty()
            && let Ok(text) = std::str::from_utf8(bytes)
        {
            return Cow::Borrowed(text);
        }
        Cow::Owned(self.push(bytes))
    }

    /// Appends `bytes` and writes the decodable UTF-8 prefix directly into
    /// `out`. A trailing incomplete multibyte sequence is retained for the
    /// next call.
    ///
    /// This avoids the intermediate `String` allocation that `push` creates
    /// when the caller only needs bytes (e.g. feeding an SSE byte buffer).
    pub fn push_bytes(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
        self.pending.extend_from_slice(bytes);
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    out.extend_from_slice(text.as_bytes());
                    self.pending.clear();
                    break;
                }
                Err(err) => {
                    let valid = err.valid_up_to();
                    if let Some(valid_bytes) = self.pending.get(..valid) {
                        // `valid_bytes` is guaranteed valid UTF-8 by `valid_up_to`.
                        out.extend_from_slice(valid_bytes);
                    }
                    match err.error_len() {
                        // Genuinely invalid sequence: emit replacement and skip it.
                        Some(invalid_len) => {
                            out.extend_from_slice("\u{FFFD}".as_bytes());
                            let _ = self.pending.drain(..valid + invalid_len);
                        }
                        // Incomplete trailing sequence: keep it for the next push.
                        None => {
                            let _ = self.pending.drain(..valid);
                            break;
                        }
                    }
                }
            }
        }
    }

    /// Ends the stream: returns any retained incomplete sequence lossily
    /// decoded (as `U+FFFD`), leaving the decoder empty.
    pub fn finish(&mut self) -> String {
        let pending = std::mem::take(&mut self.pending);
        String::from_utf8_lossy(&pending).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_bytes_matches_push_byte_by_byte() {
        let full = "data: {\"hello\":\"world\"} xin chào 世界\n\n".as_bytes();
        let mut dec_str = Utf8StreamDecoder::new();
        let mut dec_bytes = Utf8StreamDecoder::new();
        let mut joined = String::new();
        for (i, &byte) in full.iter().enumerate() {
            let mut out = Vec::new();
            dec_bytes.push_bytes(std::slice::from_ref(&byte), &mut out);
            let s = dec_str.push(std::slice::from_ref(&byte));
            assert_eq!(out, s.clone().into_bytes(), "byte {i}: push_bytes must match push");
            joined.push_str(&s);
        }
        assert_eq!(joined.as_bytes(), full);
        assert!(dec_str.finish().is_empty());
    }

    #[test]
    fn split_multibyte_is_held_until_complete() {
        // U+00E9 = 0xC3 0xA9 split across chunks.
        let mut dec = Utf8StreamDecoder::new();
        assert!(dec.push(&[0xC3]).is_empty(), "incomplete multibyte should produce no output");
        assert_eq!(dec.push(&[0xA9, b'h', b'i']), "\u{00E9}hi");
    }

    #[test]
    fn invalid_bytes_become_replacement_characters() {
        // 0xFF is never a valid UTF-8 lead byte.
        let mut dec = Utf8StreamDecoder::new();
        assert_eq!(dec.push(&[b'a', 0xFF, b'b']), "a\u{FFFD}b");
    }

    #[test]
    fn push_cow_borrows_valid_chunks_and_buffers_splits() {
        let mut dec = Utf8StreamDecoder::new();
        assert!(matches!(dec.push_cow("plain 世".as_bytes()), Cow::Borrowed("plain 世")));
        // `界` = E7 95 8C split across chunks: nothing until complete, then owned.
        assert_eq!(dec.push_cow(&[0xE7, 0x95]), "");
        let completed = dec.push_cow(&[0x8C, b'!']);
        assert!(matches!(completed, Cow::Owned(_)));
        assert_eq!(completed, "界!");
        assert!(dec.finish().is_empty());
    }

    #[test]
    fn finish_flushes_a_truncated_final_sequence() {
        let mut dec = Utf8StreamDecoder::new();
        // First two bytes of the three-byte `—` (0xE2 0x80 0x94).
        assert_eq!(dec.push(&[b'x', 0xE2, 0x80]), "x");
        assert_eq!(dec.finish(), "\u{FFFD}");
        assert!(dec.finish().is_empty(), "finish leaves the decoder empty");
    }
}
