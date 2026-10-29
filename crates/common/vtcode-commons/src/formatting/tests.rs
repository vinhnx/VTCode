use super::*;

#[test]
fn humanize_duration_compact_ladder() {
    assert_eq!(humanize_duration_compact(0), "0s");
    assert_eq!(humanize_duration_compact(42), "42s");
    assert_eq!(humanize_duration_compact(59), "59s");
    assert_eq!(humanize_duration_compact(60), "1m");
    assert_eq!(humanize_duration_compact(300), "5m");
    assert_eq!(humanize_duration_compact(3599), "59m");
    assert_eq!(humanize_duration_compact(3600), "1h");
    assert_eq!(humanize_duration_compact(3 * 3600), "3h");
    assert_eq!(humanize_duration_compact(86399), "23h");
    assert_eq!(humanize_duration_compact(86400), "1d");
    assert_eq!(humanize_duration_compact(2 * 86400), "2d");
}

#[test]
fn format_float_display_collapses_f32_widening() {
    // `0.7f32` widened to `f64` is the exact artifact seen in the settings
    // value column; asymmetric counterpart must keep full precision.
    assert_eq!(format_float_display(f64::from(0.7f32)), "0.7");
    assert_eq!(format_float_display(f64::from(0.3f32)), "0.3");
    assert_eq!(format_float_display(f64::from(-0.7f32)), "-0.7");
    assert_eq!(format_float_display(0.3333333333333333), "0.3333333333333333");
}

#[test]
fn format_float_display_snaps_stepper_artifacts_to_grid() {
    // One `+0.1` press on the widened `0.7` default lands at
    // `0.799999988079071`; the classic `0.1 + 0.2` sum snaps the same way.
    // Asymmetric counterpart keeps genuine off-grid precision.
    assert_eq!(format_float_display(f64::from(0.7f32) + 0.1), "0.8");
    assert_eq!(format_float_display(0.799999988079071), "0.8");
    assert_eq!(format_float_display(0.1 + 0.2), "0.3");
    assert_eq!(format_float_display(-0.799999988079071), "-0.8");
    assert_eq!(format_float_display(0.123456789), "0.123456789");
}

#[test]
fn format_float_display_keeps_plain_floats() {
    assert_eq!(format_float_display(1.0), "1");
    assert_eq!(format_float_display(0.0), "0");
    assert_eq!(format_float_display(0.05), "0.05");
    assert_eq!(format_float_display(0.75), "0.75");
}

#[test]
fn truncate_byte_budget_ascii() {
    assert_eq!(truncate_byte_budget("hello world", 5, "..."), "hello...");
    assert_eq!(truncate_byte_budget("hi", 10, "..."), "hi");
}

#[test]
fn contains_ignore_ascii_case_matches_str_contains_semantics() {
    // Mixed case on both sides.
    assert!(contains_ignore_ascii_case("MiniMax-M2.5", "minimax-m2.5"));
    assert!(contains_ignore_ascii_case("minimax-m2.5", "MINIMAX"));
    // Uppercase needle only (no lowercase form provided).
    assert!(contains_ignore_ascii_case("openai/gpt-5", "GPT-5"));
    // Empty needle matches, as `str::contains("")` does.
    assert!(contains_ignore_ascii_case("anything", ""));
    // Needle longer than the haystack.
    assert!(!contains_ignore_ascii_case("glm", "glm-4.5"));
    // No match.
    assert!(!contains_ignore_ascii_case("openai/gpt-5", "minimax"));
}

#[test]
fn contains_ignore_ascii_case_matches_lowercased_contains_for_unicode() {
    // ASCII needle inside a non-ASCII haystack: `to_ascii_lowercase` leaves
    // multi-byte bytes untouched, so results must agree exactly.
    for (haystack, needle) in [
        ("café GLM-5", "glm-5"),
        ("日本語 <THINK", "<think"),
        ("straße", "STRASSE"),
        ("naïve", "naïve"),
    ] {
        assert_eq!(
            contains_ignore_ascii_case(haystack, needle),
            haystack.to_ascii_lowercase().contains(&needle.to_ascii_lowercase()),
            "mismatch for haystack={haystack:?} needle={needle:?}"
        );
    }
}

