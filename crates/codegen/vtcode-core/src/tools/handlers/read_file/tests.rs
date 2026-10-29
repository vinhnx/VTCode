#[expect(
    unused_imports,
    reason = "Intentional compatibility, platform, test, or API-shape suppression."
)]
use super::indentation::*;
use super::lines::condense_for_batch;
use super::slice::*;
use super::*;
use std::io::Write;
use tempfile::NamedTempFile;

#[tokio::test]
async fn reads_requested_range() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    writeln!(temp, "alpha")?;
    writeln!(temp, "beta")?;
    writeln!(temp, "gamma")?;

    let lines = read(temp.path(), 2, 2).await?.lines;
    assert_eq!(lines, vec!["beta".to_string(), "gamma".to_string()]);
    Ok(())
}

#[test]
fn read_file_args_accepts_boolean_indentation() {
    let args = json!({
        "file_path": "/tmp/example.txt",
        "mode": "slice",
        "indentation": false
    });

    let parsed: ReadFileArgs = serde_json::from_value(args).unwrap();
    assert!(matches!(parsed.mode, ReadMode::Slice));
    assert!(parsed.indentation.is_none());
}

#[test]
fn read_file_args_accepts_true_indentation() {
    let args = json!({
        "file_path": "/tmp/example.txt",
        "mode": "indentation",
        "indentation": true
    });

    let parsed: ReadFileArgs = serde_json::from_value(args).unwrap();
    assert!(matches!(parsed.mode, ReadMode::Indentation));
    assert!(parsed.indentation.is_some());
}

#[test]
fn read_file_args_accepts_empty_mode() {
    let args = json!({
        "file_path": "/tmp/example.txt",
        "mode": ""
    });

    let parsed: ReadFileArgs = serde_json::from_value(args).unwrap();
    assert!(matches!(parsed.mode, ReadMode::Slice));
}

#[tokio::test]
async fn read_file_handler_skips_condense_when_disabled() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    for idx in 0..60 {
        writeln!(temp, "line-{idx}")?;
    }

    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 1,
        limit: 2000,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: false,
        offset_bytes: None,
        page_size_bytes: None,
    };
    let handler = ReadFileHandler;
    let content = handler.handle(args).await?;

    assert!(!content.contains("lines omitted"));
    assert_eq!(content.lines().count(), 60);
    Ok(())
}

#[tokio::test]
async fn read_file_handler_skips_condense_for_paginated_read() -> Result<()> {
    // Regression: when the caller follows an omit-hint and reads a
    // specific slice (offset > 1), the handler must NOT re-condense the
    // result. Re-condensing would emit another omit-hint and trap the
    // agent in an infinite pagination regress (checkpoint turn_613).
    let mut temp = NamedTempFile::new()?;
    for idx in 1..=300 {
        writeln!(temp, "line-{idx}")?;
    }

    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 81,
        limit: 229,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: true,
        offset_bytes: None,
        page_size_bytes: None,
    };
    let handler = ReadFileHandler;
    let content = handler.handle(args).await?;

    // No re-condensation: the requested slice is returned verbatim.
    assert!(!content.contains("lines omitted"), "paginated read must not be re-condensed; got: {content}");
    // The slice starts at line 81 (1-indexed), so line-81 must be present.
    assert!(
        content.contains("line-81"),
        "paginated read must include the requested offset; got first line: {:?}",
        content.lines().next()
    );
    Ok(())
}

#[tokio::test]
async fn read_file_handler_still_condenses_default_overview_read() -> Result<()> {
    // The default overview path (offset == 1) must still condense large
    // outputs so the catalog stays bounded. This guards against the fix
    // over-correcting and never condensing.
    let mut temp = NamedTempFile::new()?;
    for idx in 1..=300 {
        writeln!(temp, "line-{idx}")?;
    }

    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 1,
        limit: 2000,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: true,
        offset_bytes: None,
        page_size_bytes: None,
    };
    let handler = ReadFileHandler;
    let content = handler.handle(args).await?;

    assert!(
        content.contains("lines omitted"),
        "default overview read must condense large output; got {} lines",
        content.lines().count()
    );
    Ok(())
}

