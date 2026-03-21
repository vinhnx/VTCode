pub(super) struct RawVtBuffer {
    bytes: Vec<u8>,
    max_bytes: usize,
    truncated: bool,
}

pub(super) struct RawVtSnapshot {
    pub(super) bytes: Vec<u8>,
    pub(super) was_truncated: bool,
}

impl RawVtBuffer {
    pub(super) fn new(max_bytes: usize) -> Self {
        Self {
            bytes: Vec::new(),
            max_bytes: max_bytes.max(1),
            truncated: false,
        }
    }

    pub(super) fn push(&mut self, chunk: &[u8]) {
        if chunk.is_empty() {
            return;
        }

        if chunk.len() >= self.max_bytes {
            self.truncated = true;
            self.bytes.clear();
            self.bytes
                .extend_from_slice(&chunk[chunk.len().saturating_sub(self.max_bytes)..]);
            return;
        }

        let overflow = self
            .bytes
            .len()
            .saturating_add(chunk.len())
            .saturating_sub(self.max_bytes);
        if overflow > 0 {
            self.truncated = true;
            self.bytes.drain(..overflow);
        }

        self.bytes.extend_from_slice(chunk);
    }

    pub(super) fn snapshot(&self) -> RawVtSnapshot {
        RawVtSnapshot {
            bytes: self.bytes.clone(),
            was_truncated: self.truncated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RawVtBuffer;

    #[test]
    fn keeps_latest_bytes_within_capacity() {
        let mut buffer = RawVtBuffer::new(5);
        buffer.push(b"abc");
        buffer.push(b"de");
        buffer.push(b"fg");

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.bytes, b"cdefg");
        assert!(snapshot.was_truncated);
    }

    #[test]
    fn large_chunk_replaces_existing_buffer_tail() {
        let mut buffer = RawVtBuffer::new(4);
        buffer.push(b"ab");
        buffer.push(b"012345");

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.bytes, b"2345");
        assert!(snapshot.was_truncated);
    }

    #[test]
    fn untouched_buffer_reports_not_truncated() {
        let mut buffer = RawVtBuffer::new(8);
        buffer.push(b"abc");

        let snapshot = buffer.snapshot();
        assert_eq!(snapshot.bytes, b"abc");
        assert!(!snapshot.was_truncated);
    }
}
