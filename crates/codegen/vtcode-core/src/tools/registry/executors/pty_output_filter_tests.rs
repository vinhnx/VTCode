use super::filter_pty_output;

#[test]
fn normalizes_crlf_sequences() {
    let raw = "a\r\nb\rc\n";
    assert_eq!(filter_pty_output(raw), "a\nb\nc\n");
}
