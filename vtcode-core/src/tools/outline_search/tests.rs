use super::{
    BoundedRecordRead, CODE_SEARCH_OUTLINE_BYTE_CAP, read_bounded_record,
    search_declarations_bounded, smart_case_eq,
};
use crate::tools::ast_grep_binary::set_ast_grep_binary_override_for_tests as set_read_only_ast_grep_override_for_tests;
use crate::tools::ast_grep_language::AstGrepLanguage;
use serial_test::serial;
use std::{fs, path::PathBuf};
use tempfile::TempDir;

/// Write a fake `ast-grep` executable that ignores its args and writes the
/// given body to stdout, exiting 0. The body is staged in a sidecar file so
/// real newlines are preserved (embedding it via `printf` mangles NDJSON).
fn write_fake_sg(stdout_body: &str) -> (TempDir, PathBuf) {
    let script_dir = TempDir::new().expect("script tempdir");
    let output_path = script_dir.path().join("output.txt");
    fs::write(&output_path, stdout_body).expect("write output");
    let script_path = script_dir.path().join("sg");
    let script = format!("#!/bin/sh\ncat {:?}\n", output_path.as_os_str());
    fs::write(&script_path, script).expect("write fake sg");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod");
    }
    (script_dir, script_path)
}

const TWO_FILE_STREAM: &str = r#"{"path":"src/lib.rs","language":"Rust","items":[{"role":"item","symbolType":"function","name":"alpha","range":{"byteOffset":{"start":0,"end":10},"start":{"line":0,"column":0},"end":{"line":0,"column":10}},"signature":"pub fn alpha()","astKind":"function_item","isImport":false,"isExported":true,"members":[]},{"role":"item","symbolType":"struct","name":"Foo","range":{"byteOffset":{"start":11,"end":40},"start":{"line":1,"column":0},"end":{"line":3,"column":1}},"signature":"pub struct Foo","astKind":"struct_item","isImport":false,"isExported":true,"members":[{"role":"member","symbolType":"field","name":"x","range":{"byteOffset":{"start":22,"end":28},"start":{"line":2,"column":4},"end":{"line":2,"column":10}},"signature":"","astKind":"field_definition","isPublic":true}]}]}
{"path":"src/util.rs","language":"Rust","items":[{"role":"item","symbolType":"function","name":"beta","range":{"byteOffset":{"start":0,"end":12},"start":{"line":0,"column":0},"end":{"line":0,"column":12}},"signature":"fn beta()","astKind":"function_item","isImport":false,"isExported":false,"members":[]}]}
"#;

fn workspace_with_source() -> TempDir {
    let temp = TempDir::new().expect("workspace tempdir");
    let src_dir = temp.path().join("src");
    fs::create_dir_all(&src_dir).expect("create src dir");
    fs::write(src_dir.join("lib.rs"), "pub fn alpha() {}\n").expect("write lib.rs");
    fs::write(src_dir.join("util.rs"), "fn beta() {}\n").expect("write util.rs");
    temp
}

fn write_arg_order_fake_sg() -> (TempDir, PathBuf) {
    let script_dir = TempDir::new().expect("script tempdir");
    let script_path = script_dir.path().join("sg");
    let script = r#"#!/bin/sh
if [ "$1" = "--version" ]; then
    printf 'ast-grep 0.44.0\n'
    exit 0
fi
for path in "$@"; do
    case "$path" in
        *.rs) printf '{"path":"%s","language":"Rust","items":[{"name":"alpha","isImport":false,"range":{"byteOffset":{"start":0,"end":17}}}]}\n' "$path" ;;
    esac
done
"#;
    fs::write(&script_path, script).expect("write fake sg");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&script_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&script_path, perms).expect("chmod");
    }
    (script_dir, script_path)
}

