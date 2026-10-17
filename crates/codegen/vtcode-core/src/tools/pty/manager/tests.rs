use std::io::{self, Read};

use super::{PTY_OUTPUT_TRUNCATED_MARKER, read_pty_output_capped};

struct InterruptedOnce<R> {
    inner: R,
    interrupted: bool,
}

impl<R: Read> Read for InterruptedOnce<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::Error::from(io::ErrorKind::Interrupted));
        }
        self.inner.read(buf)
    }
}

#[test]
fn output_under_cap_is_returned_verbatim() {
    let output = read_pty_output_capped(&b"hello"[..], 5).unwrap();
    assert_eq!(output, b"hello");
}

#[test]
fn output_over_cap_is_truncated_across_read_boundaries_and_fully_drained() {
    // 3 reads of 4096 + remainder; cap falls mid-chunk on the second read.
    let data = vec![b'x'; 10_000];
    let mut reader = &data[..];
    let output = read_pty_output_capped(&mut reader, 5_000).unwrap();

    let mut expected = vec![b'x'; 5_000];
    expected.extend_from_slice(PTY_OUTPUT_TRUNCATED_MARKER);
    assert_eq!(output, expected);
    assert!(reader.is_empty(), "surplus must be drained so the child never blocks");
}

#[test]
fn zero_cap_keeps_only_marker() {
    let output = read_pty_output_capped(&b"abc"[..], 0).unwrap();
    assert_eq!(output, PTY_OUTPUT_TRUNCATED_MARKER);
}

#[test]
fn interrupted_reads_are_retried() {
    let reader = InterruptedOnce { inner: &b"ok"[..], interrupted: false };
    assert_eq!(read_pty_output_capped(reader, 16).unwrap(), b"ok");
}