#[test]
fn truncate_byte_budget_cjk_no_panic() {
    // 'こ' = 3 bytes, 'ん' = 3 bytes → "こんにちは" = 15 bytes
    let jp = "こんにちは";
    // Cutting at 5 bytes lands inside 'ん' (bytes 3..6); must round down to 3.
    assert_eq!(truncate_byte_budget(jp, 5, "…"), "こ…");
    // Cutting at 6 lands on boundary
    assert_eq!(truncate_byte_budget(jp, 6, "…"), "こん…");
}

#[test]
fn truncate_byte_budget_mixed_ascii_cjk() {
    let mixed = "AB日本語CD";
    // A=1, B=1, 日=3, 本=3, 語=3, C=1, D=1 → 13 bytes total
    assert_eq!(truncate_byte_budget(mixed, 4, ".."), "AB.."); // mid-日 rounds to 2
    assert_eq!(truncate_byte_budget(mixed, 5, ".."), "AB日.."); // 2+3=5 exact
}

#[test]
fn truncate_byte_budget_emoji() {
    let emoji = "👋🌍"; // 4 bytes each = 8 bytes
    assert_eq!(truncate_byte_budget(emoji, 5, "!"), "👋!");
}

#[test]
fn truncate_byte_budget_zero() {
    assert_eq!(truncate_byte_budget("abc", 0, "..."), "...");
}

#[test]
fn compact_reasoning_text_collapses_blank_runs() {
    assert_eq!(compact_reasoning_text("line1\n\n\n\nline2\n"), "line1\n\nline2");
    assert_eq!(compact_reasoning_text("a\n\n\n\n\n\nb"), "a\n\nb");
}

#[test]
fn compact_reasoning_text_preserves_single_paragraph_breaks() {
    assert_eq!(compact_reasoning_text("para one\n\npara two\n"), "para one\n\npara two");
}

#[test]
fn compact_reasoning_text_trims_trailing_whitespace() {
    assert_eq!(compact_reasoning_text("  a  \n\n\n  b  \n"), "a\n\nb");
}

#[test]
fn compact_reasoning_text_strips_leading_trailing_blanks() {
    assert_eq!(compact_reasoning_text("\n\n\nmid\n\n\n"), "mid");
    assert_eq!(compact_reasoning_text("\n\n\n"), "");
    assert_eq!(compact_reasoning_text(""), "");
}

#[test]
fn wrap_text_words_basic_and_continuation_width() {
    assert_eq!(wrap_text_words("the quick brown fox", 9, 9), vec!["the quick", "brown fox"]);
    // First line wider than continuation lines.
    assert_eq!(wrap_text_words("alpha beta gamma delta", 11, 5), vec!["alpha beta", "gamma", "delta"]);
}

#[test]
fn wrap_text_words_blank_and_unicode() {
    assert!(wrap_text_words("   ", 5, 5).is_empty());
    // Must not panic on multi-byte chars and counts chars, not bytes.
    let wrapped = wrap_text_words("あいう えお かきく", 3, 3);
    assert_eq!(wrapped, vec!["あいう", "えお", "かきく"]);
}

#[test]
fn wrap_shell_command_keeps_screenshot_pipeline_in_full() {
    // Screenshot 2026-09-24 16:37: the quoted grep pattern holds spaces
    // and pipes but must never split mid-quote; every pipe segment must
    // survive with no `…`, and rejoining restores the source exactly.
    let command = "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs | grep -v node_modules | grep -v package-lock | grep -v \"\\.backup\"";
    let wrapped = wrap_shell_command(command, 62, 58);
    assert_eq!(
        wrapped,
        vec![
            "grep -rn \"@vinhnx/vtcode|npm install -g||npx @vinhnx\" docs |",
            "grep -v node_modules | grep -v package-lock | grep -v",
            "\"\\.backup\"",
        ]
    );
    assert!(wrapped.iter().all(|line| !line.contains('…')));
    assert_eq!(wrapped.join(" "), command);
    assert_eq!(wrapped.join("\n").matches('|').count(), 6);
}

