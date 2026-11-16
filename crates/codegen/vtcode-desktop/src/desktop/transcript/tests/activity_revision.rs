//! Activity-only revision regression tests.

use super::*;

#[test]
fn activity_revision_ignores_output_text_deltas() {
    let mut transcript = Transcript::default();
    transcript.push(item(1, "tool", "running"));
    // Establish output presence first: the none→some boundary does change an
    // activity row (`has_output`) and must bump. Everything after it is text.
    transcript.set_output("1", Some("first chunk".into()));
    let activity_before = transcript.activity_revision();
    let revision_before = transcript.revision();

    // A streaming command replaces its captured text on every chunk. The row
    // still has output, so the Activity projection must stay cached.
    transcript.set_output("1", Some("first chunk\nsecond chunk".into()));
    transcript.set_output_truncated("1", true);
    assert_eq!(transcript.activity_revision(), activity_before);
    assert!(transcript.revision() > revision_before, "output consumers still see every delta");

    // The none→some boundary changes `has_output`, which activity rows read.
    let mut transcript = Transcript::default();
    transcript.push(item(1, "tool", "running"));
    let before = transcript.activity_revision();
    transcript.set_output("1", Some("output".into()));
    assert_eq!(transcript.activity_revision(), before + 1);
}