#[tokio::test]
async fn empty_file_read_succeeds_at_start_and_rejects_higher_offsets() -> Result<()> {
    let temp = NamedTempFile::new()?;
    let result = read(temp.path(), 1, 2).await?;
    assert!(result.lines.is_empty());
    assert!(!result.has_more);
    assert!(read(temp.path(), 2, 2).await.is_err());
    let ranges = vec![
        ReadRange {
            offset: 1,
            limit: 2,
            mode: ReadMode::Slice,
            indentation: None,
        },
        ReadRange {
            offset: 2,
            limit: 2,
            mode: ReadMode::Slice,
            indentation: None,
        },
    ];
    let result = read_ranges(temp.path(), &ranges).await?;
    assert!(result.results[0].as_ref().unwrap().as_ref().unwrap().lines.is_empty());
    assert!(result.results[1].as_ref().unwrap().is_err());
    Ok(())
}

#[tokio::test]
async fn errors_when_offset_exceeds_length() {
    let mut temp = NamedTempFile::new().unwrap();
    writeln!(temp, "only").unwrap();

    let err = read(temp.path(), 3, 1).await;
    err.unwrap_err();
}

#[tokio::test]
async fn reads_non_utf8_lines() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    temp.as_file_mut().write_all(b"\xff\xfe\nplain\n")?;

    let lines = read(temp.path(), 1, 2).await?.lines;
    let expected_first = format!("{}{}", '\u{FFFD}', '\u{FFFD}');
    assert_eq!(lines, vec![expected_first, "plain".to_string()]);
    Ok(())
}

#[tokio::test]
async fn reports_oversized_physical_lines_as_truncated() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    let long_line = "x".repeat(crate::tools::file_ops::MAX_LINE_READ_BYTES + 1);
    writeln!(temp, "{long_line}")?;
    writeln!(temp, "plain")?;

    let outcome = ReadFileHandler
        .handle_detailed(ReadFileArgs {
            file_path: temp.path().to_string_lossy().into_owned(),
            offset: 1,
            limit: 2,
            mode: ReadMode::Slice,
            indentation: None,
            max_tokens: None,
            condense: false,
            offset_bytes: None,
            page_size_bytes: None,
        })
        .await?;

    assert!(outcome.line_truncated);
    assert!(outcome.content.contains("plain"));
    Ok(())
}

#[tokio::test]
async fn trims_crlf_endings() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    write!(temp, "one\r\ntwo\r\n")?;

    let lines = read(temp.path(), 1, 2).await?.lines;
    assert_eq!(lines, vec!["one".to_string(), "two".to_string()]);
    Ok(())
}

#[tokio::test]
async fn respects_limit_even_with_more_lines() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    writeln!(temp, "first")?;
    writeln!(temp, "second")?;
    writeln!(temp, "third")?;

    let result = read(temp.path(), 1, 2).await?;
    assert_eq!(result.lines, vec!["first".to_string(), "second".to_string()]);
    assert!(result.has_more);
    Ok(())
}

#[tokio::test]
async fn reads_exact_limit_without_continuation_at_eof() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    writeln!(temp, "first")?;
    writeln!(temp, "second")?;

    let result = read(temp.path(), 1, 2).await?;
    assert_eq!(result.lines, vec!["first".to_string(), "second".to_string()]);
    assert!(!result.has_more);
    Ok(())
}

#[test]
fn clamp_to_absolute_cap_marks_only_true_overages() {
    // A request above the ceiling is clamped and flagged.
    assert_eq!(ReadFileHandler::clamp_to_absolute_cap(1000, 400), (400, true));
    // A request exactly at the ceiling sits at the cap but is not "clamped"
    // (this is what keeps the pagination chain alive on follow-up pages).
    assert_eq!(ReadFileHandler::clamp_to_absolute_cap(400, 400), (400, false));
    // A request below the ceiling is unchanged and not flagged.
    assert_eq!(ReadFileHandler::clamp_to_absolute_cap(50, 400), (50, false));
}