#[test]
fn wrap_shell_command_breaks_only_at_unquoted_spaces() {
    // The quoted span is atomic: `echo` overflows alone rather than the
    // pattern splitting across lines.
    assert_eq!(wrap_shell_command("echo \"a b c\" d", 10, 10), vec!["echo", "\"a b c\" d"]);
    assert_eq!(wrap_shell_command("   ", 5, 5), Vec::<String>::new());
}

#[test]
fn wrap_shell_command_splits_overlong_token_at_width() {
    assert_eq!(wrap_shell_command("abcdefghij", 4, 4), vec!["abcd", "efgh", "ij"]);
}

#[test]
fn wrap_shell_command_with_continuations_marks_wrapped_lines() {
    // Multi-line headers read as valid shell: every line except the last
    // ends with ` \`, single-line commands stay bare, blanks stay empty.
    assert_eq!(wrap_shell_command_with_continuations("echo a b c d", 7, 7), vec!["echo a \\", "b c d"]);
    assert_eq!(wrap_shell_command_with_continuations("git status --short", 62, 58), vec!["git status --short"]);
    assert!(wrap_shell_command_with_continuations("   ", 5, 5).is_empty());
    // Chained git commands (issue screenshot) keep every segment with no
    // `…` and mark each wrapped row as a continuation.
    let command = "git add a b && git commit -m \"msg\" && git status --short";
    let wrapped = wrap_shell_command_with_continuations(command, 20, 20);
    assert!(wrapped.len() > 1, "expected wrapping: {wrapped:?}");
    assert!(wrapped[..wrapped.len() - 1].iter().all(|line| line.ends_with(" \\")));
    assert!(!wrapped.last().unwrap().ends_with(" \\"));
    assert!(wrapped.iter().all(|line| !line.contains('…')));
    // Overlong chains break at operators with `&&` trailing, while
    // quoted `||` never splits.
    assert_eq!(wrap_shell_command_with_continuations("echo a && echo b", 10, 10), vec!["echo a && \\", "echo b"]);
    assert_eq!(wrap_shell_command_with_continuations("a | b | c", 7, 5), vec!["a | \\", "b | \\", "c"]);
    assert_eq!(
        wrap_shell_command_with_continuations("grep -rn \"a||b\" docs", 62, 58),
        vec!["grep -rn \"a||b\" docs"]
    );
    // Short chains that fit stay on one line (no forced splits).
    assert_eq!(wrap_shell_command_with_continuations("echo a && echo b", 62, 58), vec!["echo a && echo b"]);
    assert_eq!(wrap_shell_command_with_continuations("a | b | c", 62, 58), vec!["a | b | c"]);
    // The marker-free helper breaks identically minus suffixes.
    assert_eq!(wrap_shell_command_lines("echo a && echo b", 10, 10), vec!["echo a &&", "echo b"]);
    assert_eq!(wrap_shell_command_lines("git status", 62, 58), vec!["git status"]);
}

#[test]
fn wrap_shell_command_narrow_continuation_never_overflows() {
    // A token that fits the wider first line but not the narrower
    // continuation must hard-split after the line break instead of
    // emitting an over-budget row (62/58 production widths).
    let token = "b".repeat(60);
    let command = format!("aa {token} cc");
    let wrapped = wrap_shell_command(&command, 62, 58);
    assert!(wrapped.len() >= 3, "expected a split continuation: {wrapped:?}");
    assert!(wrapped[0].chars().count() <= 62, "first line budget: {wrapped:?}");
    for line in wrapped.iter().skip(1) {
        assert!(line.chars().count() <= 58, "continuation budget: {wrapped:?}");
    }
    // Same shape with a tiny budget for a fast unit check.
    let wrapped = wrap_shell_command("aa bbbbbbbb cc", 10, 5);
    assert_eq!(wrapped, vec!["aa", "bbbbb", "bbb", "cc"]);
}