#[tokio::test]
#[serial]
async fn code_search_declaration_stream_reaps_at_candidate_cap() {
    let (_script_dir, script_path) = write_fake_sg(TWO_FILE_STREAM);
    let _override = set_read_only_ast_grep_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let outcome = search_declarations_bounded(
        temp.path(),
        &temp.path().join("src"),
        "alpha",
        &[AstGrepLanguage::Rust],
        1,
    )
    .await
    .expect("candidate-bounded declaration stream");

    assert_eq!(outcome.files.len(), 1);
    assert_eq!(outcome.files[0].declarations.len(), 1);
    assert!(outcome.files[0].complete);
    assert!(!outcome.stream_complete);
    assert!(outcome.truncated);
}

#[tokio::test]
#[serial]
async fn code_search_declaration_candidate_cap_selects_stable_path_prefix() {
    let (_script_dir, script_path) = write_arg_order_fake_sg();
    let _override = set_read_only_ast_grep_override_for_tests(Some(script_path));
    let temp = TempDir::new().expect("workspace tempdir");
    let src = temp.path().join("src");
    fs::create_dir_all(&src).expect("src directory");
    for name in ["z.rs", "a.rs", "m.rs"] {
        fs::write(src.join(name), "pub fn alpha() {}\n").expect("source fixture");
    }

    let first =
        search_declarations_bounded(temp.path(), &src, "alpha", &[AstGrepLanguage::Rust], 1)
            .await
            .expect("first bounded declaration search");
    let second =
        search_declarations_bounded(temp.path(), &src, "alpha", &[AstGrepLanguage::Rust], 1)
            .await
            .expect("second bounded declaration search");

    assert_eq!(first, second);
    assert!(first.truncated);
    assert_eq!(first.files.len(), 1);
    assert!(first.files[0].path.ends_with("a.rs"), "{first:?}");
}

#[tokio::test]
#[serial]
async fn code_search_declaration_stream_reaps_at_byte_cap() {
    let oversized_record = format!("{}\n", "x".repeat(CODE_SEARCH_OUTLINE_BYTE_CAP + 1));
    let (_script_dir, script_path) = write_fake_sg(&oversized_record);
    let _override = set_read_only_ast_grep_override_for_tests(Some(script_path));
    let temp = workspace_with_source();

    let outcome = search_declarations_bounded(
        temp.path(),
        &temp.path().join("src"),
        "alpha",
        &[AstGrepLanguage::Rust],
        20,
    )
    .await
    .expect("byte-bounded declaration stream");

    assert!(outcome.files.is_empty(), "{outcome:?}");
    assert!(!outcome.stream_complete);
    assert!(outcome.truncated);
}

#[tokio::test]
async fn code_search_outline_bounded_record_distinguishes_exact_eof_from_exhaustion() {
    use tokio::io::BufReader;

    let mut exact_reader = BufReader::new(&b"{}\n"[..]);
    let mut exact_record = Vec::with_capacity(3);
    let mut exact_bytes_read = 0;
    assert_eq!(
        read_bounded_record(
            &mut exact_reader,
            &mut exact_record,
            &mut exact_bytes_read,
            3,
        )
        .await
        .expect("exact record"),
        BoundedRecordRead::Record
    );
    exact_record.clear();
    assert_eq!(
        read_bounded_record(
            &mut exact_reader,
            &mut exact_record,
            &mut exact_bytes_read,
            3,
        )
        .await
        .expect("exact EOF probe"),
        BoundedRecordRead::Eof
    );

    let mut oversized_reader = BufReader::new(&b"xxxx"[..]);
    let mut oversized_record = Vec::with_capacity(3);
    let mut oversized_bytes_read = 0;
    assert_eq!(
        read_bounded_record(
            &mut oversized_reader,
            &mut oversized_record,
            &mut oversized_bytes_read,
            3,
        )
        .await
        .expect("oversized record"),
        BoundedRecordRead::Exhausted
    );
    assert_eq!(oversized_record.len(), 3);
}

#[test]
fn code_search_definition_smart_case_supports_unicode_lowercase_queries() {
    assert!(smart_case_eq("Éclair", "éclair"));
    assert!(!smart_case_eq("éclair", "Éclair"));
}
