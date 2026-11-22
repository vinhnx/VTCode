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

#[test]
fn upsert_output_presence_bumps_activity_revision() {
    // upsert's no-content-change branch attaches output to a row that had
    // none. That crosses the same none→some `has_output` boundary as
    // set_output, so the Activity projection cache must be invalidated too.
    let mut transcript = Transcript::default();
    let row = item(1, "tool", "running");
    transcript.push(row);
    let before = transcript.activity_revision();

    let mut with_output = item(1, "tool", "running");
    with_output.output = Some("captured".into());
    transcript.upsert(with_output);
    assert_eq!(
        transcript.activity_revision(),
        before + 1,
        "upsert crossing none→some output must bump activity_revision",
    );

    // A later text delta on a row that already has output must NOT bump:
    // the Activity projection reads presence, not the captured bytes.
    let after_presence = transcript.activity_revision();
    let mut grown = item(1, "tool", "running");
    grown.output = Some("captured and more".into());
    transcript.upsert(grown);
    assert_eq!(
        transcript.activity_revision(),
        after_presence,
        "output text growth must keep the Activity cache warm",
    );
}