#[test]
fn truncate_within_reserves_ellipsis_budget() {
    // Matches former runner::orchestration::truncate_chars behavior.
    assert_eq!(truncate_within("hello world", 8, "..."), "hello...");
    assert_eq!(truncate_within("hi", 8, "..."), "hi");
    // Single-char ellipsis reserves exactly one char (former snapshots /
    // session_archive behavior).
    assert_eq!(truncate_within("abcdef", 4, "…"), "abc…");
}

#[test]
fn truncate_within_counts_chars() {
    let jp = "あいうえお"; // 5 chars
    assert_eq!(truncate_within(jp, 5, "…"), jp);
    assert_eq!(truncate_within(jp, 3, "…"), "あい…");
}

#[test]
fn head_tail_truncate_keeps_both_ends() {
    let value = "0123456789".repeat(10); // 100 chars
    let (out, truncated) = head_tail_truncate(&value, 40, " ... [truncated] ... ");
    assert!(truncated);
    assert!(out.chars().count() <= 40);
    assert!(out.starts_with("012"));
    assert!(out.contains("[truncated]"));
    assert!(out.ends_with('9'));
}

#[test]
fn head_tail_truncate_passes_through_when_short() {
    let (out, truncated) = head_tail_truncate("short", 64, " ... ");
    assert_eq!(out, "short");
    assert!(!truncated);
}

#[test]
fn head_tail_truncate_small_budget_falls_back_to_prefix() {
    let marker = " ... [truncated] ... ";
    // max_chars <= marker_chars + 16 triggers the prefix fallback.
    // When max_chars (5) <= suffix_len (12), return just the prefix without suffix.
    let (out, truncated) = head_tail_truncate("abcdefghij", 5, marker);
    assert!(truncated);
    assert_eq!(out, "abcde");

    // When max_chars allows room for suffix, include it in the fallback branch.
    // Use max_chars=17 which is <= 21+16=37 (triggers fallback).
    let long_text = "abcdefghijklmnopqrstuvwxyz";
    let (out2, truncated2) = head_tail_truncate(long_text, 17, marker);
    assert!(truncated2);
    assert_eq!(out2, "abcde [truncated]");
    assert_eq!(out2.chars().count(), 17);
}

#[test]
fn truncate_text_counts_chars_not_bytes() {
    let jp = "あいうえお"; // 5 chars, 15 bytes
    assert_eq!(truncate_text(jp, 3, "…"), "あいう…");
    assert_eq!(truncate_text(jp, 5, "…"), "あいうえお");
}

#[test]
fn truncate_middle_keeps_both_ends() {
    assert_eq!(truncate_middle("short", 80), "short");
    assert_eq!(truncate_middle("abcdefghij", 5), "ab…ij");
    assert_eq!(truncate_middle("a b c", 80), "a b c");
    // Zero/one-char budgets.
    assert_eq!(truncate_middle("abc", 0), "");
    assert_eq!(truncate_middle("abc", 1), "…");
    // Control characters are sanitized to spaces before truncating.
    assert_eq!(truncate_middle("a\nb\tc", 80), "a b c");
}

#[test]
fn truncate_path_middle_breaks_at_separator() {
    assert_eq!(truncate_path_middle("src/lib.rs", 80), "src/lib.rs");
    assert_eq!(truncate_path_middle("foo/bar/baz/qux", 12), "foo…/qux");
    assert_eq!(truncate_path_middle("abc", 0), "");
}

#[test]
fn truncate_path_middle_counts_chars_not_bytes() {
    // Found by generative_tests: byte offsets from rfind/find overshot the budget.
    let out = truncate_path_middle("日本/日本語/日本語日本語/xyz", 6);
    assert!(out.chars().count() <= 6, "{out:?}");
    assert_eq!(truncate_path_middle("日本/日本語/xyz", 8), "日本…xyz");
}
