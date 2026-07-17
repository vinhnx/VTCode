use super::{
    BoundedRecordRead, CODE_SEARCH_OUTLINE_BYTE_CAP, CODE_SEARCH_OUTLINE_FIXED_ARGS,
    CODE_SEARCH_OUTLINE_PATH_BATCH_SIZE, arg_bytes, next_outline_path_batch_with_cap,
    read_bounded_record, search_declarations_bounded, smart_case_eq,
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

fn write_failing_second_batch_fake_sg() -> (TempDir, PathBuf) {
    let script_dir = TempDir::new().expect("script tempdir");
    let script_path = script_dir.path().join("sg");
    let failure_name = format!("{CODE_SEARCH_OUTLINE_PATH_BATCH_SIZE:03}.rs");
    let script = format!(
        r#"#!/bin/sh
if [ "$1" = "--version" ]; then
    printf 'ast-grep 0.44.0\n'
    exit 0
fi
for path in "$@"; do
    case "$path" in
        *{failure_name}) exit 1 ;;
        *.rs) printf '{{"path":"%s","language":"Rust","items":[]}}\n' "$path" ;;
    esac
done
"#
    );
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

fn workspace_with_numbered_sources(count: usize) -> (TempDir, PathBuf) {
    let temp = TempDir::new().expect("workspace tempdir");
    let src = temp.path().join("src");
    fs::create_dir_all(&src).expect("src directory");
    for index in 0..count {
        fs::write(src.join(format!("{index:03}.rs")), "pub fn alpha() {}\n")
            .expect("source fixture");
    }
    (temp, src)
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
async fn code_search_declaration_batches_preserve_stable_global_prefix() {
    let (_script_dir, script_path) = write_arg_order_fake_sg();
    let _override = set_read_only_ast_grep_override_for_tests(Some(script_path));
    let file_count = CODE_SEARCH_OUTLINE_PATH_BATCH_SIZE + 2;
    let candidate_cap = CODE_SEARCH_OUTLINE_PATH_BATCH_SIZE + 1;
    let (temp, src) = workspace_with_numbered_sources(file_count);

    let outcome = search_declarations_bounded(
        temp.path(),
        &src,
        "alpha",
        &[AstGrepLanguage::Rust],
        candidate_cap,
    )
    .await
    .expect("multi-batch declaration search");

    assert_eq!(outcome.files.len(), candidate_cap);
    assert!(outcome.truncated);
    assert!(!outcome.stream_complete);
    for (index, file) in outcome.files.iter().enumerate() {
        assert!(
            file.path.ends_with(format!("{index:03}.rs")),
            "unexpected global prefix at {index}: {file:?}"
        );
    }
}

#[tokio::test]
#[serial]
async fn code_search_declaration_batches_keep_complete_file_inventory() {
    let (_script_dir, script_path) = write_arg_order_fake_sg();
    let _override = set_read_only_ast_grep_override_for_tests(Some(script_path));
    let file_count = CODE_SEARCH_OUTLINE_PATH_BATCH_SIZE + 2;
    let (temp, src) = workspace_with_numbered_sources(file_count);

    let outcome =
        search_declarations_bounded(temp.path(), &src, "missing", &[AstGrepLanguage::Rust], 1)
            .await
            .expect("complete multi-batch declaration search");

    assert_eq!(outcome.files.len(), file_count);
    assert!(outcome.files.iter().all(|file| file.complete));
    assert!(outcome.stream_complete);
    assert!(!outcome.truncated);
}

#[tokio::test]
#[serial]
async fn code_search_declaration_batch_failure_fails_the_component() {
    let (_script_dir, script_path) = write_failing_second_batch_fake_sg();
    let _override = set_read_only_ast_grep_override_for_tests(Some(script_path));
    let (temp, src) = workspace_with_numbered_sources(CODE_SEARCH_OUTLINE_PATH_BATCH_SIZE + 1);

    let error =
        search_declarations_bounded(temp.path(), &src, "missing", &[AstGrepLanguage::Rust], 1)
            .await
            .expect_err("failed batch must fail declaration discovery");

    assert!(error.to_string().contains("definition search failed"));
}

fn fixed_outline_arg_bytes(executable: &std::path::Path) -> usize {
    super::arg_os_bytes(executable.as_os_str())
        + CODE_SEARCH_OUTLINE_FIXED_ARGS.iter().map(|arg| arg_bytes(arg)).sum::<usize>()
}

#[test]
fn code_search_outline_path_batches_roll_over_at_byte_cap() {
    let executable = std::path::Path::new("ast-grep");
    let first = "a.rs".to_owned();
    let second = "longer.rs".to_owned();
    let cap = fixed_outline_arg_bytes(executable) + arg_bytes(&first) + arg_bytes(&second) - 1;
    let mut paths = vec![first.clone(), second.clone()].into_iter().peekable();

    assert_eq!(
        next_outline_path_batch_with_cap(executable, &mut paths, cap).expect("first batch"),
        vec![first]
    );
    assert_eq!(
        next_outline_path_batch_with_cap(executable, &mut paths, cap).expect("second batch"),
        vec![second]
    );
}

#[test]
fn code_search_outline_path_batch_counts_executable_and_nul() {
    let executable = std::path::Path::new("/long/path/to/ast-grep");
    let path = "a.rs".to_owned();
    let cap_without_executable =
        CODE_SEARCH_OUTLINE_FIXED_ARGS.iter().map(|arg| arg_bytes(arg)).sum::<usize>()
            + arg_bytes(&path);
    let mut paths = vec![path.clone()].into_iter().peekable();

    let error = next_outline_path_batch_with_cap(executable, &mut paths, cap_without_executable)
        .expect_err("executable bytes must count towards the argv cap");

    assert!(error.to_string().contains("command argument byte limit"));
    assert_eq!(paths.next(), Some(path));
}

#[test]
fn code_search_outline_path_batch_preserves_input_order() {
    let executable = std::path::Path::new("ast-grep");
    let expected = vec!["c.rs".to_owned(), "a.rs".to_owned(), "b.rs".to_owned()];
    let cap = fixed_outline_arg_bytes(executable)
        + expected.iter().map(|path| arg_bytes(path)).sum::<usize>();
    let mut paths = expected.clone().into_iter().peekable();

    let actual =
        next_outline_path_batch_with_cap(executable, &mut paths, cap).expect("ordered path batch");

    assert_eq!(actual, expected);
}

#[test]
fn code_search_outline_path_batches_do_not_drop_paths() {
    let executable = std::path::Path::new("ast-grep");
    let expected = (0..(CODE_SEARCH_OUTLINE_PATH_BATCH_SIZE + 3))
        .map(|index| format!("{index:03}.rs"))
        .collect::<Vec<_>>();
    let cap = fixed_outline_arg_bytes(executable) + arg_bytes(&expected[0]) * 3;
    let mut paths = expected.clone().into_iter().peekable();
    let mut actual = Vec::new();

    while paths.peek().is_some() {
        actual.extend(
            next_outline_path_batch_with_cap(executable, &mut paths, cap)
                .expect("bounded path batch"),
        );
    }

    assert_eq!(actual, expected);
}

#[test]
fn code_search_outline_oversized_single_path_fails_without_consuming_it() {
    let executable = std::path::Path::new("ast-grep");
    let oversized = "oversized.rs".to_owned();
    let cap = fixed_outline_arg_bytes(executable) + arg_bytes(&oversized) - 1;
    let mut paths = vec![oversized.clone()].into_iter().peekable();

    let error = next_outline_path_batch_with_cap(executable, &mut paths, cap)
        .expect_err("oversized path must fail the definition component");

    assert!(error.to_string().contains("command argument byte limit"));
    assert_eq!(paths.next(), Some(oversized));
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
        read_bounded_record(&mut exact_reader, &mut exact_record, &mut exact_bytes_read, 3,)
            .await
            .expect("exact record"),
        BoundedRecordRead::Record
    );
    exact_record.clear();
    assert_eq!(
        read_bounded_record(&mut exact_reader, &mut exact_record, &mut exact_bytes_read, 3,)
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
