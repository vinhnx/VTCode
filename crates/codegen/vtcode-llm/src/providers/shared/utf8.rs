//! Incremental UTF-8 decoding across network chunk boundaries.

/// Incrementally decodes a byte stream into UTF-8 text.
///
/// Network chunks can split a multibyte code point (CJK, emoji, accented
/// characters, smart quotes) across boundaries. Decoding each chunk
/// independently with `String::from_utf8_lossy` corrupts such code points into
/// `U+FFFD` replacement characters. This decoder buffers any trailing incomplete
/// sequence until the rest of its bytes arrive, while still replacing genuinely
/// invalid bytes with `U+FFFD` (matching `from_utf8_lossy` semantics).
#[derive(Debug, Default)]
pub struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    /// Appends `bytes` and returns the decodable UTF-8 prefix. A trailing
    /// incomplete multibyte sequence is retained for the next call.
    pub(crate) fn push(&mut self, bytes: &[u8]) -> String {
        self.pending.extend_from_slice(bytes);
        let mut out = String::with_capacity(bytes.len());
        loop {
            match std::str::from_utf8(&self.pending) {
                Ok(text) => {
                    out.push_str(text);
                    self.pending.clear();
                    break;
                }
                Err(err) => {
                    let valid = err.valid_up_to();
                    if let Some(valid_bytes) = self.pending.get(..valid) {
                        // `valid_bytes` is guaranteed valid UTF-8 by `valid_up_to`.
                        out.push_str(&String::from_utf8_lossy(valid_bytes));
                    }
                    match err.error_len() {
                        // Genuinely invalid sequence: emit replacement and skip it.
                        Some(invalid_len) => {
                            out.push('\u{FFFD}');
                            self.pending.drain(..valid + invalid_len);
                        }
                        // Incomplete trailing sequence: keep it for the next push.
                        None => {
                            self.pending.drain(..valid);
                            break;
                        }
                    }
                }
            }
        }
        out
    }

    /// Appends `bytes` and writes the decodable UTF-8 prefix directly into
    /// `out`. A trailing incomplete multibyte sequence is retained for the
    /// next call.
    ///
    /// This avoids the intermediate `String` allocation that `push` creates
    /// when the caller only needs bytes (e.g. feeding an SSE byte buffer).
    /// The output is identical to `push(bytes).into_bytes()`.
    pub(crate) fn push_bytes(&mut self, bytes: &[u8], out: &mut Vec<u8>) {
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
                        // `valid_bytes` is guaranteed valid UTF-8 by `valid_up_to`,
                        // so a direct byte copy is safe and avoids `from_utf8_lossy`.
                        out.extend_from_slice(valid_bytes);
                    }
                    match err.error_len() {
                        // Genuinely invalid sequence: emit replacement and skip it.
                        Some(invalid_len) => {
                            out.extend_from_slice("\u{FFFD}".as_bytes());
                            self.pending.drain(..valid + invalid_len);
                        }
                        // Incomplete trailing sequence: keep it for the next push.
                        None => {
                            self.pending.drain(..valid);
                            break;
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_stream_decoder_push_bytes_matches_push() {
        // Complete valid UTF-8 across multiple chunks.
        let full = "data: {\"hello\":\"world\"}\n\n".as_bytes();
        let mut dec_str = Utf8StreamDecoder::new();
        let mut dec_bytes = Utf8StreamDecoder::new();
        for (i, &byte) in full.iter().enumerate() {
            let mut out = Vec::new();
            dec_bytes.push_bytes(std::slice::from_ref(&byte), &mut out);
            let s = dec_str.push(std::slice::from_ref(&byte));
            assert_eq!(out, s.into_bytes(), "byte {i}: push_bytes must match push");
        }
        // Both decoders should have empty pending buffers after complete input.
        let mut tail = Vec::new();
        dec_bytes.push_bytes(&[], &mut tail);
        assert!(tail.is_empty());
    }

    #[test]
    fn utf8_stream_decoder_push_bytes_handles_split_multibyte() {
        // Split a multibyte character (U+00E9 = 0xC3 0xA9) across chunks.
        let mut dec = Utf8StreamDecoder::new();
        let mut out = Vec::new();
        dec.push_bytes(&[0xC3], &mut out);
        assert!(out.is_empty(), "incomplete multibyte should produce no output");
        dec.push_bytes(&[0xA9, b'h', b'i'], &mut out);
        assert_eq!(std::str::from_utf8(&out).unwrap(), "\u{00E9}hi");
    }

    #[test]
    fn utf8_stream_decoder_push_bytes_emits_replacement_for_invalid() {
        // 0xFF is never a valid UTF-8 lead byte.
        let mut dec = Utf8StreamDecoder::new();
        let mut out = Vec::new();
        dec.push_bytes(&[b'a', 0xFF, b'b'], &mut out);
        assert_eq!(std::str::from_utf8(&out).unwrap(), "a\u{FFFD}b");
    }
}
