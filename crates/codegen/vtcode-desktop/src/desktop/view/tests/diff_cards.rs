use super::*;

#[gpui_kit::test]
fn diff_card_file_rows_offer_open_copy_path_and_copy_diff_actions(cx: &mut TestAppContext) {
    use vtcode_exec_events::{FileChangeItem, FileUpdateChange, PatchApplyStatus, PatchChangeKind};

    let workspace = tempfile::TempDir::new().unwrap();
    let readme_section =
        "diff --git a/README.md b/README.md\n--- a/README.md\n+++ b/README.md\n@@ -1 +1 @@\n-old-readme\n+new-readme\n";
    let app_section =
        "diff --git a/src/app.rs b/src/app.rs\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old-app\n+new-app\n";
    let multi_patch = format!("{readme_section}{app_section}");
    let single_patch = "@@ -1 +1 @@\n-old-single\n+new-single\n";
    // Tall enough that all three expanded cards stay inside the viewport: the
    // scroller follows the tail, which would push the first card out at 900px.
    let (window, view) = open_desktop_sized(cx, 1600., 1400.);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, cx| {
            view.workspace = Some(workspace.path().to_path_buf());
            view.preferences.set_file_open_destination(FileOpenDestination::VsCode);
            view.test_launch_intercept = Some(TestLaunchIntercept::default());
            for (id, changes, patch) in [
                (
                    "change-multi",
                    vec![
                        FileUpdateChange {
                            path: "README.md".into(),
                            kind: PatchChangeKind::Update,
                        },
                        FileUpdateChange {
                            path: "src/app.rs".into(),
                            kind: PatchChangeKind::Update,
                        },
                    ],
                    Some(multi_patch.as_str()),
                ),
                (
                    "change-single",
                    vec![FileUpdateChange {
                        path: "notes.txt".into(),
                        kind: PatchChangeKind::Update,
                    }],
                    Some(single_patch),
                ),
                (
                    "change-empty",
                    vec![FileUpdateChange {
                        path: "skipped.txt".into(),
                        kind: PatchChangeKind::Update,
                    }],
                    None,
                ),
            ] {
                view.upsert_diff_card(
                    id.into(),
                    &FileChangeItem {
                        diff_incomplete: None,
                        changes,
                        status: PatchApplyStatus::Completed,
                        unified_diff: patch.map(str::to_string),
                        additions: None,
                        deletions: None,
                    },
                );
            }
            cx.notify();
        });
        window.render_frame(cx);

        let row_button = |action: &str, id: &str, path: &str| {
            super::super::opaque_path_element_id(&format!("diff-{action}-{id}"), path)
        };

        // Every file row carries open + copy-path actions.
        for (id, path) in [
            ("change-multi", "README.md"),
            ("change-multi", "src/app.rs"),
            ("change-single", "notes.txt"),
            ("change-empty", "skipped.txt"),
        ] {
            assert!(window.try_find(row_button("open", id, path)).is_some(), "open action renders for {path}");
            assert!(
                window.try_find(row_button("copy-path", id, path)).is_some(),
                "copy-path action renders for {path}"
            );
        }

        // The header toggle and copy actions share one trailing lane instead
        // of spreading across the row.
        let toggle = window.find("diff-change-multi").bounds();
        let copy = window.find("copy-patch-change-multi").bounds();
        let column = window.find("transcript-content-change-multi").bounds();
        assert!(
            copy.left() - toggle.right() <= px(8.),
            "header actions stay adjacent: toggle={toggle:?} copy={copy:?}"
        );
        assert!(
            toggle.left() > column.center().x,
            "header actions dock trailing: toggle={toggle:?} column={column:?}"
        );

        // Copy-path copies the relative path.
        window.click(row_button("copy-path", "change-multi", "README.md"), cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some("README.md"));

        // Copy-diff copies only the file's own section, never its neighbor's.
        assert!(
            window.try_find(row_button("copy-diff", "change-multi", "README.md")).is_some(),
            "copy-diff renders when the file owns a patch segment"
        );
        window.click(row_button("copy-diff", "change-multi", "README.md"), cx);
        let copied = cx.read_from_clipboard().unwrap().text().unwrap();
        assert!(copied.contains("new-readme"), "copies the file's hunk: {copied:?}");
        assert!(!copied.contains("new-app"), "excludes the neighbor's hunk: {copied:?}");

        // A headerless single-file patch copies whole.
        window.click(row_button("copy-diff", "change-single", "notes.txt"), cx);
        assert_eq!(cx.read_from_clipboard().unwrap().text().as_deref(), Some(single_patch));

        // Without an attributable segment the copy-diff action stays hidden,
        // while open and copy-path still work.
        assert!(
            window
                .try_find(row_button("copy-diff", "change-empty", "skipped.txt"))
                .is_none(),
            "copy-diff hides when no segment names the file"
        );

        // Open routes through the configured external editor.
        window.click(row_button("open", "change-multi", "src/app.rs"), cx);
        assert_eq!(
            view.read(cx).test_launch_intercept.as_ref().unwrap().calls,
            vec![ExternalLaunchCall {
                rel_path: "src/app.rs".into(),
                editor: ExternalEditor::VsCode
            }]
        );
    })
    .unwrap();
}