#[tokio::test]
async fn absolute_cap_clamps_large_request() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    for idx in 1..=1000 {
        writeln!(temp, "line-{idx}")?;
    }

    let handler = ReadFileHandler;
    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 1,
        limit: 2000,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: false,
        offset_bytes: None,
        page_size_bytes: None,
    };
    let outcome = handler.handle_detailed(args).await?;

    let cap = crate::tools::read_limits::read_limit_lines();
    assert!(outcome.capped_by_limit, "request larger than the cap must be clamped");
    assert_eq!(outcome.applied_limit, cap);
    assert!(outcome.has_more);
    assert!(outcome.lines_read <= cap);
    Ok(())
}

#[tokio::test]
async fn explicit_limit_under_cap_is_preserved() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    for idx in 1..=100 {
        writeln!(temp, "line-{idx}")?;
    }

    let handler = ReadFileHandler;
    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 1,
        limit: 50,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: false,
        offset_bytes: None,
        page_size_bytes: None,
    };
    let outcome = handler.handle_detailed(args).await?;

    assert!(!outcome.capped_by_limit);
    // Small limits are expanded to MIN_BATCH_LIMIT (200) by the batch path,
    // so the whole 100-line file is returned, but it is not cap-clamped.
    assert_eq!(outcome.lines_read, 100);
    Ok(())
}

#[tokio::test]
async fn truncates_lines_longer_than_max_length() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    let long_line = "x".repeat(MAX_LINE_LENGTH + 50);
    writeln!(temp, "{long_line}")?;

    let lines = read(temp.path(), 1, 1).await?.lines;
    let expected = "x".repeat(MAX_LINE_LENGTH);
    assert_eq!(lines, vec![expected]);
    Ok(())
}

#[tokio::test]
async fn batch_reads_multiple_files() -> Result<()> {
    let mut temp1 = NamedTempFile::new()?;
    writeln!(temp1, "file1_line1")?;
    writeln!(temp1, "file1_line2")?;

    let mut temp2 = NamedTempFile::new()?;
    writeln!(temp2, "file2_line1")?;
    writeln!(temp2, "file2_line2")?;

    let handler = ReadFileHandler;
    let args = BatchReadArgs {
        reads: vec![
            BatchReadRequest {
                file_path: temp1.path().to_string_lossy().to_string(),
                range: None,
                ranges: None,
            },
            BatchReadRequest {
                file_path: temp2.path().to_string_lossy().to_string(),
                range: None,
                ranges: None,
            },
        ],
        max_concurrency: 2,
        ui_progress: false,
    };

    let result = handler.handle_batch(args).await?;
    assert_eq!(result["success"], true);
    assert_eq!(result["files_read"], 2);
    assert_eq!(result["files_succeeded"], 2);

    let content = result["content"].as_str().unwrap();
    assert!(content.contains("file1_line1"));
    assert!(content.contains("file2_line1"));
    Ok(())
}

#[tokio::test]
async fn batch_rejects_zero_max_concurrency() -> Result<()> {
    let handler = ReadFileHandler;
    let args = BatchReadArgs {
        reads: vec![BatchReadRequest {
            file_path: "/tmp/example.txt".to_string(),
            range: None,
            ranges: None,
        }],
        max_concurrency: 0,
        ui_progress: false,
    };

    let error = handler.handle_batch(args).await.expect_err("zero must be rejected");
    assert!(error.to_string().contains("max_concurrency must be greater than zero"));
    Ok(())
}

#[tokio::test]
async fn batch_caps_excessive_max_concurrency() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    writeln!(temp, "line")?;

    let handler = ReadFileHandler;
    let args = BatchReadArgs {
        reads: vec![BatchReadRequest {
            file_path: temp.path().to_string_lossy().to_string(),
            range: None,
            ranges: None,
        }],
        max_concurrency: usize::MAX,
        ui_progress: false,
    };

    let result = handler.handle_batch(args).await?;
    assert_eq!(result["success"], true);
    assert_eq!(result["files_succeeded"], 1);
    Ok(())
}

