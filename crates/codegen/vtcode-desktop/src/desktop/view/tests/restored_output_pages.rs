use super::*;

#[test]
fn restored_output_pages_strip_structured_envelope_but_keep_spool_note() {
    let mut item = TranscriptItem::new("output".into(), "tool".into(), "Command".into(), "completed".into());
    item.output = Some("prose head".into());
    item.output_item_id = Some("capture".into());
    let mut pages = std::collections::HashMap::new();
    let mut page = vtcode_desktop_protocol::SessionOutputPage {
        session_id: "session".into(),
        request_id: "request".into(),
        item_id: "capture".into(),
        offset: 0,
        next_offset: 1,
        total_bytes: 1,
        text: "\nStructured output:\n{\"backend\": \"pipe\", \"spool_note\": \"Spooled to out.txt\"}".into(),
        has_more: false,
    };
    assert!(append_restored_output_page(&mut item, &mut pages, &mut page));
    let captured = item.output.as_ref().unwrap();
    assert_eq!(captured.as_ref(), "prose head\nSpooled to out.txt");
    assert_eq!(restored_output_for_copy("capture", captured, &pages, false), "prose head\nSpooled to out.txt");
}

#[test]
fn restored_output_pages_strip_blocks_straddling_page_boundaries() {
    let mut item = TranscriptItem::new("output".into(), "tool".into(), "Command".into(), "completed".into());
    item.output = Some("prose head".into());
    item.output_item_id = Some("capture".into());
    let mut pages = std::collections::HashMap::new();
    for (index, text, has_more) in [
        (0, "\nStructured output:\n{\"backend\": \"pi", true),
        (1, "pe\", \"pid\": 7}", false),
    ] {
        let mut page = vtcode_desktop_protocol::SessionOutputPage {
            session_id: "session".into(),
            request_id: format!("request-{index}"),
            item_id: "capture".into(),
            offset: 0,
            next_offset: 1,
            total_bytes: 1,
            text: text.into(),
            has_more,
        };
        assert!(append_restored_output_page(&mut item, &mut pages, &mut page));
    }
    // The block completes only in the join, so only the join strips it: the
    // final preview and the copy carry the prose, never the envelope.
    let captured = item.output.as_ref().unwrap();
    assert_eq!(captured.as_ref(), "prose head");
    assert_eq!(restored_output_for_copy("capture", captured, &pages, false), "prose head");
}