#[test]
fn file_patch_segment_attributes_sections_without_prefix_collisions() {
    use super::super::file_patch_segment;

    // Asymmetric prefix pair: each query resolves to its own section only.
    let app_section =
        "diff --git a/src/app.rs b/src/app.rs\n--- a/src/app.rs\n+++ b/src/app.rs\n@@ -1 +1 @@\n-old-app\n+new-app\n";
    let bak_section = "diff --git a/src/app.rs.bak b/src/app.rs.bak\n--- a/src/app.rs.bak\n+++ b/src/app.rs.bak\n@@ -1 +1 @@\n-old-bak\n+new-bak\n";
    let patch = format!("{app_section}{bak_section}");
    assert_eq!(file_patch_segment(&patch, "src/app.rs", 2), Some(app_section));
    assert_eq!(file_patch_segment(&patch, "src/app.rs.bak", 2), Some(bak_section));
    assert_eq!(file_patch_segment(&patch, "src/other.rs", 2), None);

    // `---` / `+++` sections without `diff --git` headers split the same way.
    let first = "--- a/one.rs\n+++ b/one.rs\n@@ -1 +1 @@\n-one-old\n+one-new\n";
    let second = "--- a/two.rs\n+++ b/two.rs\n@@ -1 +1 @@\n-two-old\n+two-new\n";
    let bare_headers = format!("{first}{second}");
    assert_eq!(file_patch_segment(&bare_headers, "one.rs", 2), Some(first));
    assert_eq!(file_patch_segment(&bare_headers, "two.rs", 2), Some(second));

    // A removed `-- x` comment inside a hunk never splits the file.
    let trap = "--- a/query.sql\n+++ b/query.sql\n@@ -1,2 +1,2 @@\n--- old filter\n+new filter\n context\n--- a/next.sql\n+++ b/next.sql\n@@ -1 +1 @@\n-next-old\n+next-new\n";
    let query = file_patch_segment(trap, "query.sql", 2).expect("query section");
    assert!(query.contains("--- old filter"));
    assert!(query.contains("+new filter"));
    assert!(!query.contains("next-new"));

    // Headerless patches belong to single-file change-sets only.
    let headerless = "@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(file_patch_segment(headerless, "only.rs", 1), Some(headerless));
    assert_eq!(file_patch_segment(headerless, "only.rs", 2), None);

    // Quoted paths with spaces match; empties never do.
    let spaced = "diff --git \"a/my file.rs\" \"b/my file.rs\"\n--- \"a/my file.rs\"\n+++ \"b/my file.rs\"\n@@ -1 +1 @@\n-old\n+new\n";
    assert_eq!(file_patch_segment(spaced, "my file.rs", 1), Some(spaced));
    assert_eq!(file_patch_segment("", "my file.rs", 1), None);
    assert_eq!(file_patch_segment(spaced, "", 1), None);

    // Sections without `---` / `+++` entries still match on the `diff --git`
    // header; headers naming another file fail closed instead.
    let binary =
        "diff --git a/logo.png b/logo.png\nnew file mode 100644\nBinary files /dev/null and b/logo.png differ\n";
    assert_eq!(file_patch_segment(binary, "logo.png", 1), Some(binary));
    assert_eq!(file_patch_segment(binary, "other.png", 1), None);
}

#[gpui_kit::test]
fn inspector_changes_summary_uses_singular_file_count(cx: &mut TestAppContext) {
    let (window, view) = open_desktop(cx);
    cx.update_window(window, |_, window, cx| {
        view.update(cx, |view, _| {
            view.inspector_open = true;
            let mut first = TranscriptItem::new("first".into(), "diff".into(), "Edits".into(), String::new());
            first.diff = Some(super::super::DiffCard {
                files: Vec::new(),
                additions: Some(1),
                deletions: Some(0),
                incomplete: false,
                failed: false,
                patch: "--- a/solo.rs\n+++ b/solo.rs\n@@ -1 +1 @@\n-old\n+new\n".into(),
            });
            view.transcript.push(first);
            view.changes = super::super::ChangesProjection::rebuild(&view.transcript);
        });
        window.render_frame(cx);
        assert_eq!(window.find("inspector-changes-summary").label(), Some("1 file"));
        view.update(cx, |view, _| {
            let mut second = TranscriptItem::new("second".into(), "diff".into(), "Edits".into(), String::new());
            second.diff = Some(super::super::DiffCard {
                files: Vec::new(),
                additions: Some(1),
                deletions: Some(0),
                incomplete: false,
                failed: false,
                patch: "--- a/other.rs\n+++ b/other.rs\n@@ -1 +1 @@\n-old\n+new\n".into(),
            });
            view.transcript.push(second);
            view.changes = super::super::ChangesProjection::rebuild(&view.transcript);
        });
        window.render_frame(cx);
        assert_eq!(window.find("inspector-changes-summary").label(), Some("2 files"));
    })
    .unwrap();
}