#[tokio::test]
async fn batch_preserves_input_order_with_duplicate_requests() -> Result<()> {
    let mut first = NamedTempFile::new()?;
    writeln!(first, "first")?;
    let mut second = NamedTempFile::new()?;
    writeln!(second, "second")?;

    let first_path = first.path().to_string_lossy().to_string();
    let second_path = second.path().to_string_lossy().to_string();
    let handler = ReadFileHandler;
    let args = BatchReadArgs {
        reads: vec![
            BatchReadRequest {
                file_path: second_path.clone(),
                range: None,
                ranges: None,
            },
            BatchReadRequest {
                file_path: first_path.clone(),
                range: None,
                ranges: None,
            },
            BatchReadRequest { file_path: second_path, range: None, ranges: None },
        ],
        max_concurrency: usize::MAX,
        ui_progress: false,
    };

    let result = handler.handle_batch(args).await?;
    let items = result["items"].as_array().context("batch items must be an array")?;
    assert_eq!(items.len(), 3);
    assert_eq!(items[0]["ranges"][0]["content"], "second");
    assert_eq!(items[1]["ranges"][0]["content"], "first");
    assert_eq!(items[2]["ranges"][0]["content"], "second");
    let content = result["content"].as_str().context("batch content must be a string")?;
    assert!(content.find("second") < content.find("first"));
    Ok(())
}

#[tokio::test]
async fn batch_reads_multiple_ranges_from_same_file() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    for i in 1..=20 {
        writeln!(temp, "line{i}")?;
    }

    let handler = ReadFileHandler;
    let args = BatchReadArgs {
        reads: vec![BatchReadRequest {
            file_path: temp.path().to_string_lossy().to_string(),
            range: None,
            ranges: Some(vec![
                ReadRange {
                    offset: 1,
                    limit: 3,
                    mode: ReadMode::Slice,
                    indentation: None,
                },
                ReadRange {
                    offset: 10,
                    limit: 3,
                    mode: ReadMode::Slice,
                    indentation: None,
                },
            ]),
        }],
        max_concurrency: 4,
        ui_progress: false,
    };

    let result = handler.handle_batch(args).await?;
    assert_eq!(result["success"], true);

    let items = result["items"].as_array().unwrap();
    assert_eq!(items.len(), 1);

    let ranges = items[0]["ranges"].as_array().unwrap();
    assert_eq!(ranges.len(), 2);
    assert_eq!(ranges[0]["offset"], 1);
    assert_eq!(ranges[1]["offset"], 10);
    assert_eq!(ranges[0]["content"], "line1\nline2\nline3");
    assert_eq!(ranges[1]["content"], "line10\nline11\nline12");
    Ok(())
}

#[tokio::test]
async fn batch_range_is_capped_by_absolute_line_limit() -> Result<()> {
    let cap = crate::tools::read_limits::absolute_line_cap();
    let mut temp = NamedTempFile::new()?;
    for i in 1..=cap + 1 {
        writeln!(temp, "line{i}")?;
    }

    let handler = ReadFileHandler;
    let result = handler
        .handle_batch(BatchReadArgs {
            reads: vec![BatchReadRequest {
                file_path: temp.path().to_string_lossy().to_string(),
                range: Some(ReadRange {
                    offset: 1,
                    limit: cap + 1,
                    mode: ReadMode::Slice,
                    indentation: None,
                }),
                ranges: None,
            }],
            max_concurrency: 1,
            ui_progress: false,
        })
        .await?;

    let range = &result["items"][0]["ranges"][0];
    assert_eq!(range["lines_read"], cap);

    let content = range["content"].as_str().context("batch range content must be a string")?;
    assert!(content.ends_with(&format!("line{cap}")));
    assert!(!content.contains(&format!("line{}", cap + 1)));
    Ok(())
}

#[tokio::test]
async fn batch_preserves_completed_ranges_before_a_later_error() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    writeln!(temp, "line1")?;
    writeln!(temp, "line2")?;

    let handler = ReadFileHandler;
    let args = BatchReadArgs {
        reads: vec![BatchReadRequest {
            file_path: temp.path().to_string_lossy().to_string(),
            range: None,
            ranges: Some(vec![
                ReadRange {
                    offset: 1,
                    limit: 1,
                    mode: ReadMode::Slice,
                    indentation: None,
                },
                ReadRange {
                    offset: 10,
                    limit: 1,
                    mode: ReadMode::Slice,
                    indentation: None,
                },
            ]),
        }],
        max_concurrency: 1,
        ui_progress: false,
    };

    let result = handler.handle_batch(args).await?;
    assert_eq!(result["success"], false);
    assert_eq!(result["items"][0]["ranges"].as_array().map(Vec::len), Some(1));
    assert_eq!(result["items"][0]["ranges"][0]["content"], "line1");
    assert!(result["items"][0]["error"].as_str().is_some());
    Ok(())
}

