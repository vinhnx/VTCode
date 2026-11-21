use super::*;

#[test]
fn leak_scan_flags_native_items_and_image_bytes() {
    assert!(leaked_native_artifacts("{\"type\":\"computer_call\"}").contains(&"\"computer_call"));
    assert!(leaked_native_artifacts("data:image/png;base64,AAA").contains(&"data:image/"));
    assert!(leaked_native_artifacts("iVBORw0KGgoAAAANSUhEUg").contains(&"iVBORw0KGgo"));
    assert!(leaked_native_artifacts("plain redacted summary").is_empty());
}