#[tokio::test]
async fn batch_handles_missing_file_gracefully() -> Result<()> {
    let handler = ReadFileHandler;
    let args = BatchReadArgs {
        reads: vec![BatchReadRequest {
            file_path: "/nonexistent/path/file.txt".to_string(),
            range: None,
            ranges: None,
        }],
        max_concurrency: 1,
        ui_progress: false,
    };

    let result = handler.handle_batch(args).await?;
    assert_eq!(result["success"], false);

    let items = result["items"].as_array().unwrap();
    assert!(items[0]["error"].as_str().is_some());
    Ok(())
}

#[test]
fn condense_for_batch_preserves_small_outputs() {
    let mut lines: Vec<String> = (1..=20).map(|i| format!("line{i}")).collect();
    let (condensed, omitted) = condense_for_batch(&mut lines);
    assert!(!condensed);
    assert_eq!(omitted, 0);
    assert_eq!(lines.len(), 20);
}

#[test]
fn condense_for_batch_condenses_large_outputs() {
    let mut lines: Vec<String> = (1..=200).map(|i| format!("line{i}")).collect();
    let (condensed, omitted) = condense_for_batch(&mut lines);
    assert!(condensed);
    assert!(omitted > 0);
    assert!(lines.len() < 200);
    assert!(lines.iter().any(|l| l.contains("omitted")));
}

#[test]
fn condense_for_batch_does_not_treat_plus_minus_text_as_diff() {
    let mut lines: Vec<String> = (1..=150)
        .map(|i| {
            if i % 2 == 0 {
                format!("+ normal status line {i}")
            } else {
                format!("- normal status line {i}")
            }
        })
        .collect();
    let (condensed, omitted) = condense_for_batch(&mut lines);
    assert!(condensed);
    assert!(omitted > 0);
}

#[test]
fn condense_for_batch_preserves_actual_diff_output() {
    let mut lines = vec![
        "diff --git a/src/main.rs b/src/main.rs".to_string(),
        "index 1111111..2222222 100644".to_string(),
        "--- a/src/main.rs".to_string(),
        "+++ b/src/main.rs".to_string(),
        "@@ -1 +1 @@".to_string(),
        "-old".to_string(),
        "+new".to_string(),
    ];
    let (condensed, omitted) = condense_for_batch(&mut lines);
    assert!(!condensed);
    assert_eq!(omitted, 0);
}

// --- Byte-range read tests ---

#[tokio::test]
async fn byte_range_reads_first_bytes() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    temp.as_file_mut().write_all(b"Hello, World!")?;

    let handler = ReadFileHandler;
    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 1,
        limit: 2000,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: false,
        offset_bytes: Some(0),
        page_size_bytes: Some(5),
    };
    let outcome = handler.handle_detailed(args).await?;
    // Content is line-numbered: "1: Hello"
    assert_eq!(outcome.content, "1: Hello");
    assert!(outcome.has_more);
    Ok(())
}

#[tokio::test]
async fn byte_range_reads_with_offset() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    writeln!(temp, "Hello, World!")?;
    writeln!(temp, "Second line")?;

    let handler = ReadFileHandler;
    // Seek past "Hello, World!\n" (14 bytes) to start of second line
    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 1,
        limit: 2000,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: false,
        offset_bytes: Some(14),
        page_size_bytes: Some(50),
    };
    let outcome = handler.handle_detailed(args).await?;
    // Should start at line 2
    assert!(outcome.content.contains("2: Second line"));
    assert!(!outcome.has_more);
    Ok(())
}

#[tokio::test]
async fn byte_range_beyond_eof_returns_empty() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    temp.as_file_mut().write_all(b"short")?;

    let handler = ReadFileHandler;
    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 1,
        limit: 2000,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: false,
        offset_bytes: Some(100),
        page_size_bytes: Some(10),
    };
    let outcome = handler.handle_detailed(args).await?;
    assert_eq!(outcome.content, "");
    assert!(!outcome.has_more);
    Ok(())
}

#[tokio::test]
async fn byte_range_clamps_to_file_size() -> Result<()> {
    // Multi-line file so we can test partial-line skipping
    let mut temp = NamedTempFile::new()?;
    writeln!(temp, "first line")?;
    writeln!(temp, "second line")?;
    writeln!(temp, "third line")?;

    let handler = ReadFileHandler;
    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 1,
        limit: 2000,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: false,
        offset_bytes: Some(0),
        page_size_bytes: Some(100), // Request more than available
    };
    let outcome = handler.handle_detailed(args).await?;
    // Should have line numbers for all three lines
    assert!(outcome.content.contains("1: first line"));
    assert!(outcome.content.contains("2: second line"));
    assert!(outcome.content.contains("3: third line"));
    assert!(!outcome.has_more);
    Ok(())
}

#[tokio::test]
async fn byte_range_reads_non_utf8_lossy() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    temp.as_file_mut().write_all(b"\xff\xfeplain")?;

    let handler = ReadFileHandler;
    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 1,
        limit: 2000,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: false,
        offset_bytes: Some(0),
        page_size_bytes: Some(7), // Read all 7 bytes
    };
    let outcome = handler.handle_detailed(args).await?;
    // Lossy conversion replaces invalid UTF-8 with U+FFFD
    assert!(outcome.content.contains('\u{FFFD}'));
    assert!(outcome.content.contains("plain"));
    Ok(())
}

#[tokio::test]
async fn byte_range_has_more_when_not_at_eof() -> Result<()> {
    let mut temp = NamedTempFile::new()?;
    writeln!(temp, "line one")?;
    writeln!(temp, "line two")?;
    writeln!(temp, "line three")?;

    let handler = ReadFileHandler;
    // Read only first 10 bytes - should get first line only
    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 1,
        limit: 2000,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: false,
        offset_bytes: Some(0),
        page_size_bytes: Some(10),
    };
    let outcome = handler.handle_detailed(args).await?;
    // Should have line number
    assert!(outcome.content.contains("1: line one"));
    assert!(outcome.has_more);
    Ok(())
}

#[tokio::test]
async fn byte_range_defaults_to_8192_when_length_omitted() -> Result<()> {
    // Create a file large enough that 8192 bytes doesn't cover everything
    let mut temp = NamedTempFile::new()?;
    for i in 1..=500 {
        writeln!(temp, "line-{i}: {}", "x".repeat(20))?;
    }

    let handler = ReadFileHandler;
    let args = ReadFileArgs {
        file_path: temp.path().to_string_lossy().to_string(),
        offset: 1,
        limit: 2000,
        mode: ReadMode::Slice,
        indentation: None,
        max_tokens: None,
        condense: false,
        offset_bytes: Some(0),
        page_size_bytes: None, // Should default to 8192
    };
    let outcome = handler.handle_detailed(args).await?;
    // Content should have line numbers
    assert!(outcome.content.contains("1: line-1"));
    assert!(outcome.has_more);
    Ok(())
}

#[test]
fn byte_range_args_from_json() {
    let args = json!({
        "file_path": "/tmp/test.bin",
        "offset_bytes": 1024,
        "page_size_bytes": 4096
    });
    let parsed: ReadFileArgs = serde_json::from_value(args).unwrap();
    assert_eq!(parsed.offset_bytes, Some(1024));
    assert_eq!(parsed.page_size_bytes, Some(4096));
}

#[test]
fn byte_range_args_length_alias() {
    let args = json!({
        "file_path": "/tmp/test.bin",
        "offset_bytes": 0,
        "length": 2048
    });
    let parsed: ReadFileArgs = serde_json::from_value(args).unwrap();
    assert_eq!(parsed.offset_bytes, Some(0));
    assert_eq!(parsed.page_size_bytes, Some(2048));
}
